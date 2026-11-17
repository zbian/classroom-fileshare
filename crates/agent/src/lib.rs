//! 学员端：发现、加入、接收与重连对齐（PRD 6.x、7.5、FR-S-01..08 的 M1 子集）。
//!
//! M1 范围：**自动发现并连接**（E9：beacon → `(peer.ip, tcp_port)` 目标，
//! 支持 `room_filter` 房间码过滤；PRD 2.4 学生 1 步设昵称 / 3.1 自动发现）、
//! `manual_addr` 直连（RK-04 备用路径，优先级更高）、join（携带 PIN）、
//! 心跳 5s、接收教师广播（文本正文与文件都经文件帧传输，先落盘再通知——
//! PRD 7.2）、删除事件落地为墓碑并清理实体、断线自动重连并经 Manifest +
//! 重推对齐。学员回传（发送）是 M2（P1）内容。

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use classroom_net::{
    run_client_session, spawn_discovery_with_addr, Beacon, ClientHandler, ClientSessionConfig,
    FileChunk, FileEnd, FileHeader, Frame, Hello, SessionEnd,
};
use classroom_protocol::{
    Audience, Event, EventKind, ItemKind, MergeDecision, Role, ShareItem, SyncState,
    PROTOCOL_VERSION,
};
use classroom_store::{sha256_file, Store};

/// 学员端配置。
pub struct AgentConfig {
    /// 本机昵称（教师端分组主键，重复会被拒）。
    pub nickname: String,
    /// 房间 PIN（接口修订 #5②：计划遗漏，join 需携带）。
    pub pin: Option<String>,
    /// 手动教师机地址（RK-04 备用连接方式，优先级最高）；None 时自动发现。
    pub manual_addr: Option<SocketAddr>,
    /// 期望房间码（E9）：自动发现时仅连接匹配房间的信标；None 则连接
    /// 首个发现（单房间课堂）。来源 svc.json 的 room_code（安装时配置）。
    pub room_filter: Option<String>,
    /// 数据根目录。
    pub data_root: PathBuf,
}

/// 学员端通知事件。
#[derive(Debug)]
pub enum AgentEvent {
    /// 发现的教师机信标（GUI 弹窗展示房间码，B1）。
    Discovered(Vec<Beacon>),
    /// 已加入房间（`tcp://地址` 描述，manual 与自动发现两种来源一致）。
    Joined(String),
    /// 连接断开（随后自动重连）。
    Disconnected,
    /// 收到新条目（已落盘、可打开）。
    Received(ShareItem),
    /// 条目被教师删除（本地已清实体）。
    Removed(String),
    /// 版本清单已到达（对齐完成的第一阶段）。
    Synced,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn to_hex(bytes: &[u8]) -> String {
    classroom_store::files::to_hex(bytes)
}

/// 运行学员端：连接 → 会话 → 断线退避重连（1s 起、×2、封顶 5s）。
/// join 被拒（错 PIN / 重名 / 版本不匹配）是致命错误，直接返回 Err；
/// 连接失败与 IO 断线均为可重连状态。
///
/// 连接目标来源：`manual_addr` 优先（RK-04 备用路径）；否则 **自动发现**
/// （E9，PRD 2.4「学生 1 步设昵称」/ 3.1「自动发现并连接」）——
/// 后台 discovery 泵持续把最新信标的 `(peer.ip, tcp_port)` 写入 watch，
/// 连接循环每次重连都取最新目标（教师机换 IP/端口后自然跟上）。
pub async fn run_agent(cfg: AgentConfig, notify: mpsc::Sender<AgentEvent>) -> anyhow::Result<()> {
    // 兼容既有调用：无上行命令通道（rx 端立即关闭 → 命令泵空转退出）
    let (_tx, rx) = mpsc::channel(1);
    run_agent_with_commands(cfg, notify, rx).await
}

/// 学员端上行发送命令（svc → agent，M2 #104）。
///
/// 发送对象固定为教师（PRD 6.2 FR-S-02：学员不提供「发送给全班」）：
/// agent 构造的 `Event` 一律 `audience=Teacher` / `owner_id=本机昵称` /
/// `owner_role=Student`——owner 由 host 以**会话身份**权威校验（#94 契约），
/// 客户端只表达发送意图，不携带可信身份。
#[derive(Debug)]
pub enum AgentCommand {
    /// 发送文本给教师（正文经文件帧承载，与教师广播模型对称）。
    SendText {
        /// 标题（同名覆盖键）。
        title: String,
        /// 正文。
        body: String,
    },
    /// 发送文件给教师（多选；100MB 上限与风险类型由发送端预检 + host 权威复检）。
    SendFiles {
        /// 本地文件路径列表。
        paths: Vec<std::path::PathBuf>,
    },
}

/// 运行学员端并**提供上行发送命令通道**（M2 #104，#16 学员发送 UI 的后端入口）。
///
/// 命令经命令泵翻译为协议帧（Event + FileHeader/Chunk/End），投递到**当前
/// 已认证会话**的上行通道；未连接时命令被丢弃并 `warn`（离线补发归 #18）。
pub async fn run_agent_with_commands(
    cfg: AgentConfig,
    notify: mpsc::Sender<AgentEvent>,
    commands: mpsc::Receiver<AgentCommand>,
) -> anyhow::Result<()> {
    // S2：Store 经 Arc<Mutex<>> 传给会话回调（ClientHandler 要求 Send+Sync，
    // 而 Store 内含 rusqlite Connection（非 Sync）；锁只在同步临界区持有）。
    let store = Arc::new(Mutex::new(Store::open(&cfg.data_root)?));
    // 当前会话的上行发送端：连接建立时装入、会话结束时清空；命令泵据此
    // 判定「是否已连接」（net 层同一写任务，无第二套发送路径）
    let uplink: Arc<Mutex<Option<mpsc::Sender<Frame>>>> = Arc::new(Mutex::new(None));
    tokio::spawn(command_pump(commands, uplink.clone(), cfg.nickname.clone()));

    if let Some(addr) = cfg.manual_addr {
        return connect_loop(&cfg, &store, addr, &notify, &uplink).await;
    }

    // 自动发现：watch 保存最新连接目标；None = 尚未发现任何可用教师机
    let (target_tx, mut target_rx) = tokio::sync::watch::channel::<Option<SocketAddr>>(None);
    tokio::spawn(discovery_pump(
        cfg.room_filter.clone(),
        target_tx,
        notify.clone(),
    ));
    tracing::info!("未配置 manual_addr，进入自动发现模式");

    loop {
        // 等待发现目标（未发现时阻塞，不消耗 CPU）
        let target = loop {
            if let Some(t) = *target_rx.borrow() {
                break t;
            }
            if target_rx.changed().await.is_err() {
                anyhow::bail!("discovery 任务已退出");
            }
        };
        // connect_loop 仅在致命错误（join 被拒）时返回 `?` 上抛，
        // 正常路径（断线重连）永不返回——此处无需退避（PR #69 顺手项）
        connect_loop(&cfg, &store, target, &notify, &uplink).await?;
    }
}

/// 上行命令泵：`AgentCommand` → 协议帧 → 当前会话（#104）。
///
/// 设计说明（与 #17 的 `ClientHandler::on_frame(tx)` 关系）：两者最终都经
/// **同一个** `mpsc::Sender<Frame>` 写入（net 运行器的单一写任务），不存在
/// 两条写路径或乱序：
/// - `on_frame(tx)`：**会话内被动回应**（收到帧后的即时 Ack 等）；
/// - 本泵：**会话外主动上行**（GUI 触发的发送，跨重连由 uplink 槽更换端点）。
///
/// 内存策略（评审建议 1）：**流式构造 + 立即发送**——多选大文件不再把所有
/// 帧累计进内存（旧实现峰值 N×100MB），单文件峰值 ≤ 单文件大小；通道容量
/// （32 帧）提供背压。
async fn command_pump(
    mut commands: mpsc::Receiver<AgentCommand>,
    uplink: Arc<Mutex<Option<mpsc::Sender<Frame>>>>,
    nickname: String,
) {
    while let Some(cmd) = commands.recv().await {
        let sender = uplink.lock().unwrap().clone();
        let Some(sender) = sender else {
            // svc 在 IPC 应答前已按连接状态拦截；此处为竞态兜底
            tracing::warn!("未连接教师机，丢弃上行发送（离线补发归 #18）");
            continue;
        };
        if let Err(e) = stream_command(cmd, &nickname, &sender).await {
            tracing::warn!(error = %e, "上行发送失败（通道关闭或内容非法）");
        }
    }
    tracing::debug!("上行命令通道关闭，命令泵退出");
}

/// 把一条上行命令**流式**翻译为协议帧并立即发送。
async fn stream_command(
    cmd: AgentCommand,
    nickname: &str,
    tx: &mpsc::Sender<Frame>,
) -> anyhow::Result<()> {
    let now = unix_now();
    match cmd {
        AgentCommand::SendText { title, body } => {
            // PRD 10.3：标题/正文长度与合法性（发送端预检，host 侧再复检）
            classroom_protocol::validate_text(&title, &body)?;
            let bytes = body.as_bytes();
            let sha = sha256_bytes_hex(bytes);
            let item_id = uuid::Uuid::new_v4().to_string();
            let ev = upload_event(
                nickname,
                &item_id,
                ItemKind::Text,
                &title,
                body.chars().count() as u64,
                None,
                now,
            );
            send_frame(tx, Frame::Event(ev)).await?;
            send_content(tx, &item_id, &title, bytes, &sha).await?;
        }
        AgentCommand::SendFiles { paths } => {
            if paths.is_empty() {
                anyhow::bail!("未选择文件");
            }
            for path in paths {
                let meta = tokio::fs::metadata(&path).await?;
                if meta.len() > classroom_protocol::MAX_FILE_SIZE {
                    anyhow::bail!(
                        "单文件不能超过 100 MB: {}",
                        path.file_name()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                }
                let title = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("路径无有效文件名"))?;
                if classroom_protocol::is_risky_ext(&title) {
                    anyhow::bail!("高风险文件类型不允许发送: {title}");
                }
                let bytes = tokio::fs::read(&path).await?;
                let sha = sha256_bytes_hex(&bytes);
                let item_id = uuid::Uuid::new_v4().to_string();
                let ev = upload_event(
                    nickname,
                    &item_id,
                    ItemKind::File,
                    &title,
                    bytes.len() as u64,
                    Some(sha.clone()),
                    now,
                );
                send_frame(tx, Frame::Event(ev)).await?;
                send_content(tx, &item_id, &title, &bytes, &sha).await?;
                // bytes 在此 drop：单文件峰值释放后再读下一个（流式语义）
            }
        }
    }
    Ok(())
}

/// 构造学员上行事件（owner=本机昵称；host 以会话身份权威校验，#94 契约）。
fn upload_event(
    nickname: &str,
    item_id: &str,
    kind: ItemKind,
    title: &str,
    size: u64,
    sha256: Option<String>,
    now: i64,
) -> Event {
    Event {
        event_id: format!("up-{item_id}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: item_id.to_string(),
            kind,
            owner_id: nickname.to_string(),
            owner_role: Role::Student,
            audience: Audience::Teacher,
            title: title.to_string(),
            size,
            sha256,
            rel_path: None,
            created_at: now,
            updated_at: now,
            version: 1,
            deleted: false,
            sync_state: SyncState::PendingSync,
            retry_count: 0,
        },
        new_name: None,
        acked_by: None,
        ts: now,
    }
}

async fn send_frame(tx: &mpsc::Sender<Frame>, frame: Frame) -> anyhow::Result<()> {
    tx.send(frame)
        .await
        .map_err(|_| anyhow::anyhow!("上行通道已关闭"))
}

/// 内容帧序列：FileHeader → 1MB FileChunk × N → FileEnd（逐帧发送，不累计）。
async fn send_content(
    tx: &mpsc::Sender<Frame>,
    item_id: &str,
    file_name: &str,
    bytes: &[u8],
    sha: &str,
) -> anyhow::Result<()> {
    send_frame(
        tx,
        Frame::FileHeader(FileHeader {
            item_id: item_id.into(),
            file_name: file_name.into(),
            size: bytes.len() as u64,
            sha256: sha.into(),
        }),
    )
    .await?;
    for (seq, part) in bytes.chunks(classroom_protocol::CHUNK_SIZE).enumerate() {
        send_frame(
            tx,
            Frame::FileChunk(FileChunk {
                item_id: item_id.into(),
                seq: seq as u64,
                bytes: part.to_vec(),
            }),
        )
        .await?;
    }
    send_frame(
        tx,
        Frame::FileEnd(FileEnd {
            item_id: item_id.into(),
        }),
    )
    .await?;
    Ok(())
}
fn sha256_bytes_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

/// 后台发现泵：监听 beacon，更新最新连接目标并向 GUI 推送发现列表。
async fn discovery_pump(
    room_filter: Option<String>,
    target_tx: tokio::sync::watch::Sender<Option<SocketAddr>>,
    notify: mpsc::Sender<AgentEvent>,
) {
    let mut seen: Vec<Beacon> = Vec::new();
    loop {
        let (addr_tx, mut addr_rx) = mpsc::channel::<(SocketAddr, Beacon)>(16);
        let handle = spawn_discovery_with_addr(addr_tx);
        while let Some((peer, beacon)) = addr_rx.recv().await {
            let matches_room = room_filter
                .as_deref()
                .map(|want| want == beacon.room)
                .unwrap_or(true);
            if !seen.iter().any(|x| {
                x.room == beacon.room
                    && x.tcp_port == beacon.tcp_port
                    && x.host_name == beacon.host_name
            }) {
                seen.push(beacon.clone());
                let _ = notify.send(AgentEvent::Discovered(seen.clone())).await;
            }
            if matches_room {
                // 连接目标 = 信标来源 IP + 教师 TCP 端口（T-4 遗留补齐）。
                // 有 room_filter：同房间最新信标覆盖（教师换 IP/端口跟上）；
                // 无 filter：**锁定首个发现的目标**，避免多教师机广播时
                // 目标抖动（PR #69 顺手项）
                let target = SocketAddr::new(peer.ip(), beacon.tcp_port);
                if room_filter.is_some() || target_tx.borrow().is_none() {
                    let _ = target_tx.send(Some(target));
                }
            }
        }
        // discovery 任务结束（端口绑定失败/接收错误）：休息后重试监听
        handle.abort();
        tracing::warn!("discovery 中断，2s 后重试监听");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// 连接循环：会话 → 断线退避重连（连接失败视为可重连，不返回）。
async fn connect_loop(
    cfg: &AgentConfig,
    store: &Arc<Mutex<Store>>,
    addr: SocketAddr,
    notify: &mpsc::Sender<AgentEvent>,
    uplink: &Arc<Mutex<Option<mpsc::Sender<Frame>>>>,
) -> anyhow::Result<()> {
    let mut backoff = Duration::from_secs(1);
    loop {
        // 会话结束时的 Disconnected 由 agent_session 内部发出（先于清理
        // uplink，见其中注释）——此处不再重复通知，避免 svc 收到双份状态变更。
        agent_session(cfg, store, addr, notify, uplink).await?;
        tracing::info!(retry_in = ?backoff, %addr, "与教师机断开，稍后自动重连");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

/// 一次会话：S2 起由 net 客户端运行器承载控制面（建连超时 / 握手 / 心跳 /
/// Ping→Pong / 断线分类），本函数只构造配置与业务回调。
///
/// 语义映射（与 S1 之前的实现对照）：
/// - `SessionEnd::Rejected` → bail：join 被拒仍是致命（`join_rejection_is_fatal`
///   用例守护，run_agent 直接返回 Err）
/// - `SessionEnd::Closed` → `Ok(())`：断线语义不变，由 connect_loop 退避重连
/// - 运行器 Err（建连失败 / 业务回调失败）→ 记录 debug 后返回 `Ok(())` 重连；
///   原实现「连接失败可重连、回调失败致命」的差异在此统一为可重连（重连自带
///   1s→5s 退避，比一次性致命更健壮；join 拒绝仍致命）
async fn agent_session(
    cfg: &AgentConfig,
    store: &Arc<Mutex<Store>>,
    addr: SocketAddr,
    notify: &mpsc::Sender<AgentEvent>,
    uplink: &Arc<Mutex<Option<mpsc::Sender<Frame>>>>,
) -> anyhow::Result<()> {
    let hello = Hello::Join {
        protocol_version: PROTOCOL_VERSION,
        nickname: cfg.nickname.clone(),
        role: Role::Student,
        pin: cfg.pin.clone(),
    };
    let incoming = Arc::new(Mutex::new(Incoming::new(&cfg.data_root)));
    let handler = AgentSessionHandler {
        incoming: incoming.clone(),
        store: store.clone(),
        notify: notify.clone(),
        addr,
        nickname: cfg.nickname.clone(),
    };
    // 默认：心跳 5s、建连超时 5s、加入后自动请求版本清单（与原实现一致）
    // M2 #104：附加业务上行通道，并把发送端装入 uplink 槽——命令泵据此
    // 把 GUI 触发的发送投递到**当前会话**（重连后自动换新端点）。
    let (out_tx, out_rx) = mpsc::channel::<Frame>(32);
    *uplink.lock().unwrap() = Some(out_tx);
    let session_cfg = ClientSessionConfig::new(addr, hello).with_outbound(out_rx);

    let result = run_client_session(session_cfg, handler).await;
    // M2 #104 评审（顺序修正）：**先通知 Disconnected（svc 状态先变），再清
    // 上行端点**——原顺序会在两者之间留下窗口：svc 仍认为「已连接」→ 对
    // GUI 返回 Ok，而命令泵已看到 uplink=None 丢弃命令（静默成功）。先通知
    // 可保证 svc 一旦对外说「未连接」，命令泵必然也已拒绝。
    let _ = notify.send(AgentEvent::Disconnected).await;
    *uplink.lock().unwrap() = None;
    let end = match result {
        Ok(end) => end,
        Err(e) => {
            tracing::debug!(error = %e, %addr, "会话结束（将重连）");
            incoming.lock().unwrap().abandon();
            return Ok(());
        }
    };
    incoming.lock().unwrap().abandon();
    match end {
        SessionEnd::Rejected(code) => bail!("加入被教师机拒绝: {code:?}"),
        SessionEnd::Closed => Ok(()),
    }
}

/// 业务面回调：帧 → agent 接收状态机（逻辑与原手写循环逐行一致）。
///
/// 控制面（含 E3 的 Ping→Pong 应答）已由 net 运行器内置——这类缺陷从
/// 「角色层要记得写」变为「结构上不可能漏」。
struct AgentSessionHandler {
    incoming: Arc<Mutex<Incoming>>,
    store: Arc<Mutex<Store>>,
    notify: mpsc::Sender<AgentEvent>,
    addr: SocketAddr,
    /// 本机昵称（上行 Ack 的 `acked_by`，PRD 3.6）
    nickname: String,
}

#[async_trait]
impl ClientHandler for AgentSessionHandler {
    async fn on_joined(&self, _session_id: &str) {
        let _ = self
            .notify
            .send(AgentEvent::Joined(format!("tcp://{}", self.addr)))
            .await;
    }

    async fn on_frame(&self, frame: Frame, tx: &mut mpsc::Sender<Frame>) -> anyhow::Result<()> {
        match frame {
            Frame::Event(ev) => {
                // 锁只在同步临界区持有，不跨 await（MutexGuard 非 Send）
                let outcome = {
                    let mut incoming = self.incoming.lock().unwrap();
                    let store = self.store.lock().unwrap();
                    incoming.on_event(&store, ev)?
                };
                if let EvOutcome::Removed(id) = outcome {
                    let _ = self.notify.send(AgentEvent::Removed(id)).await;
                }
            }
            Frame::FileHeader(h) => {
                self.incoming.lock().unwrap().begin_file(h)?;
            }
            Frame::FileChunk(c) => {
                self.incoming.lock().unwrap().append_chunk(&c)?;
            }
            Frame::FileEnd(_) => {
                let item = {
                    let mut incoming = self.incoming.lock().unwrap();
                    let store = self.store.lock().unwrap();
                    incoming.finish_file(&store)?
                };
                if let Some(item) = item {
                    let _ = self.notify.send(AgentEvent::Received(item)).await;
                }
            }
            Frame::Manifest(entries) => {
                let (removed, acked) = {
                    let store = self.store.lock().unwrap();
                    align_manifest(&store, entries)?
                };
                for id in removed {
                    let _ = self.notify.send(AgentEvent::Removed(id)).await;
                }
                // M2 #17（PRD 3.6 ack / 7.4）：补删完成 → 上行 Ack 确认，
                // 教师端据此观测端侧补删进度（墓碑清理用 TTL，不依赖 ack）
                for item in acked {
                    let ev = Event {
                        event_id: format!("ack-{}-{}", item.id, item.version),
                        kind: EventKind::Ack,
                        item,
                        new_name: None,
                        acked_by: Some(self.nickname.clone()),
                        ts: unix_now(),
                    };
                    if tx.send(Frame::Event(ev)).await.is_err() {
                        tracing::debug!("Ack 发送失败（写任务已退出）");
                        break;
                    }
                }
                let _ = self.notify.send(AgentEvent::Synced).await;
            }
            other => tracing::debug!(frame = ?other, "agent 忽略该帧"),
        }
        Ok(())
    }
}

/// 清单对齐（PRD 7.4 删除同步状态机）：远端已删的条目在本地执行**两阶段
/// 补删**并返回 `(removed_ids, acked_items)`：
///
/// 1. **待删除**：本地未删 → `soft_delete`（`deleted=1` +
///    `sync_state=DeleteDeferred`）——该状态下条目已不可打开（GUI 由后续
///    issue 展示）；若本地已是墓碑（上次清理中断），直接进入第 3 步。
/// 2. **清实体**：删除正文/文件实体；失败仅告警（墓碑保留，下次重连重试）。
/// 3. **补删完成**：`hard_delete` 清索引行（条目彻底消失）→ 收集用于上行 Ack。
///
/// 幂等与有界性：墓碑已被 `hard_delete` 后 `store.get` 返回 None → 后续
/// manifest 比对直接跳过，**不会重复补删、不会无限重试**；每条目每次连接
/// 至多处理一次。
fn align_manifest(
    store: &Store,
    entries: Vec<(String, u64, bool)>,
) -> anyhow::Result<(Vec<String>, Vec<ShareItem>)> {
    let mut removed = Vec::new();
    let mut acked = Vec::new();
    for (id, version, deleted) in entries {
        if !deleted {
            continue;
        }
        let Some(local) = store.get(&id)? else {
            continue; // 已补删完成（或从未有过）：幂等跳过
        };
        let now = unix_now();
        let tombstone = if local.deleted {
            // 上次清理中断（标记已落、索引未清）：完成第 3 步
            local
        } else {
            // 阶段 1：标记待删除（DeleteDeferred，不可打开）
            let marked = store.soft_delete(&id, now)?;
            // 阶段 2：清实体（失败仅告警，墓碑保留供下次重试）
            if let Ok(body) = store.file_body(&marked) {
                if body.exists() {
                    if let Err(e) = std::fs::remove_file(&body) {
                        tracing::warn!(item_id = %id, error = %e, "补删实体失败，墓碑保留待重试");
                    }
                }
            }
            marked
        };
        // 阶段 3：清索引行（补删完成）
        store.hard_delete(&id)?;
        tracing::info!(item_id = %id, version, "延迟补删完成");
        removed.push(id);
        let mut item = tombstone;
        item.version = version.max(item.version);
        acked.push(item);
    }
    Ok((removed, acked))
}

/// 事件处理结果。
enum EvOutcome {
    Removed(String),
    Ignored,
}

/// 接收状态机：一条待落盘的 upsert（事件先行）+ 至多一条活跃文件流。
///
/// 不持有 `&Store`（rusqlite 连接非 Sync，跨 await 会破坏 future 的 Send），
/// 各方法同步执行、以参数借用 store。
struct Incoming {
    repo: PathBuf,
    incoming_dir: PathBuf,
    pending_ev: Option<Event>,
    active: Option<ActiveFile>,
}

struct ActiveFile {
    header: FileHeader,
    path: PathBuf,
    file: std::fs::File,
    hasher: Sha256,
}

impl Incoming {
    fn new(data_root: &Path) -> Self {
        Self {
            repo: data_root.join("repo"),
            incoming_dir: data_root.join("repo").join(".incoming"),
            pending_ev: None,
            active: None,
        }
    }

    /// 同步事件：upsert 先记 pending（等文件流落盘后入库），delete 立即生效。
    fn on_event(&mut self, store: &Store, ev: Event) -> anyhow::Result<EvOutcome> {
        match ev.kind {
            EventKind::Upsert => {
                if self.pending_ev.is_some() {
                    tracing::warn!("上一条 upsert 尚未完成文件流，被覆盖");
                }
                self.pending_ev = Some(ev);
                Ok(EvOutcome::Ignored)
            }
            EventKind::Delete => {
                let gone = match store.apply_remote(&ev) {
                    Ok(MergeDecision::Overwrite(item)) => item,
                    Ok(_) => return Ok(EvOutcome::Ignored),
                    Err(e) => return Err(e.into()),
                };
                if let Ok(body) = store.file_body(&gone) {
                    if body.exists() {
                        let _ = std::fs::remove_file(&body);
                    }
                }
                Ok(EvOutcome::Removed(gone.id))
            }
            EventKind::Rename | EventKind::Ack => {
                tracing::debug!(kind = ?ev.kind, "M1 忽略该事件类型");
                Ok(EvOutcome::Ignored)
            }
        }
    }

    fn begin_file(&mut self, header: FileHeader) -> anyhow::Result<()> {
        if self.pending_ev.is_none() {
            tracing::warn!(item_id = %header.item_id, "文件流缺少前置事件，丢弃");
            self.active = None;
            return Ok(());
        }
        std::fs::create_dir_all(&self.incoming_dir)?;
        let path = self.incoming_dir.join(format!("{}.part", header.item_id));
        let file = std::fs::File::create(&path)?;
        self.active = Some(ActiveFile {
            header,
            path,
            file,
            hasher: Sha256::new(),
        });
        Ok(())
    }

    fn append_chunk(&mut self, chunk: &classroom_net::FileChunk) -> anyhow::Result<()> {
        let Some(act) = self.active.as_mut() else {
            return Ok(()); // 无活跃流（缺前置事件时静默吞块，已在 begin 警告）
        };
        if act.header.item_id != chunk.item_id {
            tracing::warn!(item_id = %chunk.item_id, "分块 item_id 与活跃流不符，丢弃");
            return Ok(());
        }
        use std::io::Write as _;
        act.file.write_all(&chunk.bytes)?;
        act.hasher.update(&chunk.bytes);
        Ok(())
    }

    /// 文件完成：校验整体 SHA-256 → 安装（同内容跳过；旧版转唯一 .bak）→
    /// 以教师端 id/rel_path 幂等入索引（PRD 7.2 先落盘再提醒）。
    fn finish_file(&mut self, store: &Store) -> anyhow::Result<Option<ShareItem>> {
        let Some(act) = self.active.take() else {
            return Ok(None);
        };
        let ActiveFile {
            header,
            path,
            file,
            hasher,
        } = act;
        drop(file);
        let actual = to_hex(&hasher.finalize());
        if !actual.eq_ignore_ascii_case(&header.sha256) {
            let _ = std::fs::remove_file(&path);
            self.pending_ev = None;
            tracing::warn!(item_id = %header.item_id, "整体哈希校验失败，丢弃（EX-07）");
            return Ok(None);
        }
        let Some(ev) = self.pending_ev.take() else {
            let _ = std::fs::remove_file(&path);
            tracing::warn!("文件完成但对应事件缺失，丢弃");
            return Ok(None);
        };
        self.install(&ev.item, &header, &path)?;
        // E4（#14 TC-02 计时锚点）：收到（校验安装完成、即将入索引）
        // E6：size 用 item.size（PRD 3.5：文本 size=字符数、文件=字节数），
        // 与 broadcast 侧同源，证据表列自洽
        tracing::info!(
            item_id = %ev.item.id,
            file = %header.file_name,
            size = ev.item.size,
            from = %ev.item.owner_id,
            "received"
        );
        match store.apply_remote(&ev) {
            Ok(MergeDecision::Overwrite(applied)) => {
                // E4：落盘成功（Δ=applied−broadcast 供 qa 计时）
                tracing::info!(item_id = %applied.id, version = applied.version, "applied");
                Ok(Some(*applied))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 把校验通过的 `.part` 安装到 `rel_path` 指示的仓库位置。
    fn install(&self, item: &ShareItem, header: &FileHeader, part: &Path) -> anyhow::Result<()> {
        let rel = item
            .rel_path
            .as_deref()
            .ok_or_else(|| anyhow!("事件缺少 rel_path"))?;
        if Path::new(rel).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            bail!("rel_path 含逃逸成分: {rel}");
        }
        let final_path = self.repo.join(rel);
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // 内容一致（重连重推）直接跳过，避免产生多余 .bak
        let unchanged = final_path.exists()
            && sha256_file(&final_path)
                .map(|h| h.eq_ignore_ascii_case(&header.sha256))
                .unwrap_or(false);
        if unchanged {
            let _ = std::fs::remove_file(part);
            return Ok(());
        }
        if final_path.exists() {
            let mut bak = final_path.as_os_str().to_os_string();
            bak.push(".bak");
            std::fs::rename(&final_path, PathBuf::from(bak))?;
        }
        std::fs::rename(part, &final_path)?;
        Ok(())
    }

    /// 会话结束清理未完成的 `.part`。
    fn abandon(&mut self) {
        if let Some(act) = self.active.take() {
            drop(act.file);
            let _ = std::fs::remove_file(&act.path);
        }
        self.pending_ev = None;
    }
}

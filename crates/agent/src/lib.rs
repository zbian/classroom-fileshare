//! 学员端：发现、加入、接收与重连对齐（PRD 6.x、7.5、FR-S-01..08 的 M1 子集）。
//!
//! M1 范围：**自动发现并连接**（E9：beacon → `(peer.ip, tcp_port)` 目标，
//! 支持 `room_filter` 房间码过滤；PRD 2.4 学生 1 步设昵称 / 3.1 自动发现）、
//! `manual_addr` 直连（RK-04 备用路径，优先级更高）、join（携带 PIN）、
//! 心跳 5s、接收教师广播（文本正文与文件都经文件帧传输，先落盘再通知——
//! PRD 7.2）、删除事件落地为墓碑并清理实体、断线自动重连并经 Manifest +
//! 重推对齐。学员回传（发送）是 M2（P1）内容。

pub mod pending;

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

use pending::{PendingKind, PendingOp, PendingQueue};

/// E15（#123）：已发送待确认表——`op_id → 发送时所在的会话 epoch`。
///
/// 帧写入通道 ≠ host 已收到（无 ack，见 S5）：因此发送成功后**不出队**，
/// 由下一次泵 tick 在「会话仍就绪且 epoch 未变」时确认出队；会话切换/断开
/// 时清除标记并由补发泵重发（host 端事件幂等/同名覆盖保证不重复条目）。
type SentTracker = Arc<Mutex<std::collections::HashMap<String, u64>>>;

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
    /// 待发队列变化（M2 #18，PRD 6.8）：GUI/状态条展示「待发送 N 条」。
    PendingOpsChanged {
        /// 队列长度。
        count: u32,
        /// 待发内容总字节数。
        bytes: u64,
    },
    /// 待发缓存超过阈值（PRD 6.8/9.2）：提示清理，**不自动删除**未同步内容。
    PendingCacheFull {
        /// 当前占用字节数。
        bytes: u64,
        /// 阈值。
        limit: u64,
    },
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
    // M2 #18：离线待发队列（持久化在 data_root/pending/；断电不丢）
    let pending = Arc::new(Mutex::new(PendingQueue::open(&cfg.data_root)));
    // E15（#123）：会话就绪信号——仅 `Joined` 后为 true（net 层的 Joined
    // 事件复用，不另造机制）；epoch 每次新会话 +1，用于「发送待确认」判定。
    let joined = Arc::new(AtomicBool::new(false));
    let epoch = Arc::new(AtomicU64::new(0));
    let sent: SentTracker = Arc::new(Mutex::new(std::collections::HashMap::new()));
    tokio::spawn(command_pump(
        commands,
        uplink.clone(),
        pending.clone(),
        notify.clone(),
        cfg.nickname.clone(),
        cfg.data_root.clone(),
        joined.clone(),
        epoch.clone(),
        sent.clone(),
        store.clone(),
    ));
    // M2 #18：补发泵——连接可用时按操作时间顺序补做离线操作（失败退避，
    // 单条失败不阻塞队列；PRD 6.8）
    tokio::spawn(resend_pump(
        pending.clone(),
        uplink.clone(),
        notify.clone(),
        cfg.nickname.clone(),
        cfg.data_root.clone(),
        joined.clone(),
        epoch.clone(),
        sent.clone(),
        store.clone(),
    ));

    if let Some(addr) = cfg.manual_addr {
        return connect_loop(&cfg, &store, addr, &notify, &uplink, &joined, &epoch).await;
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
        connect_loop(&cfg, &store, target, &notify, &uplink, &joined, &epoch).await?;
    }
}

/// 待发缓存阈值（PRD 6.8 / 9.2）：超过后提示清理，**不自动删除**未同步内容。
pub const PENDING_CACHE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

/// 上行命令泵：`AgentCommand` → **先持久化待发内容** → 尝试立即发送；
/// 未连接/发送失败则留在待发队列，由补发泵（[`resend_pump`]）在重连后补做
/// （M2 #18，PRD 6.8）。
///
/// 设计说明（与 #17 的 `ClientHandler::on_frame(tx)` 关系）：两者最终都经
/// **同一个** `mpsc::Sender<Frame>` 写入（net 运行器的单一写任务），不存在
/// 两条写路径或乱序：
/// - `on_frame(tx)`：**会话内被动回应**（收到帧后的即时 Ack 等）；
/// - 本泵：**会话外主动上行**（GUI 触发的发送，跨重连由 uplink 槽更换端点）。
///
/// 内存策略（#104 评审建议 1）：**流式构造 + 立即发送**——单文件峰值 ≤
/// 单文件大小；通道容量（32 帧）提供背压。
#[allow(clippy::too_many_arguments)]
async fn command_pump(
    mut commands: mpsc::Receiver<AgentCommand>,
    uplink: Arc<Mutex<Option<mpsc::Sender<Frame>>>>,
    pending: Arc<Mutex<PendingQueue>>,
    notify: mpsc::Sender<AgentEvent>,
    nickname: String,
    data_root: std::path::PathBuf,
    joined: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    sent: SentTracker,
    store: Arc<Mutex<Store>>,
) {
    while let Some(cmd) = commands.recv().await {
        // M2 #18：先落盘（PendingOp 持久化，断电不丢；PRD 6.8/9.2），
        // 再尝试立即发送；成功后从队列移除，失败留给补发泵
        let ops = match prepare_ops(cmd, &data_root, &store, &nickname).await {
            Ok(ops) => ops,
            Err(e) => {
                tracing::warn!(error = %e, "上行内容准备失败（未入队）");
                continue;
            }
        };
        {
            let mut q = pending.lock().unwrap();
            for op in &ops {
                q.push(op.clone());
            }
        }
        notify_pending(&notify, &pending).await;
        check_cache_threshold(&notify, &pending).await;

        // E15（#123）：先确认**上一轮已发送**的条目（会话就绪且 epoch 未变才
        // 出队）；会话未就绪/已切换 → 清标记留队列，绝不在「未 Joined」时
        // 标记成功。
        let session_ready = joined.load(Ordering::SeqCst) && uplink.lock().unwrap().is_some();
        let cur_epoch = epoch.load(Ordering::SeqCst);
        let confirmed = confirm_sent(&pending, &sent, session_ready, cur_epoch);
        if confirmed > 0 {
            // M2 #128：出队后必须补报队列长度（否则 GUI pending_count 不归零）
            notify_pending(&notify, &pending).await;
        }

        if !session_ready {
            tracing::info!(
                count = ops.len(),
                "会话未就绪（未 Joined），已入待发队列，等待会话就绪后补发"
            );
            continue;
        }
        // 会话就绪：尝试立即发送（成功仅登记「待确认」，下一轮确认后才出队）
        let sender = uplink.lock().unwrap().clone();
        let Some(sender) = sender else {
            continue;
        };
        for op in ops {
            match stream_op(&op, &data_root, &nickname, &sender).await {
                Ok(()) => {
                    // M2 #121：发送成功后本地落库（「我共享的」可见）
                    record_local_submission(&store, &op, &data_root, &nickname, &notify).await;
                    sent.lock().unwrap().insert(op.id.clone(), cur_epoch);
                    tracing::info!(op_id = %op.id, epoch = cur_epoch, "上行发送完成，待下一轮确认");
                }
                Err(e) => {
                    tracing::warn!(op_id = %op.id, error = %e, "上行发送失败，留待补发");
                    break; // 会话已断：后续条目由补发泵按序处理
                }
            }
        }
        notify_pending(&notify, &pending).await;
    }
    tracing::debug!("上行命令通道关闭，命令泵退出");
}

/// E15（#123）确认出队：仅当会话就绪且 `sent` 标记的 epoch 与当前会话一致
/// 时把条目移出队列；否则清除标记（条目保留，由补发泵重发）。
///
/// 返回值 = 本次**确认出队**的条目数（M2 #128：>0 时调用方需补报
/// `PendingOpsChanged`，否则补发完成后 `pending_count` 不会归零）。
fn confirm_sent(
    pending: &Arc<Mutex<PendingQueue>>,
    sent: &SentTracker,
    session_ready: bool,
    cur_epoch: u64,
) -> usize {
    let mut sent_map = sent.lock().unwrap();
    if !session_ready {
        // 会话不可用：所有待确认项重新排队（内容仍在，等待重发）
        if !sent_map.is_empty() {
            tracing::info!(count = sent_map.len(), "会话不可用，已发送待确认项退回队列");
            sent_map.clear();
        }
        return 0;
    }
    let confirmed: Vec<String> = sent_map
        .iter()
        .filter(|(_, ep)| **ep == cur_epoch)
        .map(|(id, _)| id.clone())
        .collect();
    let confirmed_count = confirmed.len();
    for id in confirmed {
        pending.lock().unwrap().complete(&id);
        sent_map.remove(&id);
        tracing::info!(op_id = %id, "待发条目已确认出队");
    }
    // 旧会话（epoch 不同）的标记：清除，下轮重发
    sent_map.retain(|_, ep| *ep == cur_epoch);
    confirmed_count
}

/// 把一条命令拆成待发操作并在 `data_root/pending/` 落盘内容（PRD 9.2 本地缓存）。
///
/// M2 #121：同时解析**本地条目身份**（item_id/version）——同名已有条目 →
/// 复用其 id + version+1，保证学员本地「我共享的」与教师端 id/version 一致。
///
/// - 文本：`validate_text` 预检（PRD 10.3），内容 = 正文 UTF-8 字节
/// - 文件：≤100MB（E10）+ 风险类型（PRD 5.3）预检，内容 = 文件字节
async fn prepare_ops(
    cmd: AgentCommand,
    data_root: &std::path::Path,
    store: &Arc<Mutex<Store>>,
    nickname: &str,
) -> anyhow::Result<Vec<PendingOp>> {
    let dir = pending::pending_dir(data_root);
    tokio::fs::create_dir_all(&dir).await?;
    let now = unix_now();
    let mut ops = Vec::new();
    match cmd {
        AgentCommand::SendText { title, body } => {
            classroom_protocol::validate_text(&title, &body)?;
            let (item_id, version) = resolve_identity(store, nickname, &title);
            ops.push(write_pending_content(
                data_root,
                PendingKind::Text,
                &title,
                body.as_bytes().to_vec(),
                now,
                item_id,
                version,
            )?);
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
                let (item_id, version) = resolve_identity(store, nickname, &title);
                ops.push(write_pending_content(
                    data_root,
                    PendingKind::File,
                    &title,
                    bytes,
                    now,
                    item_id,
                    version,
                )?);
            }
        }
    }
    Ok(ops)
}

/// 解析本地条目身份（M2 #121）：同名（本机昵称 + `audience=Teacher`）已有
/// 条目 → 复用其 `id` 与 `version+1`（与教师端同名覆盖语义一致）；否则新
/// `id` + `version = 1`。
fn resolve_identity(store: &Arc<Mutex<Store>>, nickname: &str, title: &str) -> (String, u64) {
    match store
        .lock()
        .unwrap()
        .find_by_scope(nickname, Audience::Teacher, title)
    {
        Ok(Some(old)) => (old.id, old.version + 1),
        _ => (uuid::Uuid::new_v4().to_string(), 1),
    }
}

/// 写入待发内容文件并构造队列项（`pending/<id>.bin`）。
#[allow(clippy::too_many_arguments)]
fn write_pending_content(
    data_root: &std::path::Path,
    kind: PendingKind,
    title: &str,
    bytes: Vec<u8>,
    now: i64,
    item_id: String,
    version: u64,
) -> anyhow::Result<PendingOp> {
    let id = uuid::Uuid::new_v4().to_string();
    let sha = sha256_bytes_hex(&bytes);
    let content_rel = format!("pending/{id}.bin");
    std::fs::write(data_root.join(&content_rel), &bytes)?;
    Ok(PendingOp {
        id,
        item_id,
        version,
        kind,
        title: title.to_string(),
        content_rel,
        size: bytes.len() as u64,
        sha256: sha,
        created_at: now,
        attempts: 0,
        next_at: 0, // 立即可发
    })
}

/// 按待发操作发送（内容从 `data_root/<content_rel>` 流式读取；先复校验哈希）。
///
/// `event_id` 由 op.id 确定性生成（`up-{op.id}`）——同一操作重发时 host 端
/// 事件去重/同名覆盖保证**不产生重复条目**（TC-09）。
async fn stream_op(
    op: &PendingOp,
    data_root: &std::path::Path,
    nickname: &str,
    tx: &mpsc::Sender<Frame>,
) -> anyhow::Result<()> {
    let bytes = tokio::fs::read(data_root.join(&op.content_rel)).await?;
    if sha256_bytes_hex(&bytes) != op.sha256 {
        anyhow::bail!("待发内容校验失败（哈希不符）");
    }
    let now = unix_now();
    let (kind, size) = match op.kind {
        PendingKind::Text => (
            ItemKind::Text,
            String::from_utf8_lossy(&bytes).chars().count() as u64,
        ),
        PendingKind::File => (ItemKind::File, bytes.len() as u64),
    };
    let sha = op.sha256.clone();
    let ev = upload_event(
        nickname,
        &op.item_id,
        kind,
        &op.title,
        size,
        match op.kind {
            PendingKind::File => Some(sha.clone()),
            PendingKind::Text => None,
        },
        op.version,
        now,
    );
    send_frame(tx, Frame::Event(ev)).await?;
    send_content(tx, &op.item_id, &op.title, &bytes, &sha).await
}

/// 补发泵（M2 #18，PRD 6.8）：连接可用时按**操作时间顺序**补发到期项；
/// 单条失败按退避重试（1s 起 ×2，封顶 60s）并记次数，**不阻塞其他条目**。
///
/// E15（#123）：**仅在会话 `Joined` 后**才发送；发送成功仅登记「待确认」，
/// 下一次 tick 在会话仍就绪且 epoch 未变时确认出队——避免「未就绪会话
/// 发送成功但 host 未收到」的静默丢件。
#[allow(clippy::too_many_arguments)]
async fn resend_pump(
    pending: Arc<Mutex<PendingQueue>>,
    uplink: Arc<Mutex<Option<mpsc::Sender<Frame>>>>,
    notify: mpsc::Sender<AgentEvent>,
    nickname: String,
    data_root: std::path::PathBuf,
    joined: Arc<AtomicBool>,
    epoch: Arc<AtomicU64>,
    sent: SentTracker,
    store: Arc<Mutex<Store>>,
) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let session_ready = joined.load(Ordering::SeqCst) && uplink.lock().unwrap().is_some();
        let cur_epoch = epoch.load(Ordering::SeqCst);
        // 先确认上轮发送（会话就绪且 epoch 未变才出队）
        let confirmed = confirm_sent(&pending, &sent, session_ready, cur_epoch);
        if confirmed > 0 {
            // M2 #128：出队后必须补报队列长度（否则 GUI pending_count 不归零）
            notify_pending(&notify, &pending).await;
        }
        if !session_ready {
            continue; // 未 Joined：保持队列，等会话就绪
        }
        let Some(sender) = uplink.lock().unwrap().clone() else {
            continue;
        };
        let due = pending.lock().unwrap().due(unix_now());
        for op in due {
            // 已发送待确认的条目不重复发送（等确认或断线后重发）
            if sent.lock().unwrap().contains_key(&op.id) {
                continue;
            }
            match stream_op(&op, &data_root, &nickname, &sender).await {
                Ok(()) => {
                    // M2 #121：补发送成功同样本地落库（「我共享的」可见）
                    record_local_submission(&store, &op, &data_root, &nickname, &notify).await;
                    sent.lock().unwrap().insert(op.id.clone(), cur_epoch);
                    tracing::info!(
                        op_id = %op.id,
                        created_at = op.created_at,
                        epoch = cur_epoch,
                        "延迟补发送完成，待下一轮确认"
                    );
                    notify_pending(&notify, &pending).await;
                }
                Err(e) => {
                    let attempts = pending.lock().unwrap().retry_later(&op.id, unix_now());
                    tracing::warn!(op_id = %op.id, attempts, error = %e, "补发送失败，退避重试");
                    // 单条失败不阻塞队列：继续处理下一条
                }
            }
        }
    }
}

/// 发送成功后本地落库（M2 #121，PRD 6.x「我共享的」）：`id/version` 与上行
/// Event 一致（`op.item_id`/`op.version`），初始 `PendingSync`；host
/// `FileAck{sha256_ok:true}` 到达后置 `Synced`（见 `on_frame` 的 FileAck 分支）。
async fn record_local_submission(
    store: &Arc<Mutex<Store>>,
    op: &PendingOp,
    data_root: &std::path::Path,
    nickname: &str,
    notify: &mpsc::Sender<AgentEvent>,
) {
    let content = match tokio::fs::read(data_root.join(&op.content_rel)).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(op_id = %op.id, error = %e, "本地落库读取内容失败");
            return;
        }
    };
    let now = unix_now();
    let (kind, size, sha) = match op.kind {
        PendingKind::Text => {
            // PRD 3.5：文本 size = 字符数
            let body = String::from_utf8_lossy(&content);
            (ItemKind::Text, body.chars().count() as u64, None)
        }
        PendingKind::File => (
            ItemKind::File,
            content.len() as u64,
            Some(op.sha256.clone()),
        ),
    };
    let result = store.lock().unwrap().put_entry_with_id(
        &op.item_id,
        nickname,
        Role::Student,
        Audience::Teacher,
        &op.title,
        kind,
        &content,
        size,
        sha.as_deref(),
        op.version,
        SyncState::PendingSync,
        now,
    );
    match result {
        Ok(item) => {
            tracing::info!(
                item_id = %item.id,
                version = item.version,
                "本地落库（PendingSync，等待教师确认）"
            );
            let _ = notify.send(AgentEvent::Received(item)).await;
        }
        Err(e) => tracing::warn!(item_id = %op.item_id, error = %e, "本地落库失败"),
    }
}
/// 推送待发队列状态（GUI 状态条「待发送 N 条」）。
async fn notify_pending(notify: &mpsc::Sender<AgentEvent>, pending: &Arc<Mutex<PendingQueue>>) {
    let (count, bytes) = {
        let q = pending.lock().unwrap();
        (q.len() as u32, q.total_bytes())
    };
    let _ = notify
        .send(AgentEvent::PendingOpsChanged { count, bytes })
        .await;
}

/// 缓存阈值检查（PRD 6.8/9.2）：超阈值只**提示清理**，不自动删除。
async fn check_cache_threshold(
    notify: &mpsc::Sender<AgentEvent>,
    pending: &Arc<Mutex<PendingQueue>>,
) {
    let bytes = pending.lock().unwrap().total_bytes();
    if bytes > PENDING_CACHE_LIMIT_BYTES {
        let _ = notify
            .send(AgentEvent::PendingCacheFull {
                bytes,
                limit: PENDING_CACHE_LIMIT_BYTES,
            })
            .await;
    }
}
/// 构造学员上行事件（owner=本机昵称；host 以会话身份权威校验，#94 契约）。
#[allow(clippy::too_many_arguments)]
fn upload_event(
    nickname: &str,
    item_id: &str,
    kind: ItemKind,
    title: &str,
    size: u64,
    sha256: Option<String>,
    version: u64,
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
            version,
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
    joined: &Arc<AtomicBool>,
    epoch: &Arc<AtomicU64>,
) -> anyhow::Result<()> {
    let mut backoff = Duration::from_secs(1);
    loop {
        // 会话结束时的 Disconnected 由 agent_session 内部发出（先于清理
        // uplink，见其中注释）——此处不再重复通知，避免 svc 收到双份状态变更。
        agent_session(cfg, store, addr, notify, uplink, joined, epoch).await?;
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
    joined: &Arc<AtomicBool>,
    epoch: &Arc<AtomicU64>,
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
        joined: joined.clone(),
        epoch: epoch.clone(),
    };
    // 默认：心跳 5s、建连超时 5s、加入后自动请求版本清单（与原实现一致）
    // M2 #104：附加业务上行通道，并把发送端装入 uplink 槽——命令泵据此
    // 把 GUI 触发的发送投递到**当前会话**（重连后自动换新端点）。
    let (out_tx, out_rx) = mpsc::channel::<Frame>(32);
    *uplink.lock().unwrap() = Some(out_tx);
    let session_cfg = ClientSessionConfig::new(addr, hello).with_outbound(out_rx);

    let result = run_client_session(session_cfg, handler).await;
    // E15（#123）顺序：**先置会话不可就绪（joined=false）**，再通知
    // Disconnected、最后清 uplink——保证泵不会在「会话已断但仍被判定就绪」
    // 的窗口内发送并误确认出队。
    joined.store(false, Ordering::SeqCst);
    // M2 #104 评审（顺序修正）：先通知 Disconnected（svc 状态先变），再清
    // 上行端点——消除「svc 认为已连接但命令泵已丢弃」的静默成功窗口。
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
    /// E15（#123）：会话就绪信号（仅 Joined 后 true；结束前 false）。
    joined: Arc<AtomicBool>,
    /// E15（#123）：会话 epoch（每次 Joined +1，用于发送待确认判定）。
    epoch: Arc<AtomicU64>,
}

#[async_trait]
impl ClientHandler for AgentSessionHandler {
    async fn on_joined(&self, _session_id: &str) {
        // E15（#123）：会话就绪——新会话递增 epoch（旧会话的「已发送待确认」
        // 标记随之失效，由补发泵重发，避免跨会话误确认出队）。
        // 顺序（评审修正）：**先递增 epoch，再置就绪**——泵读到 joined=true
        // 时 epoch 必已是新值，避免「就绪但 epoch 仍旧」窗口内 confirm_sent
        // 把旧会话标记误判为「epoch 未变」而错误出队（数据丢失风险）。
        self.epoch.fetch_add(1, Ordering::SeqCst);
        self.joined.store(true, Ordering::SeqCst);
        tracing::info!(
            epoch = self.epoch.load(Ordering::SeqCst),
            "会话已就绪（Joined）"
        );
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
                match outcome {
                    EvOutcome::Removed(id) => {
                        let _ = self.notify.send(AgentEvent::Removed(id)).await;
                    }
                    EvOutcome::Renamed(item) => {
                        let _ = self.notify.send(AgentEvent::Received(item)).await;
                    }
                    EvOutcome::Ignored => {}
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
            // M2 #121：host 对学员上传的确认 → 本地条目 PendingSync → Synced
            Frame::FileAck(ack) => {
                if ack.sha256_ok {
                    let synced = self.store.lock().unwrap().mark_synced(&ack.item_id);
                    match synced {
                        Ok(Some(item)) => {
                            tracing::info!(
                                item_id = %item.id,
                                "FileAck：本地提交已确认（Synced）"
                            );
                            let _ = self.notify.send(AgentEvent::Received(item)).await;
                        }
                        Ok(None) => {
                            tracing::debug!(item_id = %ack.item_id, "FileAck 无对应本地条目")
                        }
                        Err(e) => tracing::warn!(error = %e, "FileAck 更新 Synced 失败"),
                    }
                } else {
                    tracing::warn!(
                        item_id = %ack.item_id,
                        "FileAck 校验失败（本地条目保持 PendingSync）"
                    );
                }
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
    /// 重命名已应用（M2 #19）：GUI 刷新列表且条目按新名称展示。
    Renamed(ShareItem),
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
            // M2 #19：重命名——本地实体/`.bak` 改名后应用新条目（version+1）。
            EventKind::Rename => {
                let Some(new_name) = ev.new_name.clone() else {
                    tracing::warn!(item_id = %ev.item.id, "rename 事件缺少 new_name，忽略");
                    return Ok(EvOutcome::Ignored);
                };
                // 旧条目（可能不存在：首见即 rename 的边缘场景 → 直接应用）
                let old_body = store
                    .get(&ev.item.id)
                    .ok()
                    .flatten()
                    .and_then(|old| store.file_body(&old).ok());
                let item = match store.apply_remote(&ev) {
                    Ok(MergeDecision::Overwrite(item)) => *item,
                    Ok(_) => {
                        tracing::debug!(item_id = %ev.item.id, "rename 版本落后/重复，忽略");
                        return Ok(EvOutcome::Ignored);
                    }
                    Err(e) => return Err(e.into()),
                };
                // 实体改名（旧路径存在且与新路径不同；索引已更新，改名失败仅告警）
                if let Some(old_body) = old_body {
                    if let Ok(new_body) = store.file_body(&item) {
                        if old_body != new_body && old_body.exists() {
                            if let Err(e) = std::fs::rename(&old_body, &new_body) {
                                tracing::warn!(
                                    error = %e,
                                    old = %old_body.display(),
                                    new = %new_body.display(),
                                    "rename 本地实体改名失败（索引已更新）"
                                );
                            } else {
                                let mut old_bak = old_body.as_os_str().to_os_string();
                                old_bak.push(".bak");
                                let old_bak = std::path::PathBuf::from(old_bak);
                                if old_bak.exists() {
                                    let mut new_bak = new_body.as_os_str().to_os_string();
                                    new_bak.push(".bak");
                                    let _ = std::fs::rename(
                                        &old_bak,
                                        std::path::PathBuf::from(new_bak),
                                    );
                                }
                            }
                        }
                    }
                }
                tracing::info!(
                    item_id = %item.id,
                    new_name = %new_name,
                    version = item.version,
                    "rename"
                );
                Ok(EvOutcome::Renamed(item))
            }
            EventKind::Ack => {
                tracing::debug!("M2 忽略 Ack（上行确认）");
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

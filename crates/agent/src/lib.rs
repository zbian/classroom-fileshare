//! 学员端：发现、加入、接收与重连对齐（PRD 6.x、7.5、FR-S-01..08 的 M1 子集）。
//!
//! M1 范围：手动地址连接（RK-04 备用路径）、join（携带 PIN）、心跳 5s、
//! 接收教师广播（文本正文与文件都经文件帧传输，先落盘再通知——PRD 7.2）、
//! 删除事件落地为墓碑并清理实体、断线自动重连并经 Manifest + 重推对齐。
//! 学员回传（发送）是 M2（P1）内容。
//!
//! beacon 自动发现连接的遗留缺口：`spawn_discovery` 的 Beacon 不含来源 IP
//! （接口未传出 recv_from 的 peer 地址），M1 仅透传 [`AgentEvent::Discovered`]
//! 供 GUI 展示，连接一律走 `manual_addr`；M2/T-10 补自动连接。

use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use classroom_net::{
    encode_frame, read_frame, spawn_discovery, Beacon, FileHeader, Frame, Hello, HelloReply,
};
use classroom_protocol::{Event, EventKind, MergeDecision, Role, ShareItem, PROTOCOL_VERSION};
use classroom_store::{sha256_file, Store};

/// 学员端配置。
pub struct AgentConfig {
    /// 本机昵称（教师端分组主键，重复会被拒）。
    pub nickname: String,
    /// 房间 PIN（接口修订 #5②：计划遗漏，join 需携带）。
    pub pin: Option<String>,
    /// 手动教师机地址（RK-04 备用连接方式）；None 时仅监听发现通知。
    pub manual_addr: Option<SocketAddr>,
    /// 数据根目录。
    pub data_root: PathBuf,
}

/// 学员端通知事件。
#[derive(Debug)]
pub enum AgentEvent {
    /// 发现的教师机信标（M1 仅通知，不自动连接）。
    Discovered(Vec<Beacon>),
    /// 已加入房间（manual 连接时为 `tcp://` 地址描述）。
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
pub async fn run_agent(cfg: AgentConfig, notify: mpsc::Sender<AgentEvent>) -> anyhow::Result<()> {
    let mut store = Store::open(&cfg.data_root)?;
    let Some(addr) = cfg.manual_addr else {
        // M1：无手动地址——仅监听发现并透传（自动连接 M2/T-10，见模块文档）
        let (beacon_tx, mut beacon_rx) = mpsc::channel(16);
        spawn_discovery(beacon_tx);
        let mut seen: Vec<Beacon> = Vec::new();
        while let Some(b) = beacon_rx.recv().await {
            if !seen
                .iter()
                .any(|x| x.room == b.room && x.tcp_port == b.tcp_port && x.host_name == b.host_name)
            {
                seen.push(b);
            }
            let _ = notify.send(AgentEvent::Discovered(seen.clone())).await;
        }
        return Ok(());
    };

    let mut backoff = Duration::from_secs(1);
    loop {
        agent_session(&cfg, &mut store, addr, &notify).await?;
        let _ = notify.send(AgentEvent::Disconnected).await;
        tracing::info!(retry_in = ?backoff, "与教师机断开，稍后自动重连");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

async fn agent_session(
    cfg: &AgentConfig,
    store: &mut Store,
    addr: SocketAddr,
    notify: &mpsc::Sender<AgentEvent>,
) -> anyhow::Result<()> {
    let sock = match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(addr)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "连接失败，稍后重试");
            return Ok(());
        }
        Err(_) => {
            tracing::debug!("连接超时，稍后重试");
            return Ok(());
        }
    };
    sock.set_nodelay(true)?;
    let (mut reader, mut writer) = sock.into_split();

    // join：错 PIN / 重名 / 版本不匹配是致命错误，直接上抛
    let hello = Hello::Join {
        protocol_version: PROTOCOL_VERSION,
        nickname: cfg.nickname.clone(),
        role: Role::Student,
        pin: cfg.pin.clone(),
    };
    writer
        .write_all(&encode_frame(&Frame::Hello(hello)))
        .await?;
    match read_frame(&mut reader).await? {
        Frame::HelloReply(HelloReply::Ok { .. }) => {}
        Frame::HelloReply(HelloReply::Err { code }) => {
            return Err(anyhow!("加入被教师机拒绝: {code:?}"));
        }
        other => return Err(anyhow!("期望 HelloReply，收到 {other:?}")),
    }
    tracing::info!(%addr, "已加入");
    let _ = notify
        .send(AgentEvent::Joined(format!("tcp://{addr}")))
        .await;

    // 重连对齐第一步：请求版本清单（host 回 Manifest 并重推未删条目）
    writer.write_all(&encode_frame(&Frame::ManifestReq)).await?;

    let mut incoming = Incoming::new(&cfg.data_root);
    let mut ping = tokio::time::interval(Duration::from_secs(5));
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    ping.tick().await; // interval 首个 tick 立即触发，吃掉

    loop {
        tokio::select! {
            _ = ping.tick() => {
                if writer.write_all(&encode_frame(&Frame::Ping)).await.is_err() {
                    break;
                }
            }
            frame = read_frame(&mut reader) => {
                let frame = match frame {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::debug!(error = %e, "读帧结束，会话关闭");
                        break;
                    }
                };
                match frame {
                    Frame::Event(ev) => match incoming.on_event(&*store, ev)? {
                        EvOutcome::Removed(id) => {
                            let _ = notify.send(AgentEvent::Removed(id)).await;
                        }
                        EvOutcome::Ignored => {}
                    },
                    Frame::FileHeader(h) => incoming.begin_file(h)?,
                    Frame::FileChunk(c) => incoming.append_chunk(&c)?,
                    Frame::FileEnd(_) => {
                        if let Some(item) = incoming.finish_file(&*store)? {
                            let _ = notify.send(AgentEvent::Received(item)).await;
                        }
                    }
                    Frame::Manifest(entries) => {
                        let removed = align_manifest(&*store, entries)?;
                        for id in removed {
                            let _ = notify.send(AgentEvent::Removed(id)).await;
                        }
                        let _ = notify.send(AgentEvent::Synced).await;
                    }
                    Frame::Pong => {}
                    // E3（issue #37）：必须应答服务端探测 Ping——host 重名
                    // 存活探测依赖 Pong，忽略会导致活跃连接被误驱逐
                    Frame::Ping => {
                        if writer.write_all(&encode_frame(&Frame::Pong)).await.is_err() {
                            tracing::debug!("Pong 写入失败，会话即将结束");
                        }
                    }
                    other => tracing::debug!(frame = ?other, "agent 忽略该帧"),
                }
            }
        }
    }
    incoming.abandon();
    Ok(())
}

/// 清单对齐：远端已删而本地未删的条目，落地墓碑并清实体（PRD 7.4 补删）。
fn align_manifest(store: &Store, entries: Vec<(String, u64, bool)>) -> anyhow::Result<Vec<String>> {
    let mut removed = Vec::new();
    for (id, _version, deleted) in entries {
        if !deleted {
            continue;
        }
        let Some(local) = store.get(&id)? else {
            continue;
        };
        if local.deleted {
            continue;
        }
        let gone = store.soft_delete(&id, unix_now())?;
        if let Ok(body) = store.file_body(&gone) {
            if body.exists() {
                let _ = std::fs::remove_file(&body);
            }
        }
        removed.push(id);
    }
    Ok(removed)
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
        match store.apply_remote(&ev) {
            Ok(MergeDecision::Overwrite(applied)) => Ok(Some(*applied)),
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

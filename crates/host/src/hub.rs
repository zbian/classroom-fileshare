//! 会话枢纽：在线连接表、store 集成、事件与文件分块广播。
//!
//! 广播路径：教师 put → `Event(Upsert)` 先行，文件紧随 `FileHeader →
//! 1MB FileChunk × N → FileEnd`（PRD 7.1 统一发送链路）。
//! 学员→教师事件仅入教师 store（audience=Teacher，不转发，PRD 3.3）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use tokio::sync::mpsc;
use uuid::Uuid;

use classroom_net::{
    FileAck, FileChunk, FileEnd, FileHeader, Frame, Hello, HelloErr, HelloReply, SessionHandler,
};
use classroom_protocol::{
    is_reserved_device_name, Audience, Event, EventKind, Role, ShareItem, CHUNK_SIZE,
};
use classroom_store::{sha256_file, ListFilter, PutOutcome, Store};

use crate::room::Admitter;
use crate::{HostCommand, HostEvent, HostStatus, HostWarningKind, StudentInfo};

/// 教师端固定的 owner_id。
/// **保留昵称**（M2 S3，issue #89 / 设计文档 §3.4）：这是教师作用域的保留
/// 值，`on_hello` 在 join 层强制拒绝该昵称（返回 `HelloErr::InvalidNickname`）
/// ——否则学员把昵称设为 `teacher` 后伪造 `owner_id = "teacher"` 恰好能通过
/// 「owner 与会话昵称一致」校验，从而顶替教师作用域（M1 的 A2 过滤只拦
/// owner≠teacher，此变体可穿透）。
pub const TEACHER_OWNER: &str = "teacher";

/// 墓碑保留期（M2 #17，PRD 7.4 删除同步状态机）。
///
/// 语义：教师删除后墓碑在保留期内随 manifest 下发给每个上线端；离线端在
/// 期限内上线即可补删。超期由 [`Hub::delete_item`] 惰性清理（视为「端长期
/// 离线、放弃补删」，验收口径见 issue #17）。
///
/// 取值 7 天：PRD 未给数值；覆盖「周末 + 请假」级离线，同时避免墓碑无限
/// 膨胀（M1 冻结报告 Deferral 曾记「墓碑保留策略缺失」）。
pub const TOMBSTONE_TTL_SECS: i64 = 7 * 24 * 3600;

pub(crate) struct Hub {
    /// rusqlite Connection 是 Send 非 Sync，包 Mutex 以满足 Arc<dyn SessionHandler> 的 Sync 要求；
    /// 锁内全部为快速同步调用，无 await。
    store: Mutex<Store>,
    room_code: String,
    admitter: Admitter,
    notify: mpsc::Sender<HostEvent>,
    conns: Mutex<HashMap<String, Conn>>,
    /// 学员回传的「已登记元数据、等待内容帧」状态（M2 #94）：
    /// session_id → 学员上行 Event。文本/文件内容经随后的 FileHeader→Chunk→
    /// End 分块流承载（与教师广播的 Event+文件帧模型对称）；内容到位后才
    /// 入库，避免「元数据先落库、内容永远不来」的半成品条目。
    /// session_id → (item_id → 上行 Event)：允许同一会话**并发多个**待完成
    /// 上传（M2 #18 补发可能连续投递多条；按 item_id 精确配对内容帧）。
    uploads: Mutex<HashMap<String, HashMap<String, Event>>>,
    /// M2 #114（S5）：按 item_id 收集删除确认进度（内存态；幂等去重）。
    deletes: Mutex<HashMap<String, DeleteProgress>>,
    /// 占用重试初始退避（秒，M2 #103；由 HostConfig 注入，测试可缩短）。
    retry_initial_backoff_secs: i64,
    port: AtomicU16,
}

/// 单个条目的删除确认进度（M2 #114 S5，内存态）。
#[derive(Default)]
struct DeleteProgress {
    /// 已 ack 的会话集合（Ack 幂等：同会话重复 ack 不重复计数）。
    sessions: std::collections::HashSet<String>,
    /// 目标端数（删除时刻在线端数）。
    target: u32,
    /// 是否已收敛（全部 ack → `Deleted` 终态）。
    converged: bool,
}
struct Conn {
    nickname: String,
    tx: mpsc::Sender<Frame>,
    /// 最近一次 Pong 的秒级时间戳（0=从未应答；PR #34 B 存活探测）。
    pong_at: Arc<AtomicI64>,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn latest_item(out: PutOutcome) -> ShareItem {
    match out {
        PutOutcome::Created(item) => item,
        PutOutcome::Overwritten { latest, .. } => latest,
        // M2 #103：占用 → RetryLocked 条目（内容尚未安装，不广播）
        PutOutcome::RetryQueued { item, .. } => item,
    }
}

/// 确定性 event_id：`H(item_id|version|kind)` 前 32 hex。
///
/// 接口修订 #5③（lead 建议采纳）：重连重推复用同一 event_id，接收端
/// `seen_event` 直接幂等去重，`event_log` 不随重连次数膨胀。
fn deterministic_event_id(item_id: &str, version: u64, kind: &EventKind) -> String {
    use sha2::{Digest, Sha256};
    let kind_name = match kind {
        EventKind::Upsert => "upsert",
        EventKind::Delete => "delete",
        EventKind::Rename => "rename",
        EventKind::Ack => "ack",
    };
    let mut h = Sha256::new();
    h.update(item_id.as_bytes());
    h.update(b"|");
    h.update(version.to_string().as_bytes());
    h.update(b"|");
    h.update(kind_name.as_bytes());
    classroom_store::files::to_hex(&h.finalize())[..32].to_string()
}

impl Hub {
    pub(crate) fn new(
        store: Store,
        room_code: String,
        admitter: Admitter,
        notify: mpsc::Sender<HostEvent>,
        retry_initial_backoff_secs: i64,
    ) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            room_code,
            admitter,
            notify,
            conns: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
            deletes: Mutex::new(HashMap::new()),
            retry_initial_backoff_secs,
            port: AtomicU16::new(0),
        })
    }

    pub(crate) fn set_port(&self, port: u16) {
        self.port.store(port, Ordering::SeqCst);
    }

    pub(crate) fn status(&self) -> HostStatus {
        HostStatus {
            room_code: self.room_code.clone(),
            tcp_port: self.port.load(Ordering::SeqCst),
            online: self
                .conns
                .lock()
                .unwrap()
                .iter()
                .map(|(sid, c)| StudentInfo {
                    nickname: c.nickname.clone(),
                    session_id: sid.clone(),
                })
                .collect(),
        }
    }

    /// 在线连接的发送通道快照（锁内不 await，发送在锁外逐个进行）。
    fn targets(&self) -> Vec<mpsc::Sender<Frame>> {
        self.conns
            .lock()
            .unwrap()
            .values()
            .map(|c| c.tx.clone())
            .collect()
    }

    fn make_event(kind: EventKind, item: ShareItem, now: i64) -> Event {
        Event {
            event_id: deterministic_event_id(&item.id, item.version, &kind),
            kind,
            item,
            new_name: None,
            acked_by: None,
            ts: now,
        }
    }

    async fn push_file_to(&self, item: &ShareItem, tx: &mut mpsc::Sender<Frame>) {
        // M2 加固项（E10 评审裁定，记 #16）：此处为全量读内存
        // （std::fs::read）。E10 已在 store 与 send_one_file 前置 100MB
        // 上限，内存峰值受控 ≤100MB（M1 可接受）；流式推送
        // （File::open + CHUNK_SIZE 分块读）排入 M2，避免冻结前改动
        // 数据面引入回归风险。
        let Ok(body_path) = self.store.lock().unwrap().file_body(item) else {
            tracing::warn!(item_id = %item.id, "文件实体缺失，跳过推送");
            return;
        };
        let Ok(bytes) = std::fs::read(&body_path) else {
            tracing::warn!(item_id = %item.id, "文件读取失败，跳过推送");
            return;
        };
        // 文本条目 sha256=None（PRD 3.5），广播时对正文现算整体哈希
        let sha = match item.sha256.as_deref() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => {
                use sha2::{Digest, Sha256};
                classroom_store::files::to_hex(&Sha256::digest(&bytes))
            }
        };
        let _ = tx
            .send(Frame::FileHeader(FileHeader {
                item_id: item.id.clone(),
                file_name: item.title.clone(),
                size: bytes.len() as u64,
                sha256: sha.clone(),
            }))
            .await;
        for (seq, part) in bytes.chunks(CHUNK_SIZE).enumerate() {
            let _ = tx
                .send(Frame::FileChunk(FileChunk {
                    item_id: item.id.clone(),
                    seq: seq as u64,
                    bytes: part.to_vec(),
                }))
                .await;
        }
        let _ = tx
            .send(Frame::FileEnd(FileEnd {
                item_id: item.id.clone(),
            }))
            .await;
    }

    async fn broadcast_event(&self, ev: &Event) {
        for tx in self.targets() {
            let _ = tx.send(Frame::Event(ev.clone())).await;
        }
    }

    async fn broadcast_file(&self, item: &ShareItem) {
        for mut tx in self.targets() {
            self.push_file_to(item, &mut tx).await;
        }
    }

    pub(crate) async fn handle(&self, cmd: &HostCommand) {
        match cmd {
            HostCommand::SendText { title, body } => self.send_text(title, body).await,
            HostCommand::SendFiles { paths } => self.send_files(paths).await,
            HostCommand::Delete { id } => self.delete_item(id).await,
            HostCommand::Rename { id, new_name } => self.rename_item(id, new_name).await,
            HostCommand::Shutdown => {}
        }
    }

    /// PR #34 B：探测旧连接——存活 → Some(())（调用方拒绝新 join）；
    /// 无响应 → 驱逐旧表项并返回 None（新 join 可用该昵称）。
    async fn probe_or_evict(&self, old_sid: &str, nickname: &str) -> Option<()> {
        let probe_start = unix_now();
        let (ping_tx, pong_at) = {
            let conns = self.conns.lock().unwrap();
            let conn = conns.get(old_sid)?;
            (conn.tx.clone(), conn.pong_at.clone())
        };
        let _ = ping_tx.send(Frame::Ping).await;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if pong_at.load(Ordering::SeqCst) >= probe_start {
                tracing::info!(nickname = %nickname, "旧连接存活，拒绝重名加入（EX-02）");
                return Some(());
            }
            // 探测期间旧连接可能自然断开（on_disconnect 已移除表项）
            self.conns.lock().unwrap().get(old_sid)?;
        }
        // 僵死：驱逐旧表项（广播不再发它；writer 因 tx 释放而关闭，旧客户端
        // 读到 EOF 自行退出）。旧会话任务的读循环可能仍挂——M1 接受：
        // 其后续事件被 owner 过滤（PR #34 A2）忽略
        self.conns.lock().unwrap().remove(old_sid);
        tracing::info!(nickname = %nickname, "旧连接无响应，已驱逐（昵称可重新加入）");
        None
    }

    async fn send_text(&self, title: &str, body: &str) {
        let now = unix_now();
        let outcome = self.store.lock().unwrap().put_text(
            TEACHER_OWNER,
            Role::Teacher,
            Audience::All,
            title,
            body,
            now,
        );
        match outcome {
            Ok(out) => {
                let item = latest_item(out);
                let ev = Self::make_event(EventKind::Upsert, item.clone(), now);
                let _ = self.store.lock().unwrap().record_event(&ev);
                self.broadcast_event(&ev).await;
                // 文本正文同样经文件帧推送（修订 #5④：ShareItem 不含正文字段）
                self.broadcast_file(&item).await;
                // E4（#14 TC-02 计时锚点）：字段名稳定供 qa 解析
                tracing::info!(
                    item_id = %item.id,
                    file = %item.title,
                    size = item.size,
                    audience = "all",
                    "broadcast"
                );
                let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
            }
            Err(e) => tracing::warn!(error = %e, "教师文本入库失败"),
        }
    }

    async fn send_files(&self, paths: &[std::path::PathBuf]) {
        let now = unix_now();
        for path in paths {
            if let Err(e) = self.send_one_file(path, now).await {
                tracing::warn!(path = %path.display(), error = %e, "教师文件广播失败");
            }
        }
    }

    async fn send_one_file(&self, path: &Path, now: i64) -> anyhow::Result<()> {
        let raw_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("路径无有效文件名"))?;
        // E10（P0，PRD 5.3/9.4/EX-04）：发送前 metadata 快速失败——
        // 在 sha256_file（全量读 IO）与 store.copy 之前拦截，避免白耗
        let size = std::fs::metadata(path)?.len();
        if size > classroom_protocol::MAX_FILE_SIZE {
            anyhow::bail!("单文件不能超过 100 MB（{} 字节）", size);
        }
        // 教师本地发送：先自算哈希，与远端传输走同一条校验路径（EX-07）
        let sha = sha256_file(path)?;
        let out = self.store.lock().unwrap().put_file_from_tmp(
            TEACHER_OWNER,
            Role::Teacher,
            Audience::All,
            &raw_name,
            path,
            &sha,
            now,
        )?;
        // M2 #103：目标被占用 → 条目 RetryLocked 入退避重试队列。内容此刻
        // **尚未安装**，因此不广播 Event/文件（学员端无内容可拉）；成功重试
        // 后再广播。教师端仅收到列表刷新（GUI 显示「被占用重试 第 n/5 次」）。
        if let PutOutcome::RetryQueued { item, .. } = out {
            tracing::info!(
                item_id = %item.id,
                file = %item.title,
                "broadcast-retry-queued"
            );
            let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
            self.drain_store_warnings().await;
            return Ok(());
        }
        let item = latest_item(out);
        let ev = Self::make_event(EventKind::Upsert, item.clone(), now);
        let _ = self.store.lock().unwrap().record_event(&ev);
        self.broadcast_event(&ev).await;
        self.broadcast_file(&item).await;
        // E4（#14 TC-02 计时锚点）
        tracing::info!(
            item_id = %item.id,
            file = %item.title,
            size = item.size,
            audience = "all",
            "broadcast"
        );
        let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
        Ok(())
    }

    /// 占用重试调度（M2 #103，由 run_host 的 200ms tick 调用）：
    /// 到期望项逐条尝试；成功 → 内容已安装，广播给在线端；耗尽/中止 → 告警。
    pub(crate) async fn retry_tick(&self) {
        let now = unix_now();
        let due = self.store.lock().unwrap().retry_due(now);
        for entry in due {
            let outcome = self.store.lock().unwrap().retry_attempt(
                &entry.item_id,
                now,
                self.retry_initial_backoff_secs,
            );
            match outcome {
                Ok(classroom_store::RetryOutcome::Succeeded { item_id }) => {
                    // 内容现已安装：此时才广播（与教师正常发送同链路），
                    // 在线端拿到完整条目与文件。
                    // 注意：先取出 owned item（store 锁不跨 await，future 才 Send）
                    let item = self.store.lock().unwrap().get(&item_id).ok().flatten();
                    if let Some(item) = item {
                        let ev = Self::make_event(EventKind::Upsert, item.clone(), unix_now());
                        let _ = self.store.lock().unwrap().record_event(&ev);
                        self.broadcast_event(&ev).await;
                        self.broadcast_file(&item).await;
                        // E4（#14 TC-02 计时锚点）
                        tracing::info!(
                            item_id = %item.id,
                            file = %item.title,
                            size = item.size,
                            audience = "all",
                            "broadcast"
                        );
                        let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
                    }
                }
                Ok(classroom_store::RetryOutcome::Exhausted { item_id }) => {
                    tracing::warn!(item_id = %item_id, "占用重试耗尽（5 次）");
                    let _ = self
                        .notify
                        .send(HostEvent::Warning {
                            kind: HostWarningKind::RetryExhausted,
                            item_id,
                            detail: "目标文件被占用，重试 5 次仍未成功，条目已置失败".into(),
                        })
                        .await;
                }
                Ok(classroom_store::RetryOutcome::Aborted { item_id, reason }) => {
                    let _ = self
                        .notify
                        .send(HostEvent::Warning {
                            kind: HostWarningKind::RetryExhausted,
                            item_id,
                            detail: format!("重试中止: {reason}"),
                        })
                        .await;
                }
                Ok(classroom_store::RetryOutcome::StillBusy {
                    item_id,
                    attempts,
                    next_at,
                }) => {
                    tracing::info!(item_id = %item_id, attempts, next_at, "占用重试退避");
                }
                Err(e) => tracing::warn!(error = %e, "重试执行失败"),
            }
        }
        self.drain_store_warnings().await;
    }

    /// 存储层告警（备份失败等）→ `HostEvent::Warning`（M2 #103，PRD 5.7）。
    async fn drain_store_warnings(&self) {
        let warnings = self.store.lock().unwrap().take_warnings();
        for w in warnings {
            match w {
                classroom_store::StoreWarning::BackupFailed {
                    item_id,
                    title,
                    detail,
                } => {
                    tracing::warn!(item_id = %item_id, title = %title, "备份失败（不回滚新版本）");
                    let _ = self
                        .notify
                        .send(HostEvent::Warning {
                            kind: HostWarningKind::BackupFailed,
                            item_id,
                            detail,
                        })
                        .await;
                }
            }
        }
    }

    /// 重命名条目并广播 rename 事件（M2 #19，PRD 5.4）。
    ///
    /// - 同名冲突：store 返回 `NameTaken`（不静默覆盖，M2 语义，PR 说明）
    /// - 学员端收到 rename 事件后按新名称/新 `rel_path` 应用（2 秒内同步）
    async fn rename_item(&self, id: &str, new_name: &str) {
        let now = unix_now();
        let renamed = self.store.lock().unwrap().rename_item(id, new_name, now);
        match renamed {
            Ok(item) => {
                let mut ev = Self::make_event(EventKind::Rename, item.clone(), now);
                // PRD 3.6 / 5.4：rename 事件必须携带 new_name（接收端据此应用新名）
                ev.new_name = Some(item.title.clone());
                let _ = self.store.lock().unwrap().record_event(&ev);
                self.broadcast_event(&ev).await;
                // E4/回归锚点：rename item_id / version（PRD 5.4 2 秒内同步）
                tracing::info!(
                    item_id = %item.id,
                    version = item.version,
                    "rename"
                );
                let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
            }
            Err(e) => {
                tracing::warn!(item_id = %id, error = %e, "重命名失败");
            }
        }
    }

    async fn delete_item(&self, id: &str) {
        let now = unix_now();
        let deleted = self.store.lock().unwrap().soft_delete(id, now);
        match deleted {
            Ok(item) => {
                let ev = Self::make_event(EventKind::Delete, item.clone(), now);
                let _ = self.store.lock().unwrap().record_event(&ev);
                // 教师端删除**立即完成**（不等任何离线端，PRD 7.4）：软删 +
                // 墓碑 + 广播；离线端上线后经 manifest 比对补删（agent 侧）。
                self.broadcast_event(&ev).await;
                // TC-08 观察项 O-A（PRD 7.4 字面「删除实体、写墓碑、广播」）：
                // 广播后**立即删除实体**（final + .bak）；墓碑行保留至 TTL，
                // 供离线端 manifest 比对补删——此前实体滞留到 TTL purge。
                if let Err(e) = self.store.lock().unwrap().remove_item_bodies(&item) {
                    tracing::warn!(item_id = %item.id, error = %e, "删除实体失败");
                }
                // TC-08 观察项 O-B：删除广播日志锚点（回归/压测测量）。
                // PRD 9.6 隐私：条目 owner 可能是**学员昵称**（学员提交也可被
                // 教师删除）→ 只记角色不记昵称；item_id 保留（条目标识非隐私）。
                tracing::info!(
                    item_id = %item.id,
                    owner_role = ?item.owner_role,
                    "delete"
                );
                // M2 #114（S5）：初始化删除进度——target=删除时刻在线端数；
                // 端侧 ack（在线即时 / 离线补删）在此聚合，全部 ack 后收敛 Deleted。
                let target = self.conns.lock().unwrap().len() as u32;
                self.deletes.lock().unwrap().insert(
                    item.id.clone(),
                    DeleteProgress {
                        sessions: std::collections::HashSet::new(),
                        target,
                        converged: false,
                    },
                );
                // M2 #17：惰性清理过期墓碑（不引入常驻定时器）——墓碑在
                // 保留期内随 manifest 下发，超期视为「端长期离线，放弃补删」
                if let Err(e) = self
                    .store
                    .lock()
                    .unwrap()
                    .purge_tombstones(now - TOMBSTONE_TTL_SECS)
                {
                    tracing::warn!(error = %e, "过期墓碑清理失败");
                }
            }
            Err(e) => {
                // 重复删除：已删条目不再命中 WHERE deleted = 0 → NotFound。
                // 幂等语义：不 panic、不重复广播（验收：重复 delete 不报错）。
                tracing::info!(item_id = %id, error = %e, "删除未生效（条目不存在或已删除）");
            }
        }
    }
}

#[async_trait]
impl SessionHandler for Hub {
    async fn on_hello(&self, hello: Hello, tx: &mut mpsc::Sender<Frame>) -> HelloReply {
        if let Err(code) = self.admitter.check(&hello) {
            return HelloReply::Err { code };
        }
        let Hello::Join { nickname, .. } = &hello;
        // PR #30 检查点：保留设备名昵称不可用（PRD 9.4；昵称同时用作
        // root/repo/<owner>/ 目录名，Win10 上保留名目录可能创建失败）。
        if is_reserved_device_name(nickname) {
            return HelloReply::Err {
                code: HelloErr::InvalidNickname,
            };
        }
        // M2 S3（设计 §3.4）：教师作用域保留昵称不得被学员占用——否则学员
        // 可伪造 owner_id="teacher" 通过「身份一致」校验，顶替教师作用域。
        if nickname == TEACHER_OWNER {
            tracing::warn!(%nickname, "拒绝教师保留昵称加入（教师作用域保留值）");
            return HelloReply::Err {
                code: HelloErr::InvalidNickname,
            };
        }
        // PR #34 B 项（重名存活探测）：同名已在线时先探测旧连接——
        // 课堂合盖休眠/断网（无 RST）极常见，无条件拒绝会把昵称锁死到
        // 教师重启；无脑替换又破坏 PRD EX-02/TC-01 的重名拒绝语义。
        // 折中：向旧连接发 Ping，至多 3s 内收到 Pong 视为存活（拒绝新
        // join）；超时判定僵死，驱逐旧表项后接受新 join。
        let dup_sid = {
            let conns = self.conns.lock().unwrap();
            conns
                .iter()
                .find(|(_, c)| c.nickname == *nickname)
                .map(|(sid, _)| sid.clone())
        };
        if let Some(old_sid) = dup_sid {
            if self.probe_or_evict(&old_sid, nickname).await.is_some() {
                return HelloReply::Err {
                    code: HelloErr::DupNickname,
                };
            }
        }
        let session_id = Uuid::new_v4().to_string();
        self.conns.lock().unwrap().insert(
            session_id.clone(),
            Conn {
                nickname: nickname.clone(),
                tx: tx.clone(),
                pong_at: Arc::new(AtomicI64::new(0)),
            },
        );
        tracing::info!(nickname = %nickname, session_id = %session_id, "学员加入");
        let _ = self
            .notify
            .send(HostEvent::StudentJoined(nickname.clone()))
            .await;
        let _ = self.notify.send(HostEvent::Status(self.status())).await;
        HelloReply::Ok { session_id }
    }

    async fn on_event(&self, session_id: &str, ev: Event, _tx: &mut mpsc::Sender<Frame>) {
        // ===== M2 #17（PRD 3.6 / 7.4）：补删确认 Ack =====
        // 与内容事件不同：Ack 的 `item` 是被删除条目的快照（owner=teacher，
        // 学员并非该条目所有者），因此身份以 `acked_by` 为准——只校验
        // ① 会话已注册 ② acked_by 等于会话昵称。该分支必须先于下面的
        // 「owner_id == 会话昵称」内容校验。
        if ev.kind == EventKind::Ack {
            let expected = {
                let conns = self.conns.lock().unwrap();
                conns.get(session_id).map(|c| c.nickname.clone())
            };
            let Some(expected) = expected else {
                tracing::warn!(%session_id, "未注册会话的 Ack，丢弃");
                return;
            };
            if ev.acked_by.as_deref() != Some(expected.as_str()) {
                tracing::warn!(
                    %session_id,
                    acked_by = ?ev.acked_by,
                    expected = %expected,
                    "Ack 身份不符（acked_by 与会话昵称不一致），丢弃"
                );
                return;
            }
            // TC-08 观察项 O-B：补删 Ack 日志锚点（字段名稳定供 qa/M3 压测解析）。
            // PRD 9.6 隐私：**不打印学员昵称**——用会话 id 前 8 位做可关联标识，
            // item_id 保留（条目标识非隐私）。
            let session_tag = &session_id[..session_id.len().min(8)];
            tracing::info!(
                item_id = %ev.item.id,
                from_session = %session_tag,
                "delete-ack"
            );
            // M2 #114（S5）：**幂等去重**（顺带处理 #17 已知小项：Ack 未
            // record_event → 重复 Ack 会重复触发 DeleteAcked/聚合计数）
            let ack_item_id = ev.item.id.clone();
            let first_seen = {
                let store = self.store.lock().unwrap();
                !store.seen_event(&ev.event_id).unwrap_or(false)
            };
            if !first_seen {
                tracing::debug!(item_id = %ack_item_id, "重复 Ack（已处理），忽略");
                return;
            }
            let _ = self.store.lock().unwrap().record_event(&ev);
            // 聚合：按 item_id 收集 acked 会话（同会话重复 ack 不重复计数）
            let (done, total, converged_now) = {
                let mut deletes = self.deletes.lock().unwrap();
                match deletes.get_mut(&ack_item_id) {
                    Some(p) => {
                        p.sessions.insert(session_id.to_string());
                        let done = p.sessions.len() as u32;
                        let converged_now = !p.converged && p.target > 0 && done >= p.target;
                        if converged_now {
                            p.converged = true;
                        }
                        (done, p.target, converged_now)
                    }
                    // 无进度记录（如删除前已离线、上线补删的端）：只记日志
                    None => (0, 0, false),
                }
            };
            if total > 0 {
                // 先发 DeleteAcked（兼容既有观测），再发进度
                let _ = self
                    .notify
                    .send(HostEvent::DeleteAcked {
                        item_id: ack_item_id.clone(),
                        nickname: expected.clone(),
                    })
                    .await;
                let _ = self
                    .notify
                    .send(HostEvent::DeleteProgress {
                        item_id: ack_item_id.clone(),
                        done,
                        total,
                    })
                    .await;
                if converged_now {
                    // 全部目标端确认 → 终态 Deleted（PRD 7.4「已删除」收敛）
                    let _ = self
                        .store
                        .lock()
                        .unwrap()
                        .set_sync_state(&ack_item_id, classroom_protocol::SyncState::Deleted);
                    tracing::info!(item_id = %ack_item_id, done, total, "删除进度收敛（全部 ack）");
                    // 评审建议：收敛后从内存移除（防 HashMap 单调增长；
                    // 墓碑 TTL 语义不变，后续离线端补删 ack 仅记日志）
                    self.deletes.lock().unwrap().remove(&ack_item_id);
                }
            } else {
                let _ = self
                    .notify
                    .send(HostEvent::DeleteAcked {
                        item_id: ack_item_id.clone(),
                        nickname: expected.clone(),
                    })
                    .await;
            }
            return;
        }
        // ===== M2 S3 会话身份绑定（接口修订 #9①，设计文档 §3.3）=====
        // 三重校验，任一失败即丢弃（不落库、不广播、不 ack）：
        // ① 会话存在（net 层未 join 守卫之上再兜一层：conns 是唯一身份源）
        let expected_owner = {
            let conns = self.conns.lock().unwrap();
            conns.get(session_id).map(|c| c.nickname.clone())
        };
        let Some(expected_owner) = expected_owner else {
            tracing::warn!(
                %session_id,
                event_id = %ev.event_id,
                "未注册会话的事件，丢弃"
            );
            return;
        };
        // ② 身份一致：owner_id 必须等于该会话 join 时认证的昵称（冒名防线）
        if ev.item.owner_id != expected_owner {
            tracing::warn!(
                %session_id,
                owner = %ev.item.owner_id,
                expected = %expected_owner,
                event_id = %ev.event_id,
                "冒名事件被拒（owner_id 与会话昵称不一致）"
            );
            return;
        }
        // ③ 方向合法：学员事件只能定向教师（伪造 audience=All 会被拒绝，
        //    否则内容会扩散给其他学员，PRD 3.3 学生互不可见）
        if ev.item.audience != Audience::Teacher {
            tracing::warn!(
                %session_id,
                audience = ?ev.item.audience,
                event_id = %ev.event_id,
                "越权广播被拒（学员事件必须定向教师）"
            );
            return;
        }
        // ④ owner_role 必须为 Student（学员回传的会话身份固定为学生）
        if ev.item.owner_role != Role::Student {
            tracing::warn!(
                %session_id,
                role = ?ev.item.owner_role,
                event_id = %ev.event_id,
                "角色不符的事件被拒（学员会话只能提交 Student 角色内容）"
            );
            return;
        }
        // ===== 通过身份校验：登记为待完成上传（M2 #94）=====
        // 内容经随后的 FileHeader→Chunk→End 分块流承载（与教师广播的
        // Event + 文件帧模型对称）；内容到位、哈希校验通过后才入库——
        // 避免「元数据先落库、内容永远不来」的半成品条目。
        let mut uploads = self.uploads.lock().unwrap();
        if uploads
            .entry(session_id.to_string())
            .or_default()
            .insert(ev.item.id.clone(), ev.clone())
            .is_some()
        {
            tracing::warn!(%session_id, "上一个未完成的上传被新事件替换");
        }
        tracing::info!(
            %session_id,
            owner = %expected_owner,
            item_id = %ev.item.id,
            kind = ?ev.item.kind,
            "学员提交已登记，等待内容帧"
        );
    }

    async fn on_file_chunk_stream(
        &self,
        session_id: &str,
        header: FileHeader,
        mut chunks: mpsc::Receiver<FileChunk>,
        _expected_sha256: String,
    ) -> FileAck {
        // M2 S3：仅已认证会话可发起文件流；M2 #94：学员上传的内容帧
        // （文本正文或文件字节）在此落盘入库。隔离：不广播、不进 manifest。
        let nickname = {
            let conns = self.conns.lock().unwrap();
            conns.get(session_id).map(|c| c.nickname.clone())
        };
        let Some(nickname) = nickname else {
            drain_chunks_bounded(&mut chunks).await;
            tracing::warn!(%session_id, item_id = %header.item_id, "未注册会话的文件流，拒绝");
            return FileAck {
                item_id: header.item_id,
                sha256_ok: false,
            };
        };
        // 取出该会话登记的元数据事件（必须与内容帧 item_id 对应）
        let pending = self
            .uploads
            .lock()
            .unwrap()
            .get_mut(session_id)
            .and_then(|m| m.remove(&header.item_id));
        let Some(ev) = pending.filter(|e| e.item.id == header.item_id) else {
            drain_chunks_bounded(&mut chunks).await;
            tracing::warn!(
                %session_id,
                item_id = %header.item_id,
                "内容帧缺少对应的已登记事件（或 item_id 不符），拒绝"
            );
            return FileAck {
                item_id: header.item_id,
                sha256_ok: false,
            };
        };

        // 收到本机临时文件（跨卷安全：后续 put_* 内部 copy 到仓库）。
        let tmp = std::env::temp_dir().join(format!(
            "classshare-upload-{}.part",
            uuid::Uuid::new_v4().simple()
        ));
        let mut written: u64 = 0;
        {
            let mut out = match std::fs::File::create(&tmp) {
                Ok(f) => f,
                Err(e) => {
                    drain_chunks_bounded(&mut chunks).await;
                    tracing::warn!(error = %e, "上传临时文件创建失败");
                    return FileAck {
                        item_id: header.item_id,
                        sha256_ok: false,
                    };
                }
            };
            use std::io::Write as _;
            // 上限检查在**循环内**逐块进行（评审加固：不信任客户端——
            // 原实现等循环结束才判，恶意客户端可先写满 %TEMP% 再被拒）。
            // 每块先算累计再判，超限块本身不写盘，随即删临时文件 + 排空
            // 剩余分块（避免发送端阻塞）并回失败 ack。
            while let Some(chunk) = chunks.recv().await {
                let next = written + chunk.bytes.len() as u64;
                if next > classroom_protocol::MAX_FILE_SIZE {
                    drop(out);
                    let _ = std::fs::remove_file(&tmp);
                    tracing::warn!(
                        %session_id,
                        bytes = next,
                        "上传超过 100MB 上限（循环内检查，已中止写盘）"
                    );
                    drain_chunks_bounded(&mut chunks).await;
                    return FileAck {
                        item_id: header.item_id,
                        sha256_ok: false,
                    };
                }
                if let Err(e) = out.write_all(&chunk.bytes) {
                    let _ = std::fs::remove_file(&tmp);
                    tracing::warn!(error = %e, "上传写入失败");
                    // 排空余下分块，避免发送端阻塞
                    drain_chunks_bounded(&mut chunks).await;
                    return FileAck {
                        item_id: header.item_id,
                        sha256_ok: false,
                    };
                }
                written = next;
            }
        }

        // 哈希校验（EX-07）；大小上限已在循环内保证（written ≤ MAX_FILE_SIZE）
        let ok = classroom_store::sha256_file(&tmp)
            .map(|h| h.eq_ignore_ascii_case(&header.sha256))
            .unwrap_or(false);
        if !ok {
            tracing::warn!(%session_id, item_id = %header.item_id, "上传整体哈希校验失败（EX-07）");
        }
        if !ok {
            let _ = std::fs::remove_file(&tmp);
            return FileAck {
                item_id: header.item_id,
                sha256_ok: false,
            };
        }

        // 入库（M2 #121）：**按学员上行 Event 的 id/version 落库**——
        // 学员本地「我共享的」与教师端看到的同一条目（id/version 一致）。
        // owner=会话昵称（服务端权威，学员无法伪造）、Student、audience=Teacher。
        let now = unix_now();
        let content = match std::fs::read(&tmp) {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                tracing::warn!(error = %e, "上传内容读取失败");
                return FileAck {
                    item_id: header.item_id,
                    sha256_ok: false,
                };
            }
        };
        let _ = std::fs::remove_file(&tmp);
        let (kind, size, sha) = match ev.item.kind {
            classroom_protocol::ItemKind::Text => {
                // 文本 size = 字符数（PRD 3.5）
                match std::str::from_utf8(&content) {
                    Ok(body) => (
                        classroom_protocol::ItemKind::Text,
                        body.chars().count() as u64,
                        None,
                    ),
                    Err(e) => {
                        tracing::warn!(error = %e, "文本正文 UTF-8 解码失败");
                        return FileAck {
                            item_id: header.item_id,
                            sha256_ok: false,
                        };
                    }
                }
            }
            classroom_protocol::ItemKind::File => (
                classroom_protocol::ItemKind::File,
                content.len() as u64,
                Some(header.sha256.clone()),
            ),
        };
        let put = self.store.lock().unwrap().put_entry_with_id(
            &ev.item.id,
            &nickname,
            Role::Student,
            Audience::Teacher,
            &ev.item.title,
            kind,
            &content,
            size,
            sha.as_deref(),
            ev.item.version,
            classroom_protocol::SyncState::Synced,
            now,
        );
        match put {
            Ok(outcome) => {
                let item = outcome;
                tracing::info!(
                    owner = %nickname,
                    item_id = %item.id,
                    title = %item.title,
                    version = item.version,
                    "学员提交入库"
                );
                let _ = self.notify.send(HostEvent::ReceivedUpsert(item)).await;
                FileAck {
                    item_id: header.item_id,
                    sha256_ok: true,
                }
            }
            Err(e) => {
                tracing::warn!(owner = %nickname, error = %e, "学员提交入库失败");
                FileAck {
                    item_id: header.item_id,
                    sha256_ok: false,
                }
            }
        }
    }

    async fn on_disconnect(&self, session_id: &str) {
        // M2 #94：会话断开时丢弃未完成的上传登记（内容帧已不会再到达）
        if self.uploads.lock().unwrap().remove(session_id).is_some() {
            tracing::debug!(%session_id, "断开时丢弃未完成的学员上传");
        }
        let removed = self.conns.lock().unwrap().remove(session_id);
        if let Some(conn) = removed {
            tracing::info!(nickname = %conn.nickname, "学员离线");
            let _ = self
                .notify
                .send(HostEvent::StudentLeft(conn.nickname))
                .await;
            let _ = self.notify.send(HostEvent::Status(self.status())).await;
        }
    }

    async fn on_pong(&self, session_id: &str) {
        if let Some(conn) = self.conns.lock().unwrap().get(session_id) {
            conn.pong_at.store(unix_now(), Ordering::SeqCst);
        }
    }

    async fn on_manifest_req(&self, session_id: &str, tx: &mut mpsc::Sender<Frame>) {
        // M2 S3：清单请求必须来自已认证会话（net 层未 join 守卫之上再校验
        // conns 身份源；未注册会话不返回任何清单内容）。
        if !self.conns.lock().unwrap().contains_key(session_id) {
            tracing::warn!(%session_id, "未注册会话的清单请求，丢弃");
            return;
        }
        // PR #34 A1：清单仅含教师广播内容（owner=teacher + audience=All），
        // 不泄露学员提交条目
        let filter = ListFilter {
            owner: Some(TEACHER_OWNER.to_string()),
            audience: Some(Audience::All),
            include_deleted: true,
            ..Default::default()
        };
        let items = self.store.lock().unwrap().list(filter).unwrap_or_default();
        // 先回版本清单，再对未删条目整体重推（Event + 文件/正文件帧）：
        // 确定性 event_id 使接收端幂等去重，缺失条目 Overwrite、已有条目忽略
        let entries = items
            .iter()
            .map(|i| (i.id.clone(), i.version, i.deleted))
            .collect();
        let _ = tx.send(Frame::Manifest(entries)).await;
        for item in items.into_iter().filter(|i| !i.deleted) {
            let ev = Self::make_event(EventKind::Upsert, item.clone(), unix_now());
            let _ = tx.send(Frame::Event(ev)).await;
            self.push_file_to(&item, tx).await;
        }
    }
}

/// 有界排空分块流（M2 #94 加固）：失败路径必须继续读走已到达的分块，
/// 但**不能无限等待**——恶意/异常客户端可能不发 `FileEnd`，广播式
/// `while let Some(_) = recv().await` 会让会话任务永久挂起（资源泄漏）。
/// 上限 3s 后放弃排空（此后连接照常由 net 层处理）。
async fn drain_chunks_bounded(chunks: &mut mpsc::Receiver<FileChunk>) {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while let Ok(Some(_)) = tokio::time::timeout_at(deadline, chunks.recv()).await {}
}

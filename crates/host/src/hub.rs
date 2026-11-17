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
use crate::{HostCommand, HostEvent, HostStatus, StudentInfo};

/// 教师端固定的 owner_id。
///
/// **保留昵称**（M2 S3，issue #89 / 设计文档 §3.4）：这是教师作用域的保留
/// 值，`on_hello` 在 join 层强制拒绝该昵称（返回 `HelloErr::InvalidNickname`）
/// ——否则学员把昵称设为 `teacher` 后伪造 `owner_id = "teacher"` 恰好能通过
/// 「owner 与会话昵称一致」校验，从而顶替教师作用域（M1 的 A2 过滤只拦
/// owner≠teacher，此变体可穿透）。
pub const TEACHER_OWNER: &str = "teacher";

pub(crate) struct Hub {
    /// rusqlite Connection 是 Send 非 Sync，包 Mutex 以满足 Arc<dyn SessionHandler> 的 Sync 要求；
    /// 锁内全部为快速同步调用，无 await。
    store: Mutex<Store>,
    room_code: String,
    admitter: Admitter,
    notify: mpsc::Sender<HostEvent>,
    conns: Mutex<HashMap<String, Conn>>,
    port: AtomicU16,
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
    ) -> Arc<Self> {
        Arc::new(Self {
            store: Mutex::new(store),
            room_code,
            admitter,
            notify,
            conns: Mutex::new(HashMap::new()),
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

    async fn delete_item(&self, id: &str) {
        let now = unix_now();
        let deleted = self.store.lock().unwrap().soft_delete(id, now);
        match deleted {
            Ok(item) => {
                let ev = Self::make_event(EventKind::Delete, item, now);
                let _ = self.store.lock().unwrap().record_event(&ev);
                self.broadcast_event(&ev).await;
            }
            Err(e) => tracing::warn!(error = %e, "删除失败"),
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
        // ===== 通过身份校验 =====
        // S3 只收紧不放宽：学员回传的落库链路在 S4（issue #89 之后的
        // #17/#18）启用；此处保持 M1「学员回传未启用」语义，仅记录。
        // 对比 M1：原 A2 过滤（仅接受 owner==teacher）在此收紧后不可达——
        // 保留昵称 teacher 已在 join 层拒绝，故 socket 上不存在教师身份。
        tracing::info!(
            %session_id,
            event_id = %ev.event_id,
            "学员事件通过身份校验（回传入库在 S4 启用，暂不落库）"
        );
    }

    async fn on_file_chunk_stream(
        &self,
        session_id: &str,
        header: FileHeader,
        mut chunks: mpsc::Receiver<FileChunk>,
        _expected_sha256: String,
    ) -> FileAck {
        // M2 S3：仅已认证会话可发起文件流（net 层未 join 守卫之上再校验
        // conns 身份源）。学员→教师文件回传在 M2 S4 启用（FR-S-03），
        // 当前排空流并明确拒绝。
        let known = self.conns.lock().unwrap().contains_key(session_id);
        while chunks.recv().await.is_some() {}
        if !known {
            tracing::warn!(%session_id, item_id = %header.item_id, "未注册会话的文件流，拒绝");
            return FileAck {
                item_id: header.item_id,
                sha256_ok: false,
            };
        }
        tracing::warn!(%session_id, item_id = %header.item_id, "学员文件回传将在 M2 S4 启用，已拒绝");
        FileAck {
            item_id: header.item_id,
            sha256_ok: false,
        }
    }

    async fn on_disconnect(&self, session_id: &str) {
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

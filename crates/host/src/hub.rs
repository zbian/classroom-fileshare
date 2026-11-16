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
    is_reserved_device_name, Audience, Event, EventKind, MergeDecision, Role, ShareItem, CHUNK_SIZE,
};
use classroom_store::{sha256_file, ListFilter, PutOutcome, Store};

use crate::room::Admitter;
use crate::{HostCommand, HostEvent, HostStatus, StudentInfo};

/// 教师端固定的 owner_id。
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

    async fn on_event(&self, ev: Event, _tx: &mut mpsc::Sender<Frame>) {
        // PR #34 A2（最小加固）：仅接受教师端 owner 的事件，其余忽略——
        // 阻止定制客户端伪造任意 owner 入库；M2 回传的会话身份绑定另走接口修订
        if ev.item.owner_id != TEACHER_OWNER {
            tracing::warn!(
                owner = %ev.item.owner_id,
                event_id = %ev.event_id,
                "忽略非教师 owner 的事件"
            );
            return;
        }
        // 先取出决策再 match，确保 MutexGuard 不跨 await
        let decision = self.store.lock().unwrap().apply_remote(&ev);
        match decision {
            Ok(MergeDecision::Overwrite(item)) => {
                // 学员定向内容只入教师 store，不转发给其他学员（PRD 3.3）
                let _ = self.notify.send(HostEvent::ReceivedUpsert(*item)).await;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, event_id = %ev.event_id, "应用学员事件失败"),
        }
    }

    async fn on_file_chunk_stream(
        &self,
        header: FileHeader,
        mut chunks: mpsc::Receiver<FileChunk>,
        _expected_sha256: String,
    ) -> FileAck {
        // 学员→教师文件回传在 M2（P1，FR-S-03）启用；M1 排空流并明确拒绝
        while chunks.recv().await.is_some() {}
        tracing::warn!(item_id = %header.item_id, "学员文件回传将在 M2 启用，已拒绝");
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

    async fn on_manifest_req(&self, tx: &mut mpsc::Sender<Frame>) {
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

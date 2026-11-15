//! 会话枢纽：在线连接表、store 集成、事件与文件分块广播。
//!
//! 广播路径：教师 put → `Event(Upsert)` 先行，文件紧随 `FileHeader →
//! 1MB FileChunk × N → FileEnd`（PRD 7.1 统一发送链路）。
//! 学员→教师事件仅入教师 store（audience=Teacher，不转发，PRD 3.3）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU16, Ordering};
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
            event_id: Uuid::new_v4().to_string(),
            kind,
            item,
            new_name: None,
            acked_by: None,
            ts: now,
        }
    }

    async fn broadcast_event(&self, ev: &Event) {
        for tx in self.targets() {
            let _ = tx.send(Frame::Event(ev.clone())).await;
        }
    }

    async fn broadcast_file(&self, item: &ShareItem) {
        let Ok(body_path) = self.store.lock().unwrap().file_body(item) else {
            tracing::warn!(item_id = %item.id, "文件实体缺失，跳过推送");
            return;
        };
        let Ok(bytes) = std::fs::read(&body_path) else {
            tracing::warn!(item_id = %item.id, "文件读取失败，跳过推送");
            return;
        };
        let sha = item.sha256.clone().unwrap_or_default();
        for tx in self.targets() {
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
    }

    pub(crate) async fn handle(&self, cmd: &HostCommand) {
        match cmd {
            HostCommand::SendText { title, body } => self.send_text(title, body).await,
            HostCommand::SendFiles { paths } => self.send_files(paths).await,
            HostCommand::Delete { id } => self.delete_item(id).await,
            HostCommand::Shutdown => {}
        }
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
        {
            let conns = self.conns.lock().unwrap();
            if conns.values().any(|c| c.nickname == *nickname) {
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

    async fn on_manifest_req(&self, tx: &mut mpsc::Sender<Frame>) {
        let filter = ListFilter {
            include_deleted: true,
            ..Default::default()
        };
        let entries = match self.store.lock().unwrap().list(filter) {
            Ok(items) => items
                .into_iter()
                .map(|i| (i.id, i.version, i.deleted))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "读取版本清单失败");
                Vec::new()
            }
        };
        let _ = tx.send(Frame::Manifest(entries)).await;
    }
}

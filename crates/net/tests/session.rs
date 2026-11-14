//! Task 4（net）会话集成测试：真实 TCP 回环跑 Hello / Event / 分块文件 / Ping。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use classroom_net::{
    encode_frame, read_frame, run_session, FileAck, FileChunk, FileHeader, Frame, Hello,
    HelloReply, Server, SessionHandler, MAX_FRAME_SIZE,
};
use classroom_protocol::{Event, Role, PROTOCOL_VERSION};

fn sha256_hex(b: &[u8]) -> String {
    let d = Sha256::digest(b);
    let mut s = String::with_capacity(64);
    for byte in d {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// 会话回调测试桩：PIN 校验 + 版本校验 + 收集事件与文件字节。
struct MockHandler {
    pin: Option<String>,
    events_tx: mpsc::UnboundedSender<Event>,
    received_files: Mutex<HashMap<String, Vec<u8>>>,
    disconnects: Mutex<Vec<String>>,
}

impl MockHandler {
    fn new(pin: Option<&str>) -> (Arc<Self>, mpsc::UnboundedReceiver<Event>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                pin: pin.map(str::to_string),
                events_tx: tx,
                received_files: Mutex::new(HashMap::new()),
                disconnects: Mutex::new(Vec::new()),
            }),
            rx,
        )
    }
}

#[async_trait]
impl SessionHandler for MockHandler {
    async fn on_hello(&self, hello: Hello, _tx: &mut mpsc::Sender<Frame>) -> HelloReply {
        let Hello::Join {
            protocol_version,
            nickname,
            pin,
            ..
        } = hello;
        if protocol_version != PROTOCOL_VERSION {
            return HelloReply::Err {
                code: classroom_net::HelloErr::VersionMismatch,
            };
        }
        if nickname == "dup" {
            return HelloReply::Err {
                code: classroom_net::HelloErr::DupNickname,
            };
        }
        if self.pin != pin {
            return HelloReply::Err {
                code: classroom_net::HelloErr::WrongPin,
            };
        }
        HelloReply::Ok {
            session_id: "sess-1".into(),
        }
    }

    async fn on_event(&self, ev: Event, _tx: &mut mpsc::Sender<Frame>) {
        self.events_tx.send(ev).expect("event channel alive");
    }

    async fn on_file_chunk_stream(
        &self,
        header: FileHeader,
        mut chunks: mpsc::Receiver<FileChunk>,
        _expected_sha256: String,
    ) -> FileAck {
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = chunks.recv().await {
            assert_eq!(chunk.item_id, header.item_id);
            buf.extend_from_slice(&chunk.bytes);
        }
        let ok = sha256_hex(&buf) == header.sha256 && buf.len() as u64 == header.size;
        self.received_files
            .lock()
            .unwrap()
            .insert(header.item_id.clone(), buf);
        FileAck {
            item_id: header.item_id,
            sha256_ok: ok,
        }
    }

    async fn on_disconnect(&self, session_id: &str) {
        self.disconnects
            .lock()
            .unwrap()
            .push(session_id.to_string());
    }

    async fn on_manifest_req(&self, tx: &mut mpsc::Sender<Frame>) {
        tx.send(Frame::Manifest(vec![("item-a".into(), 1, false)]))
            .await
            .ok();
    }
}

async fn send(sock: &mut TcpStream, f: &Frame) {
    sock.write_all(&encode_frame(f)).await.expect("write frame");
}

async fn spawn_server(
    pin: Option<&str>,
) -> (
    std::net::SocketAddr,
    mpsc::UnboundedReceiver<Event>,
    Arc<MockHandler>,
) {
    let (handler, events_rx) = MockHandler::new(pin);
    let h = handler.clone();
    let server = Server::bind("127.0.0.1:0".parse().unwrap(), handler)
        .await
        .expect("bind");
    let addr = server.local_addr();
    tokio::spawn(server.run());
    (addr, events_rx, h)
}

fn sample_event(title: &str) -> Event {
    use classroom_protocol::{Audience, EventKind, ItemKind, ShareItem, SyncState};
    Event {
        event_id: format!("ev-{title}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: format!("id-{title}"),
            kind: ItemKind::Text,
            owner_id: "teacher".into(),
            owner_role: Role::Teacher,
            audience: Audience::All,
            title: title.into(),
            size: 2,
            sha256: None,
            rel_path: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            deleted: false,
            sync_state: SyncState::Synced,
        },
        new_name: None,
        acked_by: None,
        ts: 9,
    }
}

#[tokio::test]
async fn tcp_session_hello_event_file_roundtrip() {
    let (addr, mut events_rx, handler) = spawn_server(None).await;
    let mut sock =
        tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(addr))
            .await
            .expect("connect timeout")
            .expect("connect");

    // 1) Hello → Ok
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "alice".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::HelloReply(HelloReply::Ok { session_id }) => assert!(!session_id.is_empty()),
        other => panic!("expected HelloReply::Ok, got {other:?}"),
    }

    // 2) Event 到达 handler
    send(&mut sock, &Frame::Event(sample_event("通知"))).await;
    let got = tokio::time::timeout(std::time::Duration::from_secs(5), events_rx.recv())
        .await
        .expect("event timeout")
        .expect("event present");
    assert_eq!(got.event_id, "ev-通知");

    // 3) 分块文件：2.5MB 分 3 块（1MB + 1MB + 0.5MB），回环 sha256 校验 FileAck
    let payload: Vec<u8> = (0..2_621_440u32).map(|i| (i % 251) as u8).collect();
    let total_sha = sha256_hex(&payload);
    send(
        &mut sock,
        &Frame::FileHeader(FileHeader {
            item_id: "file-1".into(),
            file_name: "课件.zip".into(),
            size: payload.len() as u64,
            sha256: total_sha,
        }),
    )
    .await;
    for (seq, part) in payload.chunks(1024 * 1024).enumerate() {
        // 顺带守护：1MB 分块编码后低于 8MB 帧上限（lead 要求的不变量）
        let chunk_frame = Frame::FileChunk(FileChunk {
            item_id: "file-1".into(),
            seq: seq as u64,
            bytes: part.to_vec(),
        });
        assert!(encode_frame(&chunk_frame).len() < MAX_FRAME_SIZE as usize);
        send(&mut sock, &chunk_frame).await;
    }
    send(
        &mut sock,
        &Frame::FileEnd(classroom_net::FileEnd {
            item_id: "file-1".into(),
        }),
    )
    .await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::FileAck(ack) => {
            assert_eq!(ack.item_id, "file-1");
            assert!(ack.sha256_ok, "整体 SHA-256 校验应通过");
        }
        other => panic!("expected FileAck, got {other:?}"),
    }
    // handler 侧收到的字节与发送内容完全一致
    assert_eq!(
        handler.received_files.lock().unwrap().get("file-1"),
        Some(&payload)
    );

    // 4) Ping → Pong
    send(&mut sock, &Frame::Ping).await;
    assert!(matches!(read_frame(&mut sock).await.unwrap(), Frame::Pong));
}

#[tokio::test]
async fn hello_wrong_pin_rejected() {
    let (addr, _rx, _h) = spawn_server(Some("9527")).await;
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "alice".into(),
            role: Role::Student,
            pin: Some("0000".into()),
        }),
    )
    .await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::HelloReply(HelloReply::Err { code }) => {
            assert_eq!(code, classroom_net::HelloErr::WrongPin);
        }
        other => panic!("expected Err(WrongPin), got {other:?}"),
    }
}

#[tokio::test]
async fn hello_version_mismatch_and_dup_nickname_rejected() {
    let (addr, _rx, _h) = spawn_server(None).await;
    let mut sock = TcpStream::connect(addr).await.expect("connect");
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION + 1,
            nickname: "alice".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::HelloReply(HelloReply::Err { code }) => {
            assert_eq!(code, classroom_net::HelloErr::VersionMismatch);
        }
        other => panic!("expected Err(VersionMismatch), got {other:?}"),
    }
    // 昵称重复由实现方逻辑拒绝（mock 对 "dup" 特判）
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "dup".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::HelloReply(HelloReply::Err { code }) => {
            assert_eq!(code, classroom_net::HelloErr::DupNickname);
        }
        other => panic!("expected Err(DupNickname), got {other:?}"),
    }
}

/// run_session 可独立于 Server 使用（agent 侧复用同一会话循环）。
#[tokio::test]
async fn run_session_directly_with_stream() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (handler, _rx) = MockHandler::new(None);
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        run_session(stream, handler).await.ok();
    });
    let mut sock = TcpStream::connect(addr).await.unwrap();
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "bob".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    assert!(matches!(
        read_frame(&mut sock).await.unwrap(),
        Frame::HelloReply(HelloReply::Ok { .. })
    ));
}

/// 接口修订 #4：join 成功后连接断开，on_disconnect 带 session_id 回调。
#[tokio::test]
async fn disconnect_after_join_triggers_on_disconnect() {
    let (addr, _rx, handler) = spawn_server(None).await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "carol".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    assert!(matches!(
        read_frame(&mut sock).await.unwrap(),
        Frame::HelloReply(HelloReply::Ok { .. })
    ));
    drop(sock); // 断线
                // 轮询等待断开回调到达（会话结束是异步的）
    let mut seen = false;
    for _ in 0..100 {
        if !handler.disconnects.lock().unwrap().is_empty() {
            seen = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(seen, "join 成功后断线必须触发 on_disconnect");
}

/// 接口修订 #4：ManifestReq 经 on_manifest_req 回调应答。
#[tokio::test]
async fn manifest_req_answered_via_callback() {
    let (addr, _rx, _h) = spawn_server(None).await;
    let mut sock = TcpStream::connect(addr).await.unwrap();
    send(
        &mut sock,
        &Frame::Hello(Hello::Join {
            protocol_version: PROTOCOL_VERSION,
            nickname: "dave".into(),
            role: Role::Student,
            pin: None,
        }),
    )
    .await;
    assert!(matches!(
        read_frame(&mut sock).await.unwrap(),
        Frame::HelloReply(HelloReply::Ok { .. })
    ));
    send(&mut sock, &Frame::ManifestReq).await;
    match read_frame(&mut sock).await.unwrap() {
        Frame::Manifest(entries) => assert_eq!(entries, vec![("item-a".to_string(), 1, false)]),
        other => panic!("expected Manifest, got {other:?}"),
    }
}

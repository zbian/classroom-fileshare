//! Task 5（host）集成测试：真实 TCP 回环，≥2 模拟学员——准入、广播、
//! 断线重连 Manifest 对齐、文件分块推送。

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use classroom_host::{HostCommand, HostConfig, HostEvent};
use classroom_net::{encode_frame, read_frame, FileHeader, Frame, Hello, HelloErr, HelloReply};
use classroom_protocol::{Role, PROTOCOL_VERSION};
use classroom_store::{ListFilter, Store};

const TIMEOUT: Duration = Duration::from_secs(5);

fn sha256_hex(b: &[u8]) -> String {
    let d = Sha256::digest(b);
    let mut s = String::with_capacity(64);
    for byte in d {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

async fn next_event(rx: &mut mpsc::Receiver<HostEvent>) -> HostEvent {
    tokio::time::timeout(TIMEOUT, rx.recv())
        .await
        .expect("等待 host 事件超时")
        .expect("host 事件通道已关闭")
}

/// 跳过 Status 等噪音，取第一个匹配的事件。
async fn next_matching<T>(
    rx: &mut mpsc::Receiver<HostEvent>,
    f: impl Fn(HostEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("等待目标事件超时")
            .expect("host 事件通道已关闭");
        if let Some(t) = f(ev) {
            return t;
        }
    }
}

struct Host {
    #[allow(dead_code)]
    _guard: tempfile::TempDir,
    data_root: PathBuf,
    room_code: String,
    cmd: mpsc::Sender<HostCommand>,
    events: mpsc::Receiver<HostEvent>,
    port: u16,
}

async fn start_host(pin: Option<String>) -> Host {
    let guard = tempfile::tempdir().unwrap();
    let data_root = guard.path().to_path_buf();
    let cfg = HostConfig {
        room_code: "XK-204".into(),
        pin,
        data_root: data_root.clone(),
        tcp_port: 0,
    };
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (ev_tx, mut ev_rx) = mpsc::channel(128);
    tokio::spawn(classroom_host::run_host(cfg, cmd_rx, ev_tx));
    let status = match next_event(&mut ev_rx).await {
        HostEvent::Started(s) => s,
        other => panic!("expected Started first, got {other:?}"),
    };
    Host {
        _guard: guard,
        data_root,
        room_code: status.room_code,
        cmd: cmd_tx,
        events: ev_rx,
        port: status.tcp_port,
    }
}

struct MiniAgent {
    sock: TcpStream,
}

async fn send_frame(sock: &mut TcpStream, f: &Frame) {
    sock.write_all(&encode_frame(f)).await.expect("write frame");
}

impl MiniAgent {
    /// 加入房间；Err 返回拒绝码。
    async fn join(port: u16, nickname: &str, pin: Option<&str>) -> Result<MiniAgent, HelloErr> {
        let mut sock = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        send_frame(
            &mut sock,
            &Frame::Hello(Hello::Join {
                protocol_version: PROTOCOL_VERSION,
                nickname: nickname.into(),
                role: Role::Student,
                pin: pin.map(str::to_string),
            }),
        )
        .await;
        match read_frame(&mut sock).await.expect("hello reply") {
            Frame::HelloReply(HelloReply::Ok { .. }) => Ok(MiniAgent { sock }),
            Frame::HelloReply(HelloReply::Err { code }) => Err(code),
            other => panic!("expected HelloReply, got {other:?}"),
        }
    }

    async fn next(&mut self) -> Frame {
        tokio::time::timeout(TIMEOUT, read_frame(&mut self.sock))
            .await
            .expect("等待帧超时")
            .expect("帧解码失败")
    }
}

#[tokio::test]
async fn started_auto_port_assigned() {
    let host = start_host(None).await;
    assert_eq!(host.room_code, "XK-204");
    assert_ne!(host.port, 0, "tcp_port=0 应由内核分配实际端口");
    // cmd 通道 drop → run_host 退出（beacon 停止）
}

#[tokio::test]
async fn broadcast_text_reaches_two_students_and_stores() {
    let mut host = start_host(None).await;
    let port = host.port;
    // 两个模拟学员加入
    let mut a = MiniAgent::join(port, "alice", None)
        .await
        .expect("alice join");
    next_matching(&mut host.events, |e| match e {
        HostEvent::StudentJoined(n) if n == "alice" => Some(n),
        _ => None,
    })
    .await;
    let mut b = MiniAgent::join(port, "bob", None).await.expect("bob join");
    next_matching(&mut host.events, |e| match e {
        HostEvent::StudentJoined(n) if n == "bob" => Some(n),
        _ => None,
    })
    .await;

    // 教师 put_text 广播
    host.cmd
        .send(HostCommand::SendText {
            title: "通知".into(),
            body: "明天带教材".into(),
        })
        .await
        .unwrap();

    // A/B 都收到 Event(Upsert)，字段符合教师广播
    let item_from = |f: Frame| match f {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };
    let item_a = item_from(a.next().await);
    let item_b = item_from(b.next().await);
    for item in [&item_a, &item_b] {
        assert_eq!(item.title, "通知");
        assert_eq!(item.owner_id, "teacher");
        assert_eq!(item.audience, classroom_protocol::Audience::All);
        assert_eq!(item.version, 1);
    }

    // host 侧索引有且仅有一条；学员端 apply_remote 后一致
    let teacher_store = Store::open(&host.data_root).unwrap();
    let listed = teacher_store.list(ListFilter::default()).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "通知");

    let alice_dir = tempfile::tempdir().unwrap();
    let alice_store = Store::open(alice_dir.path()).unwrap();
    let ev = classroom_protocol::Event {
        event_id: format!("ev-{}", item_a.id),
        kind: classroom_protocol::EventKind::Upsert,
        item: item_a.clone(),
        new_name: None,
        acked_by: None,
        ts: 1,
    };
    assert!(matches!(
        alice_store.apply_remote(&ev).unwrap(),
        classroom_protocol::MergeDecision::Overwrite(_)
    ));
    assert_eq!(alice_store.list(ListFilter::default()).unwrap().len(), 1);

    // host 通知 GUI 刷新
    next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.title == "通知" => Some(i),
        _ => None,
    })
    .await;
}

#[tokio::test]
async fn wrong_pin_and_dup_nickname_rejected() {
    let host = start_host(Some("9527".into())).await;
    let port = host.port;
    // 错 PIN 拒绝
    assert_eq!(
        MiniAgent::join(port, "alice", Some("0000")).await.err(),
        Some(HelloErr::WrongPin)
    );
    // 正确 PIN 加入成功（保活连接，否则 drop 会触发 on_disconnect 注销昵称）
    let _online = MiniAgent::join(port, "alice", Some("9527"))
        .await
        .expect("join ok");
    // 重名拒绝（EX-02）
    assert_eq!(
        MiniAgent::join(port, "alice", Some("9527")).await.err(),
        Some(HelloErr::DupNickname)
    );
}

#[tokio::test]
async fn disconnect_then_reconnect_gets_manifest() {
    let mut host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");
    host.cmd
        .send(HostCommand::SendText {
            title: "课件清单".into(),
            body: "第 1 章".into(),
        })
        .await
        .unwrap();
    let ev_item = match a.next().await {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };

    // 断线 → StudentLeft
    drop(a);
    next_matching(&mut host.events, |e| match e {
        HostEvent::StudentLeft(n) if n == "alice" => Some(n),
        _ => None,
    })
    .await;

    // 重连（同名可重新加入）→ 发 ManifestReq → 收到含条目的 Manifest
    let mut a2 = MiniAgent::join(port, "alice", None).await.expect("rejoin");
    send_frame(&mut a2.sock, &Frame::ManifestReq).await;
    match a2.next().await {
        Frame::Manifest(entries) => {
            assert!(
                entries.contains(&(ev_item.id.clone(), 1, false)),
                "Manifest 应含教师条目 {ev_item:?}，实际 {entries:?}"
            );
        }
        other => panic!("expected Manifest, got {other:?}"),
    }
}

#[tokio::test]
async fn teacher_file_broadcast_chunks_and_sha() {
    let host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");

    // 2.5MB 文件（3 块：1MB + 1MB + 0.5MB）
    let payload: Vec<u8> = (0..2_621_440u32).map(|i| (i % 251) as u8).collect();
    let src_dir = tempfile::tempdir().unwrap();
    let file_path = src_dir.path().join("课件.bin");
    std::fs::write(&file_path, &payload).unwrap();

    host.cmd
        .send(HostCommand::SendFiles {
            paths: vec![file_path],
        })
        .await
        .unwrap();

    // 学员视角：Event(Upsert, File) → FileHeader → chunks → FileEnd
    let item = match a.next().await {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };
    assert_eq!(item.kind, classroom_protocol::ItemKind::File);
    assert_eq!(item.title, "课件.bin");

    let header: FileHeader = match a.next().await {
        Frame::FileHeader(h) => h,
        other => panic!("expected FileHeader, got {other:?}"),
    };
    assert_eq!(header.file_name, "课件.bin");
    assert_eq!(header.size, payload.len() as u64);
    assert_eq!(header.sha256, sha256_hex(&payload));

    let mut got: Vec<u8> = Vec::new();
    let mut chunk_count = 0u32;
    loop {
        match a.next().await {
            Frame::FileChunk(c) => {
                assert_eq!(c.item_id, item.id);
                got.extend_from_slice(&c.bytes);
                chunk_count += 1;
            }
            Frame::FileEnd(_) => break,
            other => panic!("expected chunk/end, got {other:?}"),
        }
    }
    assert_eq!(chunk_count, 3, "2.5MB 按 1MB 分块应为 3 块");
    assert_eq!(got, payload, "分块重组后字节一致");
    assert_eq!(sha256_hex(&got), header.sha256, "整体 SHA-256 一致");
}

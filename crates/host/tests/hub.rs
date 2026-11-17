//! Task 5（host）集成测试：真实 TCP 回环，≥2 模拟学员——准入、广播、
//! 断线重连 Manifest 对齐、文件分块推送。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use classroom_host::{HostCommand, HostConfig, HostEvent};
use classroom_net::{encode_frame, read_frame, FileHeader, Frame, Hello, HelloErr, HelloReply};
use classroom_protocol::{ItemKind, Role, MAX_FILE_SIZE, PROTOCOL_VERSION};
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

/// 模拟学员：独立 reader 任务自动应答 Ping（可用 silent 模式模拟合盖
/// 休眠——连接保持但不回 Pong），其余帧经队列供测试消费。
struct MiniAgent {
    writer: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
    frames: mpsc::Receiver<Frame>,
    kill: Arc<tokio::sync::Notify>,
}

impl MiniAgent {
    /// 加入房间（自动应答 Ping）；Err 返回拒绝码。
    async fn join(port: u16, nickname: &str, pin: Option<&str>) -> Result<Self, HelloErr> {
        Self::join_probe(port, nickname, pin, true).await
    }

    /// 加入房间；`pong_reply=false` 时对服务端 Ping 不应答（僵死连接语义）。
    async fn join_probe(
        port: u16,
        nickname: &str,
        pin: Option<&str>,
        pong_reply: bool,
    ) -> Result<Self, HelloErr> {
        let sock = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let (mut reader, writer) = sock.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer));
        {
            let mut w = writer.lock().await;
            w.write_all(&encode_frame(&Frame::Hello(Hello::Join {
                protocol_version: PROTOCOL_VERSION,
                nickname: nickname.into(),
                role: Role::Student,
                pin: pin.map(str::to_string),
            })))
            .await
            .expect("write hello");
        }
        match read_frame(&mut reader).await.expect("hello reply") {
            Frame::HelloReply(HelloReply::Ok { .. }) => {}
            Frame::HelloReply(HelloReply::Err { code }) => return Err(code),
            other => panic!("expected HelloReply, got {other:?}"),
        }
        let (tx, frames) = mpsc::channel(64);
        let reply_writer = writer.clone();
        let kill = Arc::new(tokio::sync::Notify::new());
        let task_kill = kill.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = task_kill.notified() => break, // halves 随任务结束 drop → 连接关闭
                    frame = read_frame(&mut reader) => match frame {
                        Ok(Frame::Ping) if pong_reply => {
                            let mut w = reply_writer.lock().await;
                            if w.write_all(&encode_frame(&Frame::Pong)).await.is_err() {
                                break;
                            }
                        }
                        Ok(Frame::Ping) => {} // silent：吞掉不应答（模拟休眠）
                        Ok(f) => {
                            if tx.send(f).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
        });
        Ok(Self {
            writer,
            frames,
            kill,
        })
    }

    /// 显式断开（halves 持有在 reader 任务，drop 本结构不会关连接）。
    async fn hang_up(&self) {
        self.kill.notify_waiters();
    }

    async fn send(&self, f: &Frame) {
        let mut w = self.writer.lock().await;
        w.write_all(&encode_frame(f)).await.expect("write frame");
    }

    async fn next(&mut self) -> Frame {
        tokio::time::timeout(TIMEOUT, self.frames.recv())
            .await
            .expect("等待帧超时")
            .expect("帧通道已关闭")
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

/// PR #30 检查点：Windows/NTFS 保留设备名昵称拒绝加入。
#[tokio::test]
async fn reserved_device_name_nicknames_rejected() {
    let host = start_host(None).await;
    let port = host.port;
    assert_eq!(
        MiniAgent::join(port, "CON", None).await.err(),
        Some(HelloErr::InvalidNickname)
    );
    assert_eq!(
        MiniAgent::join(port, "com1", None).await.err(),
        Some(HelloErr::InvalidNickname)
    );
    // 保留名近似词不受影响（"console" 非保留名）
    MiniAgent::join(port, "console", None)
        .await
        .expect("近似词昵称应可正常加入");
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

    // 断线 → StudentLeft（halves 在 reader 任务，需显式挂断）
    a.hang_up().await;
    drop(a);
    next_matching(&mut host.events, |e| match e {
        HostEvent::StudentLeft(n) if n == "alice" => Some(n),
        _ => None,
    })
    .await;

    // 重连（同名可重新加入）→ 发 ManifestReq → 收到含条目的 Manifest
    let mut a2 = MiniAgent::join(port, "alice", None).await.expect("rejoin");
    a2.send(&Frame::ManifestReq).await;
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

// ---------- PR #34 A：事件来源加固与清单过滤 ----------

#[tokio::test]
async fn non_teacher_event_ignored_and_manifest_excludes_it() {
    let mut host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");

    // 教师广播一条（进入清单）
    host.cmd
        .send(HostCommand::SendText {
            title: "通知".into(),
            body: "明天带教材".into(),
        })
        .await
        .unwrap();
    let teacher_item = match a.next().await {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };
    // 消费正文文件帧（Event 后紧跟 FileHeader → … → FileEnd）
    loop {
        match a.next().await {
            Frame::FileEnd(_) => break,
            Frame::FileHeader(_) | Frame::FileChunk(_) => continue,
            other => panic!("正文推送帧序列异常: {other:?}"),
        }
    }

    // 伪造事件（定制客户端直连注入 owner="小林"）——必须被忽略
    let forged = classroom_protocol::Event {
        event_id: "forged-1".into(),
        kind: classroom_protocol::EventKind::Upsert,
        item: classroom_protocol::ShareItem {
            id: "forged-item".into(),
            kind: ItemKind::Text,
            owner_id: "小林".into(),
            owner_role: Role::Student,
            audience: classroom_protocol::Audience::All,
            title: "伪造条目".into(),
            size: 1,
            sha256: None,
            rel_path: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            deleted: false,
            sync_state: classroom_protocol::SyncState::Synced,
        },
        new_name: None,
        acked_by: None,
        ts: 1,
    };
    a.send(&Frame::Event(forged)).await;
    // 给 host 处理时间，然后排水：不应出现伪造条目的 ReceivedUpsert
    tokio::time::sleep(Duration::from_millis(300)).await;
    let leaked = drain_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "小林" => Some(i),
        _ => None,
    });
    assert!(
        leaked.is_none(),
        "伪造 owner 事件不得入店或通知: {leaked:?}"
    );

    // 重连清单只含教师广播内容（A1：owner/audience 过滤）
    a.send(&Frame::ManifestReq).await;
    match a.next().await {
        Frame::Manifest(entries) => {
            assert_eq!(entries.len(), 1, "清单仅含教师条目: {entries:?}");
            assert_eq!(entries[0].0, teacher_item.id);
        }
        other => panic!("expected Manifest, got {other:?}"),
    }
}

/// 非阻塞排水：返回第一个匹配项（无则 None），不等待。
fn drain_matching<T>(
    rx: &mut mpsc::Receiver<HostEvent>,
    mut f: impl FnMut(HostEvent) -> Option<T>,
) -> Option<T> {
    while let Ok(ev) = rx.try_recv() {
        if let Some(t) = f(ev) {
            return Some(t);
        }
    }
    None
}

// ---------- PR #34 B：重名存活探测 ----------

#[tokio::test]
async fn live_connection_defends_duplicate_nickname() {
    let host = start_host(None).await;
    let port = host.port;
    // alice 在线且自动应答 Pong
    let _alive = MiniAgent::join(port, "alice", None).await.expect("join");
    // 第二个 alice：探测到旧连接存活 → 维持 EX-02 拒绝
    let err = MiniAgent::join(port, "alice", None).await.err();
    assert_eq!(err, Some(HelloErr::DupNickname));
}

#[tokio::test]
async fn dead_connection_evicted_and_nickname_reusable() {
    let host = start_host(None).await;
    let port = host.port;
    // alice「合盖休眠」：连接保持但不应答 Ping（silent 模式）
    let _sleeping = MiniAgent::join_probe(port, "alice", None, false)
        .await
        .expect("join");
    // 第二个 alice：探测 3s 无 Pong → 驱逐旧的 → 接受新 join
    let replacement = MiniAgent::join(port, "alice", None).await;
    assert!(replacement.is_ok(), "僵死连接应被驱逐、昵称可重新加入");
}

// ---------- E10（P0，PRD 5.3/9.4/EX-04）：100 MB 上限端到端 ----------

/// >100MB 文件经 host 发送链路快速失败：不推送任何帧、不入库；
/// 且拒绝后正常小文件仍可发送（EX-04 后半句）。稀疏文件构造，不真写 100MB。
#[tokio::test]
async fn oversized_file_rejected_and_small_file_still_sends() {
    let host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");

    let src = tempfile::tempdir().unwrap();
    let big = src.path().join("big.bin");
    let f = std::fs::File::create(&big).unwrap();
    f.set_len(MAX_FILE_SIZE + 1).unwrap();
    drop(f);

    host.cmd
        .send(HostCommand::SendFiles { paths: vec![big] })
        .await
        .unwrap();
    // 超限必须在「读内存/入库/广播」之前被拒：学员侧 300ms 内无任何帧
    let no_frame = tokio::time::timeout(Duration::from_millis(300), a.next()).await;
    assert!(no_frame.is_err(), "超限文件不应推送任何帧");
    let store = Store::open(&host.data_root).unwrap();
    assert!(
        store.list(ListFilter::default()).unwrap().is_empty(),
        "超限文件不得入库"
    );

    // EX-04 后半句：拒绝后链路仍正常
    let small = src.path().join("small.txt");
    std::fs::write(&small, b"ok").unwrap();
    host.cmd
        .send(HostCommand::SendFiles { paths: vec![small] })
        .await
        .unwrap();
    let item = match a.next().await {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };
    assert_eq!(item.title, "small.txt");
    assert_eq!(item.size, 2);
}

/// .exe 端到端拒绝：确认 is_risky_ext 在 host → store put 路径生效（EX-05），
/// 且拒绝后正常文件仍可发送。
#[tokio::test]
async fn executable_rejected_end_to_end() {
    let host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");

    let src = tempfile::tempdir().unwrap();
    let exe = src.path().join("virus.exe");
    std::fs::write(&exe, b"MZ").unwrap();
    host.cmd
        .send(HostCommand::SendFiles { paths: vec![exe] })
        .await
        .unwrap();
    let no_frame = tokio::time::timeout(Duration::from_millis(300), a.next()).await;
    assert!(no_frame.is_err(), "高风险类型不应推送任何帧");
    let store = Store::open(&host.data_root).unwrap();
    assert!(store.list(ListFilter::default()).unwrap().is_empty());

    let ok_file = src.path().join("课件.pdf");
    std::fs::write(&ok_file, b"%PDF-1.7").unwrap();
    host.cmd
        .send(HostCommand::SendFiles {
            paths: vec![ok_file],
        })
        .await
        .unwrap();
    let item = match a.next().await {
        Frame::Event(ev) => ev.item,
        other => panic!("expected Event, got {other:?}"),
    };
    assert_eq!(item.title, "课件.pdf");
}

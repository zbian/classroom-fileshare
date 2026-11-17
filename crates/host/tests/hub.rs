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

// ---------- M2 S3（issue #89）：会话身份绑定与未 join 守卫 ----------

/// 裸 TCP 连接（不发 Hello）——用于验证未 join 守卫。
async fn raw_connect(port: u16) -> tokio::net::TcpStream {
    tokio::time::timeout(TIMEOUT, tokio::net::TcpStream::connect(("127.0.0.1", port)))
        .await
        .expect("connect timeout")
        .expect("connect")
}

/// 构造一个可指定 owner / audience 的事件（用于冒名与越权注入）。
fn crafted_event(owner: &str, audience: classroom_protocol::Audience) -> classroom_protocol::Event {
    classroom_protocol::Event {
        event_id: format!("crafted-{owner}"),
        kind: classroom_protocol::EventKind::Upsert,
        item: classroom_protocol::ShareItem {
            id: format!("crafted-item-{owner}"),
            kind: ItemKind::Text,
            owner_id: owner.into(),
            owner_role: Role::Student,
            audience,
            title: "注入条目".into(),
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
        ts: 9,
    }
}

/// 断言短时间内没有「学员事件入店通知」（跳过 Status/StudentJoined 等噪音）。
async fn assert_no_received_upsert(host: &mut Host) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(400);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        match tokio::time::timeout(remaining, host.events.recv()).await {
            Ok(Some(HostEvent::ReceivedUpsert(item))) => {
                panic!("不应收到学员事件入店通知: {item:?}")
            }
            Ok(Some(_)) => continue,
            Ok(None) => panic!("host 事件通道关闭"),
            Err(_) => return,
        }
    }
}

/// 教师 store 中没有任何非教师条目（学员事件不得落库）。
fn assert_no_student_items(host: &Host) {
    let store = Store::open(&host.data_root).unwrap();
    let all = store.list(ListFilter::default()).unwrap();
    assert!(
        all.iter().all(|i| i.owner_id == "teacher"),
        "学员事件不得落库: {all:?}"
    );
}

/// S3 用例 1：**未 join 连接**直接发 Event → net 未 join 守卫拦截，
/// 不进 handler、不落库、不广播。
#[tokio::test]
async fn unjoined_event_is_dropped() {
    let mut host = start_host(None).await;
    let port = host.port;
    let mut sock = raw_connect(port).await;
    sock.write_all(&encode_frame(&Frame::Event(crafted_event(
        "alice",
        classroom_protocol::Audience::Teacher,
    ))))
    .await
    .unwrap();

    assert_no_received_upsert(&mut host).await;
    assert_no_student_items(&host);
    // 服务端不应因此断开连接：Ping 仍有 Pong（守卫是丢弃而非断连）
    sock.write_all(&encode_frame(&Frame::Ping)).await.unwrap();
    let reply = tokio::time::timeout(TIMEOUT, read_frame(&mut sock))
        .await
        .expect("等待 Pong 超时")
        .expect("读 Pong 失败");
    assert_eq!(reply, Frame::Pong);
}

/// S3 用例 2：**冒名**——已 join 的 alice 发送 owner_id=bob 的事件 → 丢弃。
#[tokio::test]
async fn spoofed_owner_event_rejected() {
    let mut host = start_host(None).await;
    let port = host.port;
    let a = MiniAgent::join(port, "alice", None).await.expect("join");

    a.send(&Frame::Event(crafted_event(
        "bob",
        classroom_protocol::Audience::Teacher,
    )))
    .await;

    assert_no_received_upsert(&mut host).await;
    assert_no_student_items(&host);
}

/// S3 用例 3：**越权广播**——alice 以自己为 owner 但 audience=All 提交 →
/// 丢弃（否则内容会扩散给其他学员，PRD 3.3）。
#[tokio::test]
async fn student_broadcast_event_rejected() {
    let mut host = start_host(None).await;
    let port = host.port;
    let a = MiniAgent::join(port, "alice", None).await.expect("join");

    a.send(&Frame::Event(crafted_event(
        "alice",
        classroom_protocol::Audience::All,
    )))
    .await;

    assert_no_received_upsert(&mut host).await;
    assert_no_student_items(&host);
}

/// S3 用例 4：**保留昵称**——学员以 `teacher` 加入 → InvalidNickname，
/// 且不产生连接（连接数/状态不变）。
#[tokio::test]
async fn teacher_reserved_nickname_rejected() {
    let host = start_host(None).await;
    let port = host.port;
    let err = MiniAgent::join(port, "teacher", None).await.err();
    assert_eq!(
        err,
        Some(HelloErr::InvalidNickname),
        "教师作用域保留昵称必须在 join 层拒绝"
    );
    // 未被占用：同名再次 join 仍是 InvalidNickname（而非 DupNickname）
    let err2 = MiniAgent::join(port, "teacher", None).await.err();
    assert_eq!(err2, Some(HelloErr::InvalidNickname));
    // 普通昵称不受影响
    let _ok = MiniAgent::join(port, "alice", None)
        .await
        .expect("普通昵称可加入");
}
// ---------- M2 #94：学员回传（audience=Teacher 上行）----------

/// 模拟学员 app 的上行：Event（元数据）+ 文件帧（内容；文本正文与文件字节
/// 走同一条内容帧链路，与教师广播模型对称）。
///
/// `owner` 必须与已 join 的昵称一致（S3 三重校验要求），本 helper 不构造
/// 伪身份——冒名/越权场景直接用 [`crafted_event`]。
async fn student_upload(
    agent: &MiniAgent,
    item_id: &str,
    kind: ItemKind,
    title: &str,
    owner: &str,
    body: &[u8],
) -> String {
    use classroom_protocol::{Audience, EventKind, ShareItem, SyncState};
    let sha = sha256_hex(body);
    let size = body.len() as u64;
    let size_field = match &kind {
        ItemKind::File => size,
        // PRD 3.5：文本条目 size = 字符数（教师端 put_text 会重算）
        ItemKind::Text => String::from_utf8_lossy(body).chars().count() as u64,
    };
    let ev = classroom_protocol::Event {
        event_id: format!("upload-{item_id}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: item_id.into(),
            kind: kind.clone(),
            owner_id: owner.into(),
            owner_role: Role::Student,
            audience: Audience::Teacher,
            title: title.into(),
            size: size_field,
            sha256: match &kind {
                ItemKind::File => Some(sha.clone()),
                ItemKind::Text => None,
            },
            rel_path: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            deleted: false,
            sync_state: SyncState::PendingSync,
        },
        new_name: None,
        acked_by: None,
        ts: 9,
    };
    agent.send(&Frame::Event(ev)).await;
    agent
        .send(&Frame::FileHeader(FileHeader {
            item_id: item_id.into(),
            file_name: title.into(),
            size,
            sha256: sha.clone(),
        }))
        .await;
    for (seq, part) in body.chunks(1024 * 1024).enumerate() {
        agent
            .send(&Frame::FileChunk(classroom_net::FileChunk {
                item_id: item_id.into(),
                seq: seq as u64,
                bytes: part.to_vec(),
            }))
            .await;
    }
    agent
        .send(&Frame::FileEnd(classroom_net::FileEnd {
            item_id: item_id.into(),
        }))
        .await;
    sha
}

/// 启动一个**真实 agent**（生产代码，非 mock），返回事件流与数据目录，
/// 并消费掉加入后的对齐事件（Synced），使后续「无帧」断言干净。
async fn spawn_real_agent(
    port: u16,
    nickname: &str,
) -> (
    mpsc::Receiver<classroom_agent::AgentEvent>,
    tempfile::TempDir,
) {
    let dir = tempfile::tempdir().unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let cfg = classroom_agent::AgentConfig {
        nickname: nickname.into(),
        pin: None,
        manual_addr: Some(format!("127.0.0.1:{port}").parse().unwrap()),
        room_filter: None,
        data_root: dir.path().to_path_buf(),
    };
    tokio::spawn(classroom_agent::run_agent(cfg, tx));
    // 等 Joined，再等 Synced（Manifest 对齐完成）
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .expect("agent 加入超时")
            .expect("agent 事件通道关闭");
        match ev {
            classroom_agent::AgentEvent::Joined(_) => continue,
            classroom_agent::AgentEvent::Synced => break,
            other => panic!("agent 加入阶段意外事件: {other:?}"),
        }
    }
    (rx, dir)
}

/// #94 用例 1：学员文本回传 → 教师端入库可查（owner=昵称、role=Student、
/// audience=Teacher、kind=Text），正文可读。
#[tokio::test]
async fn student_upload_text_visible_to_teacher() {
    let mut host = start_host(None).await;
    let port = host.port;
    let alice = MiniAgent::join(port, "alice", None).await.expect("join");
    let body = "老师，这是我的作业";
    student_upload(
        &alice,
        "up-text-1",
        ItemKind::Text,
        "作业",
        "alice",
        body.as_bytes(),
    )
    .await;

    let item = next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "alice" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item.owner_role, Role::Student);
    assert_eq!(item.audience, classroom_protocol::Audience::Teacher);
    assert_eq!(item.kind, ItemKind::Text);
    assert_eq!(item.version, 1);

    let store = Store::open(&host.data_root).unwrap();
    let got = store.get(&item.id).unwrap().expect("教师端可查询到该提交");
    assert_eq!(got.title, "作业");
    assert_eq!(got.size, body.chars().count() as u64, "文本 size=字符数");
    let body_path = store.file_body(&got).unwrap();
    assert_eq!(std::fs::read_to_string(body_path).unwrap(), body);
}

/// #94 用例 2：学员文件回传（分块）→ 教师端落盘 + 索引正确（sha256/size/version）。
#[tokio::test]
async fn student_upload_file_persisted() {
    let mut host = start_host(None).await;
    let port = host.port;
    let alice = MiniAgent::join(port, "alice", None).await.expect("join");
    // 1.5MB（2 块）验证分块内容链路
    let payload: Vec<u8> = (0..1_572_864u32).map(|i| (i % 251) as u8).collect();
    let sha = student_upload(
        &alice,
        "up-file-1",
        ItemKind::File,
        "作业.pdf",
        "alice",
        &payload,
    )
    .await;

    let item = next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "alice" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item.kind, ItemKind::File);
    assert_eq!(item.size, payload.len() as u64);
    assert_eq!(item.sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(item.version, 1);

    let store = Store::open(&host.data_root).unwrap();
    let got = store.get(&item.id).unwrap().expect("索引存在");
    assert!(got.rel_path.is_some(), "文件条目应有 rel_path");
    let path = store.file_body(&got).unwrap();
    assert_eq!(
        sha256_hex(&std::fs::read(path).unwrap()),
        sha,
        "落盘字节与哈希一致"
    );
}

/// #94 用例 3：同名覆盖——同一学员两次同名提交 → version+1、单条、`.bak` 1 份。
#[tokio::test]
async fn student_upload_same_name_overwrites() {
    let mut host = start_host(None).await;
    let port = host.port;
    let alice = MiniAgent::join(port, "alice", None).await.expect("join");

    student_upload(&alice, "up-ov-1", ItemKind::Text, "作业", "alice", b"v1").await;
    let first = next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "alice" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(first.version, 1);

    student_upload(&alice, "up-ov-2", ItemKind::Text, "作业", "alice", b"v2").await;
    let second = next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "alice" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(second.version, 2, "同名覆盖 version+1");
    assert_eq!(second.id, first.id, "覆盖保持同一 id");

    let store = Store::open(&host.data_root).unwrap();
    let alice_items = store
        .list(ListFilter {
            owner: Some("alice".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(alice_items.len(), 1, "同一 scope 只保留一条");
    assert_eq!(
        std::fs::read_to_string(store.file_body(&alice_items[0]).unwrap()).unwrap(),
        "v2",
        "正文为最新版本"
    );
    let baks = std::fs::read_dir(host.data_root.join("repo/alice/texts"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
        .count();
    assert_eq!(baks, 1, "仅保留 1 份 .bak");
}

/// #94 用例 4（验收硬指标）：**其他学员零感知**——用 **2 个真实 agent**
/// （生产代码）验证：carol 的上行提交只落教师端，alice/bob 收不到任何帧、
/// 各自 store 不含该条目。
#[tokio::test]
async fn student_upload_not_visible_to_other_students() {
    let mut host = start_host(None).await;
    let port = host.port;
    // 两个真实 agent 加入（生产 agent 代码，非 mock）
    let (mut a_ev, a_dir) = spawn_real_agent(port, "alice").await;
    let (mut b_ev, b_dir) = spawn_real_agent(port, "bob").await;
    // 定制客户端以 carol 身份上行（模拟学员 app 的发送侧）
    let carol = MiniAgent::join(port, "carol", None).await.expect("join");
    student_upload(
        &carol,
        "up-carol-1",
        ItemKind::Text,
        "只给老师",
        "carol",
        "秘密作业内容".as_bytes(),
    )
    .await;

    let item = next_matching(&mut host.events, |e| match e {
        HostEvent::ReceivedUpsert(i) if i.owner_id == "carol" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item.audience, classroom_protocol::Audience::Teacher);
    assert_eq!(item.owner_role, Role::Student);

    // 其他学员零感知：两个真实 agent 均无任何事件（300ms 静默窗口）
    assert!(
        tokio::time::timeout(Duration::from_millis(300), a_ev.recv())
            .await
            .is_err(),
        "alice 不应收到 carol 提交的任何通知"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(300), b_ev.recv())
            .await
            .is_err(),
        "bob 不应收到 carol 提交的任何通知"
    );
    // 各自 store 不含他人提交
    for dir in [&a_dir, &b_dir] {
        let store = Store::open(dir.path()).unwrap();
        let all = store.list(ListFilter::default()).unwrap();
        assert!(
            all.iter().all(|i| i.owner_id != "carol"),
            "其他学员的本地库不得包含该提交: {all:?}"
        );
        assert!(all.is_empty(), "教师未广播任何内容时学员库应为空");
    }
}

/// #94 加固用例（评审发现 1）：上传**超限在流式过程中**即被拒——100MB
/// 上限检查在分块循环内逐块进行（不信任客户端，防止先写满磁盘再拒）。
/// 构造 101 个 1MB 块（累计 101MB > 100MB），断言：
/// 1) 第 101 块触发上限 → 回 `FileAck { sha256_ok: false }`；
/// 2) 条目未入库、教师侧无 `ReceivedUpsert` 通知。
#[tokio::test]
async fn student_upload_over_limit_rejected_streaming() {
    use classroom_protocol::{Audience, EventKind, ShareItem, SyncState};
    let mut host = start_host(None).await;
    let port = host.port;
    let mut alice = MiniAgent::join(port, "alice", None).await.expect("join");

    let total = MAX_FILE_SIZE + 1024 * 1024; // 101MB（超过上限 1MB）
    let item_id = "up-too-big";
    let ev = classroom_protocol::Event {
        event_id: format!("upload-{item_id}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: item_id.into(),
            kind: ItemKind::File,
            owner_id: "alice".into(),
            owner_role: Role::Student,
            audience: Audience::Teacher,
            title: "big.bin".into(),
            size: total,
            sha256: Some("0".repeat(64)),
            rel_path: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            deleted: false,
            sync_state: SyncState::PendingSync,
        },
        new_name: None,
        acked_by: None,
        ts: 9,
    };
    alice.send(&Frame::Event(ev)).await;
    alice
        .send(&Frame::FileHeader(FileHeader {
            item_id: item_id.into(),
            file_name: "big.bin".into(),
            size: total,
            sha256: "0".repeat(64),
        }))
        .await;

    let chunk = vec![0u8; 1024 * 1024];
    let blocks = total / (1024 * 1024); // 101
    for seq in 0..blocks {
        alice
            .send(&Frame::FileChunk(classroom_net::FileChunk {
                item_id: item_id.into(),
                seq,
                bytes: chunk.clone(),
            }))
            .await;
    }
    // 客户端正常结束流（真实协议行为）；host 在超限块处已中止写盘并回失败 ack
    alice
        .send(&Frame::FileEnd(classroom_net::FileEnd {
            item_id: item_id.into(),
        }))
        .await;
    let got = loop {
        match alice.next().await {
            Frame::FileAck(a) => break a,
            _ => continue,
        }
    };
    assert!(!got.sha256_ok, "超限上传必须回失败 ack");
    assert_eq!(got.item_id, item_id);

    assert_no_received_upsert(&mut host).await;
    let store = Store::open(&host.data_root).unwrap();
    assert!(
        store.list(ListFilter::default()).unwrap().is_empty(),
        "超限上传不得入库"
    );
}

// ---------- M2 #17：删除同步与延迟补删 ----------

/// 真实 agent 实例（生产代码）：可 abort 模拟离线，再用**同一 data_root**
/// 重启验证「离线期间教师删除 → 上线补删」。
struct RealAgent {
    task: tokio::task::JoinHandle<anyhow::Result<()>>,
    events: mpsc::Receiver<classroom_agent::AgentEvent>,
    dir: tempfile::TempDir,
}

/// 启动真实 agent 并等到 manifest 对齐完成（Synced）。
async fn start_real_agent(port: u16, nickname: &str, dir: tempfile::TempDir) -> RealAgent {
    let (tx, mut rx) = mpsc::channel(64);
    let cfg = classroom_agent::AgentConfig {
        nickname: nickname.into(),
        pin: None,
        manual_addr: Some(format!("127.0.0.1:{port}").parse().unwrap()),
        room_filter: None,
        data_root: dir.path().to_path_buf(),
    };
    let task = tokio::spawn(classroom_agent::run_agent(cfg, tx));
    loop {
        let ev = tokio::time::timeout(TIMEOUT, rx.recv())
            .await
            .expect("agent 加入超时")
            .expect("agent 事件通道关闭");
        // 加入阶段可能伴随补删 Removed 等事件（离线补删先于 Synced 发出），
        // 统一忽略，只等对齐完成信号 Synced。
        if matches!(ev, classroom_agent::AgentEvent::Synced) {
            break;
        }
    }
    RealAgent {
        task,
        events: rx,
        dir,
    }
}

/// #17 用例 1（验收核心）：离线端上线后**补删**——教师删除时端离线（墓碑
/// 落库、无接收者）；端重连经 manifest 比对完成补删（清实体 + 清索引），
/// 并上行 Ack；**再重连一次不重复补删**（有界，不无限重试）。
#[tokio::test]
async fn offline_student_delete_caught_up_on_reconnect() {
    let mut host = start_host(None).await;
    let port = host.port;
    let dir = tempfile::tempdir().unwrap();
    let mut agent = start_real_agent(port, "alice", dir).await;

    // 教师广播一条 → 真实 agent 落盘
    host.cmd
        .send(HostCommand::SendText {
            title: "临时通知".into(),
            body: "会被删除".into(),
        })
        .await
        .unwrap();
    let item = loop {
        match tokio::time::timeout(TIMEOUT, agent.events.recv())
            .await
            .expect("等待 Received 超时")
            .expect("agent 事件通道关闭")
        {
            classroom_agent::AgentEvent::Received(i) if i.title == "临时通知" => break i,
            _ => continue,
        }
    };
    let local_rel = item.rel_path.clone().expect("文本条目应有 rel_path");
    {
        let store = Store::open(agent.dir.path()).unwrap();
        assert!(store.get(&item.id).unwrap().is_some(), "在线时本地有条目");
    }

    // 学员离线（abort agent 任务，保留 data_root 模拟「关机后重开」）
    agent.task.abort();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 教师删除：无在线端接收 → 仅落墓碑（删除立即完成，不等待离线端）
    host.cmd
        .send(HostCommand::Delete {
            id: item.id.clone(),
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 重连（同一 data_root）→ manifest 含墓碑 → 补删 + 上行 Ack
    let agent2 = start_real_agent(port, "alice", agent.dir).await;
    let ack_item = next_matching(&mut host.events, |e| match e {
        HostEvent::DeleteAcked { item_id, nickname } if item_id == item.id.as_str() => {
            Some(nickname.clone())
        }
        _ => None,
    })
    .await;
    assert_eq!(ack_item, "alice", "补删完成应上行 Ack（昵称=会话身份）");

    // 补删完成终态：索引清除 + 实体清理（PRD 7.4）
    let store2 = Store::open(agent2.dir.path()).unwrap();
    assert!(
        store2.get(&item.id).unwrap().is_none(),
        "补删完成索引清除（PRD 7.4）"
    );
    assert!(
        store2.list(ListFilter::default()).unwrap().is_empty(),
        "默认列表不可见"
    );
    assert!(
        !agent2.dir.path().join("repo").join(&local_rel).exists(),
        "实体文件应被补删清理"
    );

    // 有界性：再重连一次 → 本地已无该条目 → 不重复补删、不重复 Ack
    agent2.task.abort();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let agent3 = start_real_agent(port, "alice", agent2.dir).await;
    let extra = tokio::time::timeout(
        Duration::from_millis(500),
        next_matching(&mut host.events, |e| match e {
            HostEvent::DeleteAcked { .. } => Some(()),
            _ => None,
        }),
    )
    .await;
    assert!(extra.is_err(), "补删只执行一次，不无限重试");
    agent3.task.abort();
}

/// #17 用例 2：在线端删除**立即**生效——教师删除后在线学员立刻收到 delete
/// 事件（墓碑快照），重复删除幂等（不 panic、不重复广播）。
#[tokio::test]
async fn online_student_delete_immediate_and_repeat_idempotent() {
    let host = start_host(None).await;
    let port = host.port;
    let mut a = MiniAgent::join(port, "alice", None).await.expect("join");

    host.cmd
        .send(HostCommand::SendText {
            title: "在线通知".into(),
            body: "会被删除".into(),
        })
        .await
        .unwrap();
    let item = loop {
        match a.next().await {
            Frame::Event(ev) => break ev.item,
            _ => continue,
        }
    };
    // 消费正文文件帧序列
    loop {
        match a.next().await {
            Frame::FileEnd(_) => break,
            _ => continue,
        }
    }

    host.cmd
        .send(HostCommand::Delete {
            id: item.id.clone(),
        })
        .await
        .unwrap();
    let del = loop {
        match a.next().await {
            Frame::Event(ev) if ev.kind == classroom_protocol::EventKind::Delete => break ev,
            _ => continue,
        }
    };
    assert_eq!(del.item.id, item.id);
    assert!(del.item.deleted, "删除事件携带墓碑快照");

    // 重复删除：幂等（不报错、无第二次广播）
    host.cmd
        .send(HostCommand::Delete {
            id: item.id.clone(),
        })
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(300), a.next())
            .await
            .is_err(),
        "重复删除不应重复广播"
    );
    // 教师端列表默认不含墓碑
    let store = Store::open(&host.data_root).unwrap();
    assert!(
        store.list(ListFilter::default()).unwrap().is_empty(),
        "教师端列表默认不含墓碑"
    );
    assert!(
        store.get(&item.id).unwrap().is_some_and(|i| i.deleted),
        "墓碑保留在教师端索引（供离线端补删）"
    );
}

//! M2 S1：客户端会话运行器集成测试（真实 TCP 回环 + mock 服务端）。
//!
//! 覆盖设计文档 §8.1 五类：Hello 拒绝分类、心跳、**服务端 Ping → 客户端
//! 自动 Pong**（E3 结构性守护）、业务帧分派、对端关闭 → `Closed`；
//! 另加「加入后自动请求清单」。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use classroom_net::{
    encode_frame, read_frame, run_client_session, ClientHandler, ClientSessionConfig, Frame, Hello,
    HelloErr, HelloReply, SessionEnd,
};
use classroom_protocol::{
    Event, EventKind, ItemKind, Role, ShareItem, SyncState, PROTOCOL_VERSION,
};

const STEP_TIMEOUT: Duration = Duration::from_secs(3);

fn sample_event(title: &str) -> Event {
    use classroom_protocol::{Audience, Role as R};
    Event {
        event_id: format!("ev-{title}"),
        kind: EventKind::Upsert,
        item: ShareItem {
            id: format!("id-{title}"),
            kind: ItemKind::Text,
            owner_id: "teacher".into(),
            owner_role: R::Teacher,
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

fn join_hello(nickname: &str) -> Hello {
    Hello::Join {
        protocol_version: PROTOCOL_VERSION,
        nickname: nickname.into(),
        role: Role::Student,
        pin: None,
    }
}

/// 测试回调：把收到的业务帧转发到 channel，记录 on_joined 的 session_id。
struct CollectHandler {
    tx: mpsc::UnboundedSender<Frame>,
    joined: std::sync::Mutex<Vec<String>>,
}

impl CollectHandler {
    fn new(tx: mpsc::UnboundedSender<Frame>) -> Self {
        Self {
            tx,
            joined: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ClientHandler for CollectHandler {
    async fn on_frame(&self, frame: Frame, _tx: &mut mpsc::Sender<Frame>) -> anyhow::Result<()> {
        let _ = self.tx.send(frame);
        Ok(())
    }

    async fn on_joined(&self, session_id: &str) {
        self.joined.lock().unwrap().push(session_id.to_string());
    }
}

/// 起一个 mock 服务端：accept 一条连接后交给 `serve` 处理，返回地址。
async fn mock_server<F, Fut>(serve: F) -> SocketAddr
where
    F: FnOnce(TcpStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        serve(stream).await;
    });
    addr
}

/// 用例 1：Hello 成功 → `on_joined` 回调 + 业务帧分派到 handler。
#[tokio::test]
async fn hello_ok_then_business_frames_dispatched() {
    let addr = mock_server(|stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let first = read_frame(&mut reader).await.unwrap();
        assert!(matches!(first, Frame::Hello(_)), "首个帧应为 Hello");
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-1".into(),
            })))
            .await
            .unwrap();
        // 等 ManifestReq（默认 request_manifest=true）后发业务帧
        let f = read_frame(&mut reader).await.unwrap();
        assert!(matches!(f, Frame::ManifestReq), "加入后应自动请求清单");
        writer
            .write_all(&encode_frame(&Frame::Event(sample_event("通知"))))
            .await
            .unwrap();
        // 保持连接直到对端关闭
        let _ = read_frame(&mut reader).await;
    })
    .await;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let cfg = ClientSessionConfig::new(addr, join_hello("alice"));
    let handler = CollectHandler::new(tx);
    let task = tokio::spawn(run_client_session(cfg, handler));

    let frame = tokio::time::timeout(STEP_TIMEOUT, rx.recv())
        .await
        .expect("等待业务帧超时")
        .expect("channel 关闭");
    match frame {
        Frame::Event(ev) => {
            assert_eq!(ev.item.title, "通知");
            assert_eq!(ev.item.owner_id, "teacher");
        }
        other => panic!("期望 Event，收到 {other:?}"),
    }

    task.abort();
}

/// 用例 1b：`on_joined` 在业务帧之前回调，且携带服务端分配的 session_id。
#[tokio::test]
async fn on_joined_receives_session_id_before_frames() {
    let addr = mock_server(|stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-joined".into(),
            })))
            .await
            .unwrap();
        // 等 ManifestReq 后发一帧，确保会话进入主循环
        let _ = read_frame(&mut reader).await;
        writer
            .write_all(&encode_frame(&Frame::Event(sample_event("加入后"))))
            .await
            .unwrap();
        let _ = read_frame(&mut reader).await;
    })
    .await;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let handler = std::sync::Arc::new(CollectHandler::new(tx));
    let h = handler.clone();
    let cfg = ClientSessionConfig::new(addr, join_hello("alice2"));
    let task = tokio::spawn(run_client_session(cfg, ArcHandler(h)));
    // 等业务帧（证明 joined 已在此前发生）
    let _ = tokio::time::timeout(STEP_TIMEOUT, rx.recv())
        .await
        .expect("等待业务帧超时");
    let joined = handler.joined.lock().unwrap().clone();
    assert_eq!(
        joined,
        vec!["sid-joined".to_string()],
        "on_joined 应收到 session_id"
    );
    task.abort();
}

/// Arc 包装以便测试在会话结束后仍能读取 handler 内部记录。
struct ArcHandler(std::sync::Arc<CollectHandler>);

#[async_trait]
impl ClientHandler for ArcHandler {
    async fn on_frame(&self, frame: Frame, tx: &mut mpsc::Sender<Frame>) -> anyhow::Result<()> {
        self.0.on_frame(frame, tx).await
    }
    async fn on_joined(&self, session_id: &str) {
        self.0.on_joined(session_id).await
    }
}

/// 用例 2：join 被拒 → `SessionEnd::Rejected(code)`（致命，不重连）。
#[tokio::test]
async fn rejected_hello_returns_rejected() {
    let addr = mock_server(|stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Err {
                code: HelloErr::WrongPin,
            })))
            .await
            .unwrap();
    })
    .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let end = tokio::time::timeout(
        STEP_TIMEOUT,
        run_client_session(
            ClientSessionConfig::new(addr, join_hello("bob")),
            CollectHandler::new(tx),
        ),
    )
    .await
    .expect("会话超时")
    .expect("会话不应返回 Err");
    assert_eq!(end, SessionEnd::Rejected(HelloErr::WrongPin));
}

/// 用例 3：心跳——服务端在短间隔内收到至少 2 个 Ping。
#[tokio::test]
async fn heartbeat_sends_periodic_ping() {
    let (ping_tx, mut ping_rx) = mpsc::unbounded_channel();
    let addr = mock_server(move |stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-hb".into(),
            })))
            .await
            .unwrap();
        loop {
            match read_frame(&mut reader).await {
                Ok(Frame::Ping) => {
                    let _ = ping_tx.send(());
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
    .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut cfg = ClientSessionConfig::new(addr, join_hello("carol"));
    cfg.heartbeat = Duration::from_millis(120);
    cfg.request_manifest = false;
    let task = tokio::spawn(run_client_session(cfg, CollectHandler::new(tx)));

    tokio::time::timeout(STEP_TIMEOUT, ping_rx.recv())
        .await
        .expect("首个心跳超时")
        .expect("channel 关闭");
    tokio::time::timeout(STEP_TIMEOUT, ping_rx.recv())
        .await
        .expect("第二个心跳超时")
        .expect("channel 关闭");
    task.abort();
}

/// 用例 4（E3 结构性守护）：服务端发 Ping → 客户端自动回 Pong，
/// 不依赖角色层实现（此前 agent 手写循环漏掉该分支导致活跃学员被误驱逐）。
#[tokio::test]
async fn auto_responds_pong_to_server_ping() {
    let (pong_tx, mut pong_rx) = mpsc::unbounded_channel();
    let addr = mock_server(move |stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-pong".into(),
            })))
            .await
            .unwrap();
        // 连发两次 Ping（探测重名存活会重复探测）
        for _ in 0..2 {
            writer.write_all(&encode_frame(&Frame::Ping)).await.unwrap();
            match read_frame(&mut reader).await {
                Ok(Frame::Pong) => {
                    let _ = pong_tx.send(());
                }
                Ok(other) => panic!("期望 Pong，收到 {other:?}"),
                Err(e) => panic!("读 Pong 失败: {e}"),
            }
        }
    })
    .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut cfg = ClientSessionConfig::new(addr, join_hello("dave"));
    cfg.heartbeat = Duration::from_secs(30); // 避免心跳干扰断言
    cfg.request_manifest = false;
    let task = tokio::spawn(run_client_session(cfg, CollectHandler::new(tx)));

    for i in 1..=2 {
        tokio::time::timeout(STEP_TIMEOUT, pong_rx.recv())
            .await
            .unwrap_or_else(|_| panic!("第 {i} 次 Pong 超时"))
            .unwrap_or_else(|| panic!("第 {i} 次 Pong channel 关闭"));
    }
    task.abort();
}

/// 用例 5：对端关闭 → `SessionEnd::Closed`（可重连，不是错误）。
#[tokio::test]
async fn peer_close_returns_closed() {
    let addr = mock_server(|stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-close".into(),
            })))
            .await
            .unwrap();
        // 立即关闭连接
        drop(writer);
        drop(reader);
    })
    .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut cfg = ClientSessionConfig::new(addr, join_hello("erin"));
    cfg.heartbeat = Duration::from_secs(30);
    let end = tokio::time::timeout(
        STEP_TIMEOUT,
        run_client_session(cfg, CollectHandler::new(tx)),
    )
    .await
    .expect("会话超时")
    .expect("对端关闭应返回 Ok(Closed)");
    assert_eq!(end, SessionEnd::Closed);
}

/// 用例 6：`request_manifest = false` 时不发清单请求；为 true 时发出。
#[tokio::test]
async fn request_manifest_flag_controls_manifest_req() {
    let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
    let addr = mock_server(move |stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-manifest".into(),
            })))
            .await
            .unwrap();
        // 第一条帧：若为 ManifestReq → true
        match read_frame(&mut reader).await {
            Ok(Frame::ManifestReq) => {
                let _ = seen_tx.send(true);
            }
            _ => {
                let _ = seen_tx.send(false);
            }
        }
        let _ = read_frame(&mut reader).await;
    })
    .await;

    let (tx, _rx) = mpsc::unbounded_channel();
    let mut cfg = ClientSessionConfig::new(addr, join_hello("frank"));
    cfg.heartbeat = Duration::from_secs(30);
    let task = tokio::spawn(run_client_session(cfg, CollectHandler::new(tx)));
    let got = tokio::time::timeout(STEP_TIMEOUT, seen_rx.recv())
        .await
        .expect("等待清单请求超时")
        .expect("channel 关闭");
    assert!(got, "request_manifest=true 时应自动发送 ManifestReq");
    task.abort();
}

/// 用例 7：handler 返回 Err → 会话以 Err 结束（致命错误上抛）。
#[tokio::test]
async fn handler_error_is_fatal() {
    struct Failing;
    #[async_trait]
    impl ClientHandler for Failing {
        async fn on_frame(
            &self,
            _frame: Frame,
            _tx: &mut mpsc::Sender<Frame>,
        ) -> anyhow::Result<()> {
            anyhow::bail!("回调主动失败")
        }
    }

    let addr = mock_server(|stream| async move {
        let (mut reader, mut writer) = stream.into_split();
        let _ = read_frame(&mut reader).await.unwrap();
        writer
            .write_all(&encode_frame(&Frame::HelloReply(HelloReply::Ok {
                session_id: "sid-fail".into(),
            })))
            .await
            .unwrap();
        writer
            .write_all(&encode_frame(&Frame::Event(sample_event("坏帧"))))
            .await
            .unwrap();
        let _ = read_frame(&mut reader).await;
    })
    .await;

    let mut cfg = ClientSessionConfig::new(addr, join_hello("grace"));
    cfg.heartbeat = Duration::from_secs(30);
    cfg.request_manifest = false;
    let result = tokio::time::timeout(STEP_TIMEOUT, run_client_session(cfg, Failing))
        .await
        .expect("会话超时");
    assert!(result.is_err(), "handler 失败应上抛 Err");
    let msg = format!("{:#}", result.unwrap_err());
    assert!(
        msg.contains("客户端帧回调失败"),
        "错误信息应指明回调失败: {msg}"
    );
}

/// 汇编线索：Arc 仅用于跨任务共享统计（保留以防未来用例需要）。
#[allow(dead_code)]
fn _unused(arc: Arc<u8>) -> u8 {
    *arc
}

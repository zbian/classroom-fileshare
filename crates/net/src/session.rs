//! TCP 会话：accept → 每连接 spawn 会话，回调 [`SessionHandler`]。
//!
//! 协议层内置行为：收到 `Ping` 自动回 `Pong`；`FileHeader` 起始一条分块流，
//! 逐块转发给 handler，`FileEnd` 关闭流并由 handler 校验整体哈希后回 `FileAck`。
//! 服务端不处理的帧（HelloReply/FileAck/Pong/Manifest 系）记 debug 日志忽略，
//! Manifest 的业务语义由角色层（host/agent）在后续任务中经扩展回调处理。

use std::net::SocketAddr;
use std::sync::Arc;

use crate::frame::{
    encode_frame, read_frame, FileAck, FileChunk, FileHeader, Frame, Hello, HelloReply,
};
use anyhow::Result;
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

/// 会话回调：由角色层（host/agent）实现。
///
/// 接口修订 #4（lead 报备）：`on_hello` 增加连接发送通道参数（host 需在
/// join 成功时注册广播目标）；新增 `on_disconnect` / `on_manifest_req`
/// 默认方法（断线通知与重连对齐，均为空默认实现，不影响既有实现方）。
#[async_trait]
pub trait SessionHandler: Send + Sync {
    /// 处理加入请求，返回应答（PIN 校验 / 昵称查重 / 版本检查在实现方）；
    /// `tx` 为该连接的发送通道，实现方可保存用于后续推送。
    async fn on_hello(&self, hello: Hello, tx: &mut mpsc::Sender<Frame>) -> HelloReply;

    /// 处理同步事件；`tx` 可用于向对端回帧。
    async fn on_event(&self, ev: classroom_protocol::Event, tx: &mut mpsc::Sender<Frame>);

    /// 处理文件分块流：`chunks` 在 FileEnd 后关闭（recv 返回 None），
    /// 实现方累计字节并按 `expected_sha256` 校验，返回 [`FileAck`]。
    async fn on_file_chunk_stream(
        &self,
        header: FileHeader,
        chunks: mpsc::Receiver<FileChunk>,
        expected_sha256: String,
    ) -> FileAck;

    /// 连接断开（仅对 join 成功获得 session_id 的会话调用）。默认空实现。
    async fn on_disconnect(&self, _session_id: &str) {}

    /// 收到版本清单请求（重连对齐，PRD 7.5）；实现方应经 `tx` 回
    /// [`Frame::Manifest`]。默认空实现。
    async fn on_manifest_req(&self, _tx: &mut mpsc::Sender<Frame>) {}

    /// 收到客户端 Pong（join 成功的会话；PR #34 B 项存活探测依赖）。
    /// 默认空实现。
    async fn on_pong(&self, _session_id: &str) {}
}

/// TCP 服务端：bind/accept，每连接 spawn 一个会话任务。
pub struct Server {
    listener: TcpListener,
    handler: Arc<dyn SessionHandler>,
    shutdown: Arc<tokio::sync::Notify>,
    sessions: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

/// 服务停机句柄：`shutdown()` 通知 [`Server::run`] 退出并终止全部会话
/// （关闭已接受连接、释放监听端口）——接口扩展 #5⑤：host 优雅停止必需，
/// 仅 abort accept loop 会让既有会话继续存活而对端无感知。
#[derive(Clone)]
pub struct ServerHandle {
    shutdown: Arc<tokio::sync::Notify>,
}

impl ServerHandle {
    /// 请求停机（幂等；随后 `Server::run` 会退出并 abort 全部会话）。
    ///
    /// PR #34 C 项：用 notify_one 而非 notify_waiters——Notify 的单许可
    /// 语义保证「run 尚未进入 select 时先行调用」也不丢通知（waiters
    /// 不保存许可，存在丢通知窗口）。
    pub fn shutdown(&self) {
        self.shutdown.notify_one();
    }
}

impl Server {
    /// 绑定地址（测试用 `127.0.0.1:0` 随机端口）。
    pub async fn bind(addr: SocketAddr, handler: Arc<dyn SessionHandler>) -> Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self {
            listener,
            handler,
            shutdown: Arc::new(tokio::sync::Notify::new()),
            sessions: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// 实际监听地址（端口 0 时由内核分配）。
    pub fn local_addr(&self) -> SocketAddr {
        self.listener
            .local_addr()
            .expect("已绑定的 listener 必有本地地址")
    }

    /// 停机句柄（在 `run` 之前获取，spawn 后仍可调用）。
    pub fn handle(&self) -> ServerHandle {
        ServerHandle {
            shutdown: self.shutdown.clone(),
        }
    }

    /// accept 循环（通常 `tokio::spawn` 运行）；收到停机通知后退出，
    /// abort 全部会话任务并随返回释放监听端口。
    pub async fn run(self) -> Result<()> {
        loop {
            tokio::select! {
                _ = self.shutdown.notified() => break,
                accepted = self.listener.accept() => {
                    let (stream, peer) = accepted?;
                    tracing::debug!(%peer, "接入连接");
                    let handler = self.handler.clone();
                    let task = tokio::spawn(async move {
                        if let Err(e) = run_session(stream, handler).await {
                            tracing::debug!(%peer, error = %e, "会话结束");
                        }
                    });
                    self.sessions.lock().unwrap().push(task);
                }
            }
        }
        let tasks: Vec<_> = self.sessions.lock().unwrap().drain(..).collect();
        for task in tasks {
            task.abort();
        }
        tracing::debug!("服务端停机，全部会话已终止");
        Ok(())
    }
}

/// 单连接会话主循环：读帧分发 + 独立写任务（避免读写互锁）。
pub async fn run_session(stream: TcpStream, handler: Arc<dyn SessionHandler>) -> Result<()> {
    stream.set_nodelay(true)?;
    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Frame>(64);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let bytes = encode_frame(&frame);
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });

    // 当前活跃文件流的分块转发通道（FileEnd 后置 None 触发接收端校验）
    let mut file_tx: Option<mpsc::Sender<FileChunk>> = None;
    // join 成功的会话 id（断开时回调 on_disconnect）
    let mut joined_session: Option<String> = None;

    loop {
        let frame = match read_frame(&mut reader).await {
            Ok(f) => f,
            Err(e) => {
                // 对端关闭或坏帧：结束会话（详细原因在 debug 级，避免刷屏）
                tracing::debug!(error = %e, "读帧结束，关闭会话");
                break;
            }
        };
        match frame {
            Frame::Hello(hello) => {
                let mut reply_tx = tx.clone();
                let reply = handler.on_hello(hello, &mut reply_tx).await;
                if let HelloReply::Ok { session_id } = &reply {
                    joined_session = Some(session_id.clone());
                }
                if tx.send(Frame::HelloReply(reply)).await.is_err() {
                    break;
                }
            }
            Frame::Event(ev) => {
                let mut reply_tx = tx.clone();
                handler.on_event(ev, &mut reply_tx).await;
            }
            Frame::FileHeader(header) => {
                if file_tx.is_some() {
                    tracing::warn!("上一文件流未收到 FileEnd 即开始新文件，旧流被放弃");
                }
                let (chunk_tx, chunk_rx) = mpsc::channel::<FileChunk>(8);
                file_tx = Some(chunk_tx);
                let expected = header.sha256.clone();
                let stream_header = header.clone();
                let h = handler.clone();
                let ack_tx = tx.clone();
                tokio::spawn(async move {
                    let ack = h
                        .on_file_chunk_stream(stream_header, chunk_rx, expected)
                        .await;
                    let _ = ack_tx.send(Frame::FileAck(ack)).await;
                });
            }
            Frame::FileChunk(chunk) => {
                let item_id = chunk.item_id.clone();
                match &file_tx {
                    Some(ch) => {
                        if ch.send(chunk).await.is_err() {
                            tracing::warn!(%item_id, "分块通道已关闭，丢弃该块");
                        }
                    }
                    None => tracing::warn!(%item_id, "无活跃文件流，丢弃 FileChunk"),
                }
            }
            Frame::FileEnd(_) => {
                // drop 发送端 → handler 的 recv 返回 None → 校验哈希并回 FileAck
                file_tx = None;
            }
            Frame::Ping => {
                if tx.send(Frame::Pong).await.is_err() {
                    break;
                }
            }
            Frame::ManifestReq => {
                let mut reply_tx = tx.clone();
                handler.on_manifest_req(&mut reply_tx).await;
            }
            Frame::Pong => {
                if let Some(session_id) = &joined_session {
                    handler.on_pong(session_id).await;
                }
            }
            other => {
                tracing::debug!(frame = ?other, "服务端会话暂不处理该帧");
            }
        }
    }

    drop(tx);
    // 先回调断开：实现方（host）在回调里注销该连接并 drop 广播用 tx clone，
    // 之后 writer 的 rx 才会关闭；若放到 writer_task.await 之后会形成
    // 「session 等 writer ↔ 实现方等 on_disconnect」互等。
    if let Some(session_id) = joined_session {
        handler.on_disconnect(&session_id).await;
    }
    let _ = writer_task.await;
    Ok(())
}

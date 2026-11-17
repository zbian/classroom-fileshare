//! TCP 客户端会话运行器（M2 S1）。
//!
//! 设计与动机见 `docs/design/M2-session-identity-and-client-runner.md` §4：
//! 服务端 [`crate::session::run_session`] 与客户端此前各有一份帧分发实现，
//! 已发生真实漂移——host 的重名存活探测依赖客户端应答 `Ping`，而 agent
//! 手写循环漏了该分支，导致活跃学员被误判僵死驱逐（E3 / issue #37）。
//!
//! 本模块把**控制面**（建连、握手、心跳、`Ping → Pong` 自动应答、断线分类、
//! 可选清单请求）收敛到单点维护；**业务面**仍由 [`ClientHandler`] 分派
//! （与设计文档 §4.1 的精确表述一致：抽取只消除控制面漂移）。

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::frame::{encode_frame, read_frame, Frame, Hello, HelloErr, HelloReply};

/// 客户端会话回调：由角色层（agent）实现，把业务帧映射到自身状态机。
#[async_trait]
pub trait ClientHandler: Send + Sync {
    /// 收到服务端下发的业务帧（`Event` / `Manifest` / `FileHeader` /
    /// `FileChunk` / `FileEnd` / `FileAck`）——控制帧（`Ping`/`Pong`/
    /// `HelloReply`）由运行器内部处理，不会到达此处。
    ///
    /// 返回 `Err` 视为致命错误，会话立即结束并向上抛出（调用方决定是否重连）。
    async fn on_frame(&self, frame: Frame) -> Result<()>;

    /// 加入成功（在 [`Self::on_frame`] 首次回调之前调用，可据此通知上层
    /// 「已加入房间」/ 展示会话标识）。默认空实现。
    async fn on_joined(&self, _session_id: &str) {}
}

/// 客户端会话配置。
#[derive(Clone, Debug)]
pub struct ClientSessionConfig {
    /// 服务端地址。
    pub addr: SocketAddr,
    /// 加入请求（版本/昵称/角色/PIN）。
    pub hello: Hello,
    /// 心跳间隔（M1 约定 5s）。
    pub heartbeat: Duration,
    /// 建连超时。
    pub connect_timeout: Duration,
    /// 加入成功后是否自动请求版本清单（重连对齐，PRD 7.5）。
    pub request_manifest: bool,
}

impl ClientSessionConfig {
    /// 默认配置：心跳 5s、建连超时 5s、加入后请求清单。
    pub fn new(addr: SocketAddr, hello: Hello) -> Self {
        Self {
            addr,
            hello,
            heartbeat: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            request_manifest: true,
        }
    }
}

/// 会话结束原因（区分「可重连」与「致命」）。
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEnd {
    /// 对端关闭 / IO 断线：可重连（临时性）。
    Closed,
    /// join 被拒（错 PIN / 重名 / 昵称不可用 / 版本不匹配）：致命，不应重连。
    Rejected(HelloErr),
}

/// 运行一次客户端会话：建连 → Hello/HelloReply → 心跳 + 帧分派 → 结束。
///
/// 行为约定（与设计文档 §4.2 一一对应）：
/// - 建连失败/超时返回 `Err`（可重连由调用方处理）；
/// - 收到 `HelloReply::Err` 返回 `Ok(SessionEnd::Rejected(code))`（致命）；
/// - 收到 `Ping` **自动回 `Pong`**（E3 根因的结构性修复，不再依赖角色层记忆）；
/// - `Pong` 由运行器吞掉（活性数据留给 M2 的 Pong 读超时）；
/// - 对端关闭或读帧失败返回 `Ok(SessionEnd::Closed)`；
/// - [`ClientHandler::on_frame`] 返回 `Err` 时向上抛出（视为致命）。
pub async fn run_client_session<H: ClientHandler>(
    cfg: ClientSessionConfig,
    handler: H,
) -> Result<SessionEnd> {
    // ---- 建连（带超时） ----
    let stream = tokio::time::timeout(cfg.connect_timeout, TcpStream::connect(cfg.addr))
        .await
        .with_context(|| format!("连接 {} 超时（{:?}）", cfg.addr, cfg.connect_timeout))?
        .with_context(|| format!("连接 {} 失败", cfg.addr))?;
    stream.set_nodelay(true)?;
    tracing::debug!(addr = %cfg.addr, "客户端会话已建立");

    let (mut reader, mut writer) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Frame>(64);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            if writer.write_all(&encode_frame(&frame)).await.is_err() {
                break;
            }
        }
    });

    // ---- 握手 ----
    if tx.send(Frame::Hello(cfg.hello)).await.is_err() {
        let _ = writer_task.await;
        bail!("握手发送失败（写任务已退出）");
    }
    let reply = match read_frame(&mut reader).await {
        Ok(Frame::HelloReply(reply)) => reply,
        Ok(other) => {
            drop(tx);
            let _ = writer_task.await;
            bail!("握手期望 HelloReply，收到 {other:?}");
        }
        Err(e) => {
            drop(tx);
            let _ = writer_task.await;
            return Err(e.context("等待 HelloReply 失败"));
        }
    };
    let session_id = match reply {
        HelloReply::Ok { session_id } => session_id,
        HelloReply::Err { code } => {
            drop(tx);
            let _ = writer_task.await;
            tracing::info!(?code, "加入被拒（致命，不重连）");
            return Ok(SessionEnd::Rejected(code));
        }
    };
    tracing::info!(%session_id, "已加入房间");
    handler.on_joined(&session_id).await;

    // ---- 可选：请求版本清单（重连对齐第一阶段） ----
    if cfg.request_manifest && tx.send(Frame::ManifestReq).await.is_err() {
        let _ = writer_task.await;
        bail!("ManifestReq 发送失败（写任务已退出）");
    }

    // ---- 主循环：心跳 + 帧分派 ----
    let mut heartbeat = tokio::time::interval(cfg.heartbeat);
    heartbeat.tick().await; // interval 首个 tick 立即触发，先吃掉
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if tx.send(Frame::Ping).await.is_err() {
                    tracing::debug!("心跳发送失败（写任务已退出）");
                    break;
                }
            }
            read = read_frame(&mut reader) => {
                let frame = match read {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::debug!(error = %e, "读帧结束，关闭会话");
                        break;
                    }
                };
                match frame {
                    // 控制面：服务端探测活跃性，必须应答（E3）
                    Frame::Ping => {
                        if tx.send(Frame::Pong).await.is_err() {
                            break;
                        }
                    }
                    // 控制面：对端心跳应答（M2 的 Pong 读超时将从这里取活性数据）
                    Frame::Pong => {}
                    // 控制面异常帧（重复 HelloReply 等）：忽略
                    Frame::HelloReply(other) => {
                        tracing::debug!(reply = ?other, "会话中忽略 HelloReply");
                    }
                    // 业务面：交给角色层
                    other => {
                        if let Err(e) = handler.on_frame(other).await {
                            tracing::warn!(error = %e, "客户端回调失败，结束会话");
                            drop(tx);
                            let _ = writer_task.await;
                            return Err(e.context("客户端帧回调失败"));
                        }
                    }
                }
            }
        }
    }

    drop(tx);
    let _ = writer_task.await;
    Ok(SessionEnd::Closed)
}

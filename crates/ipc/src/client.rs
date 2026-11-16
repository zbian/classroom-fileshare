//! 客户端（GUI 进程侧）：连接命名管道、握手、请求往返（id 关联）与
//! 通知接收。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::{mpsc, oneshot};

use classroom_protocol::Role;

use crate::codec::{self, IpcMessage};
use crate::{pipe_name, GuiRequest, SvcNotify, SvcResult};

/// 已连接并通过握手的 IPC 客户端句柄（可克隆共享写端）。
///
/// 注：named pipe 无 owned split（tokio 仅提供共享锁 halves），写端经
/// tokio::sync::Mutex 序列化并发请求。
#[derive(Clone)]
pub struct IpcClient {
    writer: Arc<tokio::sync::Mutex<tokio::io::WriteHalf<NamedPipeClient>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<SvcResult>>>>,
    next_id: Arc<AtomicU64>,
}

/// 连接服务管道并完成握手（令牌 + IPC 协议版本，PRD 9.4）。
///
/// 接口修订 #6②（报备 lead）：增 `role` 参数以确定管道名。
/// 服务实例尚未创建（ERROR_FILE_NOT_FOUND）或繁忙（ERROR_PIPE_BUSY）时
/// 自旋重试（总时限约 5s），便于 GUI 与服务竞速启动。
pub async fn connect_ipc(
    role: Role,
    token: &str,
) -> anyhow::Result<(IpcClient, mpsc::Receiver<SvcNotify>)> {
    connect_named(&pipe_name(role), token).await
}

/// 指定管名的客户端连接（测试用唯一管名；生产契约 [`pipe_name`] 不变，
/// doc(hidden) 不入公开文档）。
#[doc(hidden)]
pub async fn connect_named(
    pipe: &str,
    token: &str,
) -> anyhow::Result<(IpcClient, mpsc::Receiver<SvcNotify>)> {
    let name = pipe.to_string();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let client = loop {
        match ClientOptions::new().open(&name) {
            Ok(c) => break c,
            Err(e)
                if matches!(e.raw_os_error(), Some(2) | Some(231))
                    && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => {
                return Err(anyhow!(e)).context(format!("连接 IPC 管道失败: {name}"));
            }
        }
    };

    // named pipe 无 into_split：用共享锁 halves（读任务独占读端，写端经 Mutex）
    let (read_half, write_half) = tokio::io::split(client);
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut writer = write_half;

    // 握手
    codec::write_msg(&mut writer, &codec::handshake_msg(token)).await?;
    match codec::read_msg(&mut reader).await? {
        IpcMessage::HandshakeOk => {}
        IpcMessage::HandshakeRejected { reason } => bail!("IPC 握手被拒: {reason}"),
        other => bail!("期望握手应答，收到 {other:?}"),
    }

    // 读任务：Reply 按 id 唤醒等待者；Notify 推给调用方
    let (notify_tx, notify_rx) = mpsc::channel::<SvcNotify>(64);
    let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<SvcResult>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let reader_pending = pending.clone();
    tokio::spawn(async move {
        loop {
            match codec::read_msg(&mut reader).await {
                Ok(IpcMessage::Reply { id, result }) => {
                    if let Some(waiter) = reader_pending.lock().unwrap().remove(&id) {
                        let _ = waiter.send(result);
                    }
                }
                Ok(IpcMessage::Notify(notify)) => {
                    if notify_tx.send(notify).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break, // 对端关闭
            }
        }
        // 连接断开：丢弃全部等待者（request 侧收到 RecvError）
        reader_pending.lock().unwrap().clear();
    });

    Ok((
        IpcClient {
            writer: Arc::new(tokio::sync::Mutex::new(writer)),
            pending,
            next_id: Arc::new(AtomicU64::new(1)),
        },
        notify_rx,
    ))
}

impl IpcClient {
    /// 发送请求并等待应答（id 关联；默认超时 10s）。
    pub async fn request(&self, req: GuiRequest) -> anyhow::Result<SvcResult> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        {
            let mut writer = self.writer.lock().await;
            // MutexGuard 需显式解引用为 &mut WriteHalf（AsyncWrite blanket impl）
            let write_result =
                codec::write_msg(&mut *writer, &IpcMessage::Request { id, req }).await;
            if let Err(e) = write_result {
                self.pending.lock().unwrap().remove(&id);
                return Err(e);
            }
        }
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => bail!("IPC 连接已关闭"),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                bail!("IPC 请求超时");
            }
        }
    }
}

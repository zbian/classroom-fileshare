//! 服务端（Session 0 后台服务侧）：监听命名管道、握手校验、请求分发、
//! 通知扇出。
//!
//! 并发模型：accept 循环持续创建管道实例（Windows named pipe 多实例），
//! 每个连接独立任务（握手 → select 读请求/收通知）；服务核心的通知经
//! `notify_rx` 进入扇出任务，复制到全部已握手连接。

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::mpsc;

use classroom_protocol::Role;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

use crate::codec::{self, IpcMessage};
use crate::{GuiRequest, SvcNotify, SvcResult, IPC_PROTOCOL_VERSION};

type Handler = Arc<dyn Fn(GuiRequest) -> SvcResult + Send + Sync>;
type Conns = Arc<Mutex<Vec<mpsc::Sender<SvcNotify>>>>;

/// 运行 IPC 服务：监听 [`crate::pipe_name(role)`]，直到出错返回。
///
/// 接口修订 #6②（报备 lead）：计划的 `notify_tx: mpsc::Sender<SvcNotify>`
/// 改为 `notify_rx: mpsc::Receiver<SvcNotify>`（Sender 方向无法实现
/// 「服务核心 → GUI 推送」）；增 `role` 决定监听管道名。
///
/// `handler` 在连接任务内**同步调用**，必须快速返回（T-13 组装时用
/// try_send + 本地 store 读取；阻塞操作应自行 spawn）。
pub async fn serve_ipc(
    role: Role,
    token: &str,
    handler: Handler,
    notify_rx: mpsc::Receiver<SvcNotify>,
) -> anyhow::Result<()> {
    serve_named(&crate::pipe_name(role), token, handler, notify_rx).await
}

/// 指定管名的服务运行器（测试用唯一管名防并行实例串扰；
/// 生产契约 [`crate::pipe_name`] 不变，故 doc(hidden) 不入公开文档）。
#[doc(hidden)]
pub async fn serve_named(
    pipe: &str,
    token: &str,
    handler: Handler,
    mut notify_rx: mpsc::Receiver<SvcNotify>,
) -> anyhow::Result<()> {
    let name = pipe.to_string();
    let conns: Conns = Arc::new(Mutex::new(Vec::new()));

    // 通知扇出：服务核心 → 全部已握手连接（发送失败即剔除该连接）。
    // 注意锁内不 await：先 drain 收集（drop guard），再逐个异步发送，最后
    // 把仍存活的合并回去（间隔期新注册的连接不受影响）。
    let fanout_conns = conns.clone();
    tokio::spawn(async move {
        while let Some(notify) = notify_rx.recv().await {
            let senders: Vec<_> = fanout_conns.lock().unwrap().drain(..).collect();
            let mut alive = Vec::new();
            for tx in senders {
                if tx.send(notify.clone()).await.is_ok() {
                    alive.push(tx);
                }
            }
            fanout_conns.lock().unwrap().extend(alive);
        }
    });

    tracing::info!(pipe = %name, "IPC 服务监听中");
    loop {
        // 每轮创建新实例供下一个客户端连接（当前实例交给连接任务）
        // 管道 ACL：服务进程（LocalSystem）默认 DACL 只放行 SYSTEM/管理员，
        // 用户会话的 GUI 会被「对路径的访问被拒绝」挡在握手之前（真机实测）。
        // 显式 SA：SYSTEM/管理员全权 + Authenticated Users 读写；鉴权仍靠令牌握手。
        // SD 在 create 后立即释放，避免非 Send 指针跨越 await。
        let created = {
            let sddl: Vec<u16> = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;AU)\0"
                .encode_utf16()
                .collect();
            let mut sd = PSECURITY_DESCRIPTOR::default();
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    PCWSTR(sddl.as_ptr()),
                    1,
                    &mut sd,
                    None,
                )
                .context("构造 IPC 管道安全描述符失败")?;
            }
            let mut sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: windows::Win32::Foundation::FALSE,
            };
            let created = unsafe {
                ServerOptions::new()
                    .create_with_security_attributes_raw(&name, &mut sa as *mut _ as *mut _)
            };
            unsafe {
                let _ = LocalFree(Some(HLOCAL(sd.0)));
            }
            created
        };
        let server = created.with_context(|| format!("创建管道实例失败: {name}"))?;
        server.connect().await.context("等待 IPC 客户端失败")?;
        let handler = handler.clone();
        let conns = conns.clone();
        let token = token.to_string();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(server, &token, handler, conns).await {
                tracing::debug!(error = %e, "IPC 连接结束");
            }
        });
    }
}

async fn handle_conn(
    pipe: NamedPipeServer,
    token: &str,
    handler: Handler,
    conns: Conns,
) -> anyhow::Result<()> {
    // named pipe 无 into_split：共享锁 halves（单任务 select 内交替读写，
    // 两半不会真正并发使用，共享锁无争用开销）
    let (read_half, write_half) = tokio::io::split(pipe);
    let mut reader = tokio::io::BufReader::new(read_half);
    let mut writer = write_half;

    // ---- 握手：令牌 + IPC 协议版本（PRD 4.5/9.4）----
    let first = codec::read_msg(&mut reader).await?;
    let IpcMessage::Handshake {
        protocol_version,
        token: client_token,
    } = first
    else {
        return Err(anyhow!("首条 IPC 消息必须是握手"));
    };
    if protocol_version != IPC_PROTOCOL_VERSION {
        codec::write_msg(
            &mut writer,
            &IpcMessage::HandshakeRejected {
                reason: format!(
                    "IPC 协议版本不匹配: 服务端 {IPC_PROTOCOL_VERSION}，客户端 {protocol_version}"
                ),
            },
        )
        .await
        .ok();
        return Err(anyhow!("IPC 协议版本不匹配"));
    }
    if client_token != token {
        codec::write_msg(
            &mut writer,
            &IpcMessage::HandshakeRejected {
                reason: "IPC 令牌错误".into(),
            },
        )
        .await
        .ok();
        tracing::warn!("IPC 握手被拒：令牌错误");
        return Err(anyhow!("IPC 令牌错误"));
    }
    codec::write_msg(&mut writer, &IpcMessage::HandshakeOk).await?;
    tracing::debug!("IPC 客户端握手通过");

    // ---- 注册通知通道 ----
    let (notify_tx, mut notify_rx) = mpsc::channel::<SvcNotify>(64);
    conns.lock().unwrap().push(notify_tx);
    let (reply_tx, mut reply_rx) = mpsc::channel::<IpcMessage>(16);

    // ---- 双工循环：读请求 / 收通知 / 内部应答 ----
    let reason = loop {
        tokio::select! {
            msg = codec::read_msg(&mut reader) => {
                match msg {
                    Ok(IpcMessage::Request { id, req }) => {
                        // handler 同步调用（约定快速返回），结果异步写回
                        let result = handler(req);
                        if reply_tx.send(IpcMessage::Reply { id, result }).await.is_err() {
                            break "写侧已关闭";
                        }
                    }
                    Ok(IpcMessage::Handshake { .. }) => {} // 重复握手忽略
                    Ok(_) => {}
                    Err(_) => break "客户端断开",
                }
            }
            Some(notify) = notify_rx.recv() => {
                if codec::write_msg(&mut writer, &IpcMessage::Notify(notify)).await.is_err() {
                    break "通知写入失败";
                }
            }
            Some(reply) = reply_rx.recv() => {
                if codec::write_msg(&mut writer, &reply).await.is_err() {
                    break "应答写入失败";
                }
            }
            else => break "通道全部关闭",
        }
    };
    tracing::debug!(reason, "IPC 会话结束");
    // 断开后由扇出任务在下一次 send 失败时剔除本连接
    Ok(())
}

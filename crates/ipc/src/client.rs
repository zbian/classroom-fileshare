//! 客户端（GUI 进程侧）：连接命名管道、握手、请求往返与通知接收。
//! 实现于增量 B（签名先落地）。

use tokio::sync::mpsc;

use classroom_protocol::Role;

use crate::{GuiRequest, SvcNotify, SvcResult};

/// 已连接的 IPC 客户端句柄。
pub struct IpcClient;

/// 连接服务管道并完成握手（令牌 + IPC 协议版本，PRD 9.4）。
///
/// 接口修订 #6②（报备 lead）：增 `role` 参数以确定管道名。
pub async fn connect_ipc(
    _role: Role,
    _token: &str,
) -> anyhow::Result<(IpcClient, mpsc::Receiver<SvcNotify>)> {
    unimplemented!()
}

impl IpcClient {
    /// 发送请求并等待应答（id 关联；超时 10s）。
    pub async fn request(&self, _req: GuiRequest) -> anyhow::Result<SvcResult> {
        unimplemented!()
    }
}

//! 服务端：监听命名管道、握手校验、请求分发与通知扇出。
//! 实现于增量 B（签名先落地）。

use std::sync::Arc;

use tokio::sync::mpsc;

use classroom_protocol::Role;

use crate::{GuiRequest, SvcNotify, SvcResult};

/// 运行 IPC 服务：监听 [`crate::pipe_name(role)`] 命名管道。
///
/// 接口修订 #6②（报备 lead）：计划签名的 `notify_tx: mpsc::Sender<SvcNotify>`
/// 方向上无法实现「服务通知推送到 GUI」——Sender 不能接收；改为
/// `notify_rx: mpsc::Receiver<SvcNotify>`：服务核心持发送端、本函数消费并
/// 推给已连接的 GUI，与 `connect_ipc` 返回的 Receiver 对称。
/// 另增 `role` 参数决定监听管道名（原签名无从得知管道名）。
pub async fn serve_ipc(
    _role: Role,
    _token: &str,
    _handler: Arc<dyn Fn(GuiRequest) -> SvcResult + Send + Sync>,
    _notify_rx: mpsc::Receiver<SvcNotify>,
) -> anyhow::Result<()> {
    unimplemented!()
}

//! 行分帧 JSON 编解码：每条消息一行（本地管道，无需二进制紧凑性）。

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

use crate::{GuiRequest, SvcNotify, SvcResult, IPC_PROTOCOL_VERSION};

/// 线协议消息（内部）。
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub(crate) enum IpcMessage {
    /// 客户端握手：令牌 + IPC 协议版本。
    Handshake {
        protocol_version: u32,
        token: String,
    },
    /// 握手通过。
    HandshakeOk,
    /// 握手被拒（令牌错误或版本不匹配），随后服务端关闭连接。
    HandshakeRejected { reason: String },
    /// 请求（id 关联）。
    Request { id: u64, req: GuiRequest },
    /// 应答（与 Request 同 id）。
    Reply { id: u64, result: SvcResult },
    /// 服务端异步通知。
    Notify(SvcNotify),
}

/// 编码为单行 JSON（含结尾换行）。
pub(crate) fn encode(msg: &IpcMessage) -> anyhow::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(msg).context("IPC 消息序列化失败")?;
    line.push(b'\n');
    Ok(line)
}

/// 写一条消息。
pub(crate) async fn write_msg<W: AsyncWriteExt + Unpin>(
    w: &mut W,
    msg: &IpcMessage,
) -> anyhow::Result<()> {
    w.write_all(&encode(msg)?).await?;
    Ok(())
}

/// 读一条消息（读到 EOF 返回 Err）。
pub(crate) async fn read_msg<R: AsyncBufReadExt + Unpin>(r: &mut R) -> anyhow::Result<IpcMessage> {
    let mut line = Vec::new();
    let n = r.read_until(b'\n', &mut line).await?;
    if n == 0 {
        anyhow::bail!("IPC 对端已关闭");
    }
    serde_json::from_slice(&line).context("IPC 消息解码失败")
}

/// 构造握手消息。
pub(crate) fn handshake_msg(token: &str) -> IpcMessage {
    IpcMessage::Handshake {
        protocol_version: IPC_PROTOCOL_VERSION,
        token: token.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GuiReply, ItemUpdatedPayload};
    use classroom_net::Beacon;
    use classroom_protocol::{Audience, ItemKind, Role, ShareItem, SyncState};

    fn sample_item(title: &str) -> ShareItem {
        ShareItem {
            id: format!("id-{title}"),
            kind: ItemKind::Text,
            owner_id: "teacher".into(),
            owner_role: Role::Teacher,
            audience: Audience::All,
            title: title.into(),
            size: 1,
            sha256: None,
            rel_path: None,
            created_at: 1,
            updated_at: 1,
            version: 1,
            deleted: false,
            sync_state: SyncState::Synced,
        }
    }

    #[tokio::test]
    async fn message_roundtrip_all_kinds() {
        let msgs = vec![
            handshake_msg("tok"),
            IpcMessage::HandshakeOk,
            IpcMessage::HandshakeRejected {
                reason: "令牌错误".into(),
            },
            IpcMessage::Request {
                id: 7,
                req: GuiRequest::SendText {
                    title: "通知".into(),
                    body: "内容".into(),
                },
            },
            IpcMessage::Reply {
                id: 7,
                result: Ok(GuiReply::Items(vec![sample_item("通知")])),
            },
            IpcMessage::Reply {
                id: 8,
                result: Err("失败原因".into()),
            },
            IpcMessage::Notify(SvcNotify::ShareReceived(sample_item("课件"))),
            IpcMessage::Notify(SvcNotify::ItemUpdated(ItemUpdatedPayload {
                item: sample_item("作业"),
                overwritten: true,
                old_version: Some(1),
            })),
            IpcMessage::Notify(SvcNotify::Discovered(vec![Beacon {
                protocol_version: 1,
                room: "XK-204".into(),
                tcp_port: 47821,
                pin_required: true,
                host_name: "PC".into(),
            }])),
        ];
        for m in msgs {
            let bytes = encode(&m).unwrap();
            assert!(bytes.ends_with(b"\n"), "行分帧必须以换行结尾");
            // 经内存管道往返（&[u8] 实现 AsyncRead，BufReader 供 read_until）
            let mut reader = bytes.as_slice();
            let back = read_msg(&mut tokio::io::BufReader::new(&mut reader))
                .await
                .unwrap();
            assert_eq!(back, m);
        }
    }
}

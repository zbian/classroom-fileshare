//! 行分帧 JSON 编解码：每条消息一行（本地管道，无需二进制紧凑性）。

use anyhow::Context;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

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

/// 单行上限：1MB（消息均为小载荷，对齐 net 8MB 帧上限先例；
/// 超限断连防内存放大——PR #38 发现 3）。
pub const MAX_LINE: usize = 1024 * 1024;

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

/// 读一条消息（读到 EOF 返回 Err；行超过 [`MAX_LINE`] 断连拒绝）。
pub(crate) async fn read_msg<R: AsyncBufReadExt + Unpin>(r: &mut R) -> anyhow::Result<IpcMessage> {
    let mut line = Vec::new();
    // take 限制单行读取量：超过上限即拒绝（无换行的超长流不会无限积累）
    let n = r
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)
        .await?;
    if n == 0 {
        anyhow::bail!("IPC 对端已关闭");
    }
    if line.len() > MAX_LINE {
        anyhow::bail!("IPC 行超过 {MAX_LINE} 字节上限，断开（防内存放大）");
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
    // M2 #103：Warning 契约用例使用
    use crate::WarningKind;
    use crate::{GuiReply, ItemUpdatedPayload, ServiceState, StatusSnapshot};
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
            retry_count: 0,
        }
    }

    #[tokio::test]
    async fn oversized_line_rejected() {
        // 1MB + 1 无换行超长行：读取必须拒绝而非无限积累（PR #38 发现 3）
        let mut payload = vec![b'a'; MAX_LINE + 1];
        payload.push(b'\n');
        let mut reader = payload.as_slice();
        let err = read_msg(&mut tokio::io::BufReader::new(&mut reader))
            .await
            .expect_err("超长行必须被拒绝");
        assert!(
            format!("{err:#}").contains("上限"),
            "应报行上限错误: {err:#}"
        );
        // 恰好 1MB（含换行）不触发上限（会因 JSON 解码失败而 Err，但非上限错误）
        let mut edge = vec![b'a'; MAX_LINE - 1];
        edge.push(b'\n');
        let mut reader = edge.as_slice();
        let result = read_msg(&mut tokio::io::BufReader::new(&mut reader)).await;
        assert!(result.is_err(), "非法 JSON 应解码失败");
        assert!(!format!("{:#}", result.unwrap_err()).contains("上限"));
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
                id: 6,
                result: Ok(GuiReply::Status(StatusSnapshot {
                    service_state: ServiceState::Running,
                    room: "XK-204".into(),
                    online_count: 2,
                    nickname: "alice".into(),
                    role: Role::Student,
                    pending_count: 0,
                })),
            },
            IpcMessage::Reply {
                id: 8,
                result: Err("失败原因".into()),
            },
            IpcMessage::Reply {
                id: 9,
                result: Ok(GuiReply::FilePath(r"C:\Data\repo\teacher\课件.pdf".into())),
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
            // M2 #114：删除进度契约（GUI 显示「N/M 台已删除」）
            IpcMessage::Notify(SvcNotify::DeleteProgress {
                item_id: "item-1".into(),
                done: 1,
                total: 2,
            }), // M2 #103：同步异常告警契约（GUI SyncIssuesBanner 接入）
            IpcMessage::Notify(SvcNotify::Warning {
                kind: WarningKind::RetryExhausted,
                item_id: "item-1".into(),
                detail: "目标文件被占用，重试 5 次仍未成功".into(),
            }),
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

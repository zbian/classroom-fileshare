//! Task 7（ipc）回环集成测试：命名管道请求往返、错 token 拒绝、
//! notify 双向推送。Teacher/Student 管名分用避免并行串扰。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use classroom_ipc::{
    connect_ipc, serve_ipc, GuiReply, GuiRequest, IpcClient, ItemUpdatedPayload, ListFilter,
    ServiceState, StatusSnapshot, SvcNotify, SvcResult,
};
use classroom_protocol::{Audience, ItemKind, Role, ShareItem, SyncState};

fn sample_item(title: &str) -> ShareItem {
    ShareItem {
        id: format!("id-{title}"),
        kind: ItemKind::Text,
        owner_id: "teacher".into(),
        owner_role: Role::Teacher,
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
    }
}

fn stub_handler(req: GuiRequest) -> SvcResult {
    match req {
        GuiRequest::List(_) => Ok(GuiReply::Items(vec![sample_item("通知")])),
        GuiRequest::Status => Ok(GuiReply::Status(StatusSnapshot {
            service_state: ServiceState::Running,
            room: "XK-204".into(),
            online_count: 2,
            nickname: String::new(),
            role: Role::Teacher,
            pending_count: 0,
        })),
        GuiRequest::SendText { title, .. } => {
            if title.trim().is_empty() {
                Err("标题不能为空".into())
            } else {
                Ok(GuiReply::Ok)
            }
        }
        _ => Ok(GuiReply::Ok),
    }
}

async fn spawn_server(role: Role, token: &str) -> mpsc::Sender<SvcNotify> {
    let (tx, rx) = mpsc::channel(16);
    let handler: Arc<dyn Fn(GuiRequest) -> SvcResult + Send + Sync> = Arc::new(stub_handler);
    let token = token.to_string();
    tokio::spawn(async move { serve_ipc(role, &token, handler, rx).await });
    tx
}

#[tokio::test]
async fn request_roundtrip_and_notify_push() {
    let role = Role::Teacher;
    let token = "token-abc-0000000000000000000000000000";
    let notify_tx = spawn_server(role.clone(), token).await;

    // 连接（内置等待服务实例就绪：FILE_NOT_FOUND/PIPE_BUSY 重试）
    let mut last_err = None;
    for _ in 0..100 {
        match connect_ipc(role.clone(), token).await {
            Ok((client, notify_rx)) => {
                return assertions(client, notify_rx, &notify_tx, role, token).await;
            }
            Err(e) => last_err = Some(e),
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("连接 IPC 服务失败: {last_err:?}");
}

#[allow(clippy::too_many_arguments)]
async fn assertions(
    client: IpcClient,
    mut notify_rx: mpsc::Receiver<SvcNotify>,
    notify_tx: &mpsc::Sender<SvcNotify>,
    role: Role,
    token: &str,
) {
    // 1) 请求往返（多种应答载荷）
    match client
        .request(GuiRequest::List(ListFilter::default()))
        .await
    {
        Ok(Ok(GuiReply::Items(items))) => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].title, "通知");
        }
        other => panic!("List 往返异常: {other:?}"),
    }
    match client.request(GuiRequest::Status).await {
        Ok(Ok(GuiReply::Status(snap))) => {
            assert_eq!(snap.room, "XK-204");
            assert_eq!(snap.online_count, 2);
            assert_eq!(snap.service_state, ServiceState::Running);
            assert_eq!(snap.pending_count, 0, "M1 恒 0");
        }
        other => panic!("Status 往返异常: {other:?}"),
    }
    assert!(matches!(
        client
            .request(GuiRequest::SendText {
                title: "通知".into(),
                body: "内容".into(),
            })
            .await,
        Ok(Ok(GuiReply::Ok))
    ));
    // 修订 #4 接口可达：ConfigureRoom / SetNickname / PauseTips
    assert!(matches!(
        client
            .request(GuiRequest::ConfigureRoom {
                room: "XK-204".into(),
                pin: Some("9527".into()),
                allow_discovery: true
            })
            .await,
        Ok(Ok(GuiReply::Ok))
    ));
    assert!(matches!(
        client
            .request(GuiRequest::SetNickname {
                nickname: "alice".into(),
                pin: Some("9527".into())
            })
            .await,
        Ok(Ok(GuiReply::Ok))
    ));
    // 错误字符串原样往返
    match client
        .request(GuiRequest::SendText {
            title: "  ".into(),
            body: "x".into(),
        })
        .await
    {
        Ok(Err(msg)) => assert!(msg.contains("标题")),
        other => panic!("错误往返异常: {other:?}"),
    }

    // 2) notify 双向推送（服务核心 → 管道 → GUI Receiver）
    notify_tx
        .send(SvcNotify::ShareReceived(sample_item("课件")))
        .await
        .unwrap();
    let got = tokio::time::timeout(Duration::from_secs(5), notify_rx.recv())
        .await
        .expect("notify 超时")
        .expect("notify 通道存活");
    assert!(matches!(got, SvcNotify::ShareReceived(ref i) if i.title == "课件"));

    notify_tx
        .send(SvcNotify::ItemUpdated(ItemUpdatedPayload {
            item: sample_item("作业"),
            overwritten: true,
            old_version: Some(1),
        }))
        .await
        .unwrap();
    match tokio::time::timeout(Duration::from_secs(5), notify_rx.recv())
        .await
        .expect("notify 超时 2")
    {
        Some(SvcNotify::ItemUpdated(p)) => {
            assert!(p.overwritten);
            assert_eq!(p.old_version, Some(1));
        }
        other => panic!("ItemUpdated 异常: {other:?}"),
    }

    // 3) 多实例连接：第二客户端并发请求 + 扇出可达（并入本测试，
    //    避免与其他测试并行占用同名管道造成串扰）
    let (client2, mut notify_rx2) = connect_ipc(role, token).await.expect("第二连接（多实例）");
    let (r1, r2) = tokio::join!(
        client.request(GuiRequest::Status),
        client2.request(GuiRequest::Status)
    );
    assert!(r1.is_ok() && r2.is_ok(), "两连接并发请求应都成功");
    notify_tx
        .send(SvcNotify::StatusChanged("在线 3".into()))
        .await
        .unwrap();
    let got2 = tokio::time::timeout(Duration::from_secs(5), notify_rx2.recv())
        .await
        .expect("第二连接扇出超时");
    assert!(matches!(got2, Some(SvcNotify::StatusChanged(_))));
}

#[tokio::test]
async fn wrong_token_rejected_at_handshake() {
    let role = Role::Student; // 与 roundtrip 测试分用管名
    let _notify_tx = spawn_server(role.clone(), "correct-token-00000000000000000000").await;
    let mut last = None;
    for _ in 0..100 {
        match connect_ipc(role.clone(), "wrong-token-000000000000000000000000").await {
            // 正确令牌在服务启动后应可用；错误令牌必须握手被拒
            Err(e) => {
                let msg = format!("{e:#}");
                assert!(
                    msg.contains("拒") || msg.contains("token") || msg.contains("令牌"),
                    "应报握手被拒，实际: {msg}"
                );
                return;
            }
            Ok(_) => {
                // 服务尚未就绪时 connect 可能因实例未创建而重试中——这里拿到
                // 连接说明握手已过，与预期矛盾
                last = Some("意外握手成功".to_string());
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("wrong token 测试未达成: {last:?}");
}

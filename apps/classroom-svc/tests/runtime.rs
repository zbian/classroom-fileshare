//! T-13 增量 2 集成测试：student 模式核心组装 → IPC 请求往返 → 干净停机。
//! （teacher 模式的 IPC 行为由单元测试覆盖：通道分发 + 本地读取。）

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

use classroom_ipc::{connect_ipc, load_or_create_token, GuiReply, GuiRequest, ListFilter};
use classroom_protocol::Role;
use classroom_svc::config::{SvcConfig, SvcMode};
use classroom_svc::runtime::run_core;

#[tokio::test]
async fn student_core_serves_ipc_and_stops_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    // 先创建令牌（run_core 内部 load 到同值）
    let token = load_or_create_token(&root).unwrap();

    let cfg = SvcConfig {
        mode: SvcMode::Student,
        nickname: "alice".into(),
        // 指向不可达端口：agent 进入无害重连循环；manual 模式不绑定发现端口，
        // 避免与并行测试争用 UDP 47820
        manual_addr: Some("127.0.0.1:1".parse().unwrap()),
        ..Default::default()
    };
    let shutdown = Arc::new(Notify::new());
    let task = {
        let cfg = cfg.clone();
        let root = root.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move { run_core(&cfg, &root, shutdown).await })
    };

    // 轮询等待 IPC 就绪后做请求往返
    let mut served = false;
    for _ in 0..100 {
        if let Ok((client, _notify_rx)) = connect_ipc(Role::Student, &token).await {
            match client
                .request(GuiRequest::List(ListFilter::default()))
                .await
            {
                Ok(Ok(GuiReply::Items(v))) => assert!(v.is_empty(), "新仓库应为空"),
                other => panic!("List 往返异常: {other:?}"),
            }
            match client
                .request(GuiRequest::SendText {
                    title: "t".into(),
                    body: "b".into(),
                })
                .await
            {
                // M2 #18（PRD 6.8）：student 未连接教师机时**不再报错**——
                // 命令入离线待发队列（持久化 data_root/pending），重连后
                // 自动补发；IPC 返回 Ok（断网可提交，不静默丢弃）
                Ok(Ok(GuiReply::Ok)) => {}
                other => panic!("student 断网提交应入队并返回 Ok: {other:?}"),
            }
            served = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(served, "IPC 服务未就绪");

    shutdown.notify_waiters();
    let outcome = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("停机后 run_core 应在 3 秒内退出")
        .expect("核心任务应存活");
    assert!(outcome.is_ok(), "run_core 应返回 Ok: {outcome:?}");
}

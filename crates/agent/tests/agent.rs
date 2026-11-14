//! Task 6（agent）集成测试：host + agent 真实 TCP 回环——接收落盘、
//! 删除清理、断线重连对齐、join 拒绝上抛。

use std::time::Duration;

use tokio::sync::mpsc;

use classroom_agent::{AgentConfig, AgentEvent};
use classroom_host::{HostCommand, HostConfig};
use classroom_store::{ListFilter, Store};

const TIMEOUT: Duration = Duration::from_secs(10);

/// 跳过噪音（Synced/Disconnected 等），取第一个匹配的事件。
async fn next_matching<T>(
    rx: &mut mpsc::Receiver<AgentEvent>,
    f: impl Fn(AgentEvent) -> Option<T>,
) -> T {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    loop {
        let ev = tokio::time::timeout_at(deadline, rx.recv())
            .await
            .expect("等待目标事件超时")
            .expect("agent 已退出");
        if let Some(t) = f(ev) {
            return t;
        }
    }
}

/// 预占一个空闲回环端口（bind 0 后立即释放，供 host 固定端口复用）。
fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

struct TestHost {
    cmd: mpsc::Sender<HostCommand>,
    port: u16,
    /// None = 复用外部数据目录（此时目录生命期由调用方持有）。
    #[allow(dead_code)]
    guard: Option<tempfile::TempDir>,
    root: std::path::PathBuf,
}

/// 在固定端口起 host；`data_root` 传入时复用既有数据目录（模拟教师机重启恢复）。
async fn start_host_at(port: u16) -> TestHost {
    start_host_at_inner(port, None, None).await
}

async fn start_host_at_pin(port: u16, pin: Option<String>) -> TestHost {
    start_host_at_inner(port, pin, None).await
}

async fn start_host_at_inner(
    port: u16,
    pin: Option<String>,
    data_root: Option<std::path::PathBuf>,
) -> TestHost {
    let (root, guard) = match data_root {
        Some(r) => (r, None),
        None => {
            let g = tempfile::tempdir().unwrap();
            (g.path().to_path_buf(), Some(g))
        }
    };
    let cfg = HostConfig {
        room_code: "XK-204".into(),
        pin,
        data_root: root.clone(),
        tcp_port: port,
    };
    let (cmd_tx, cmd_rx) = mpsc::channel(16);
    let (ev_tx, mut ev_rx) = mpsc::channel(128);
    tokio::spawn(classroom_host::run_host(cfg, cmd_rx, ev_tx));
    let status = match tokio::time::timeout(TIMEOUT, ev_rx.recv())
        .await
        .expect("host 启动超时")
        .expect("host 任务存活")
    {
        classroom_host::HostEvent::Started(s) => s,
        other => panic!("expected Started, got {other:?}"),
    };
    TestHost {
        cmd: cmd_tx,
        port: status.tcp_port,
        guard,
        root,
    }
}

async fn start_agent(
    port: u16,
    nickname: &str,
    pin: Option<String>,
    root: &std::path::Path,
) -> mpsc::Receiver<AgentEvent> {
    let (tx, rx) = mpsc::channel(128);
    let cfg = AgentConfig {
        nickname: nickname.into(),
        pin,
        manual_addr: Some(format!("127.0.0.1:{port}").parse().unwrap()),
        data_root: root.to_path_buf(),
    };
    tokio::spawn(classroom_agent::run_agent(cfg, tx));
    rx
}

#[tokio::test]
async fn text_received_persisted_and_readable() {
    let host = start_host_at_pin(free_port(), None).await;
    let agent_dir = tempfile::tempdir().unwrap();
    let mut rx = start_agent(host.port, "alice", None, agent_dir.path()).await;

    // join 成功
    next_matching(&mut rx, |e| match e {
        AgentEvent::Joined(_) => Some(()),
        _ => None,
    })
    .await;

    // 教师广播文本
    host.cmd
        .send(HostCommand::SendText {
            title: "通知".into(),
            body: "明天带教材".into(),
        })
        .await
        .unwrap();

    let item = next_matching(&mut rx, |e| match e {
        AgentEvent::Received(i) if i.title == "通知" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item.owner_id, "teacher");
    assert_eq!(item.version, 1);

    // 索引与正文实体都在学员端本地，且内容一致（教师 id 权威）
    let store = Store::open(agent_dir.path()).unwrap();
    let got = store.get(&item.id).unwrap().expect("索引存在");
    assert_eq!(got.title, "通知");
    let body = store.file_body(&got).unwrap();
    assert_eq!(std::fs::read_to_string(&body).unwrap(), "明天带教材");
}

#[tokio::test]
async fn file_received_chunks_verified_and_readable() {
    let host = start_host_at(free_port()).await;
    let agent_dir = tempfile::tempdir().unwrap();
    let mut rx = start_agent(host.port, "alice", None, agent_dir.path()).await;
    next_matching(&mut rx, |e| {
        matches!(e, AgentEvent::Joined(_)).then_some(())
    })
    .await;

    let payload: Vec<u8> = (0..2_621_440u32).map(|i| (i % 251) as u8).collect();
    let src = tempfile::tempdir().unwrap();
    let file_path = src.path().join("课件.bin");
    std::fs::write(&file_path, &payload).unwrap();

    host.cmd
        .send(HostCommand::SendFiles {
            paths: vec![file_path],
        })
        .await
        .unwrap();

    let item = next_matching(&mut rx, |e| match e {
        AgentEvent::Received(i) if i.title == "课件.bin" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item.size, payload.len() as u64);

    let store = Store::open(agent_dir.path()).unwrap();
    let body = store.file_body(&item).unwrap();
    assert_eq!(std::fs::read(&body).unwrap(), payload, "分块重组后实体一致");
}

#[tokio::test]
async fn delete_removes_index_and_body() {
    let host = start_host_at_pin(free_port(), None).await;
    let agent_dir = tempfile::tempdir().unwrap();
    let mut rx = start_agent(host.port, "alice", None, agent_dir.path()).await;
    next_matching(&mut rx, |e| {
        matches!(e, AgentEvent::Joined(_)).then_some(())
    })
    .await;

    host.cmd
        .send(HostCommand::SendText {
            title: "临时通知".into(),
            body: "马上删".into(),
        })
        .await
        .unwrap();
    let item = next_matching(&mut rx, |e| match e {
        AgentEvent::Received(i) if i.title == "临时通知" => Some(i),
        _ => None,
    })
    .await;

    host.cmd
        .send(HostCommand::Delete {
            id: item.id.clone(),
        })
        .await
        .unwrap();

    let removed_id = next_matching(&mut rx, |e| match e {
        AgentEvent::Removed(id) => Some(id),
        _ => None,
    })
    .await;
    assert_eq!(removed_id, item.id);

    let store = Store::open(agent_dir.path()).unwrap();
    let gone = store.get(&item.id).unwrap().expect("墓碑仍在索引");
    assert!(gone.deleted, "delete 事件必须落地为墓碑");
    // 默认列表不可见；实体已清
    assert!(store.list(ListFilter::default()).unwrap().is_empty());
    assert!(
        !store.file_body(&gone).unwrap().exists(),
        "实体文件应被删除"
    );
}

#[tokio::test]
async fn reconnect_catches_up_missed_upsert() {
    let port = free_port();
    let host1 = start_host_at(port).await;
    let agent_dir = tempfile::tempdir().unwrap();
    let mut rx = start_agent(port, "alice", None, agent_dir.path()).await;
    next_matching(&mut rx, |e| {
        matches!(e, AgentEvent::Joined(_)).then_some(())
    })
    .await;

    // 第一阶段：收到 A
    host1
        .cmd
        .send(HostCommand::SendText {
            title: "A".into(),
            body: "第一条".into(),
        })
        .await
        .unwrap();
    next_matching(&mut rx, |e| match e {
        AgentEvent::Received(i) if i.title == "A" => Some(i),
        _ => None,
    })
    .await;

    // 教师机停止 → agent 感知断线
    host1.cmd.send(HostCommand::Shutdown).await.unwrap();
    next_matching(&mut rx, |e| match e {
        AgentEvent::Disconnected => Some(()),
        _ => None,
    })
    .await;
    // 同端口同数据根重启教师机（数据保留），agent 不在线时教师广播 B
    let host2 = start_host_at_inner(port, None, Some(host1.root.clone())).await;
    host2
        .cmd
        .send(HostCommand::SendText {
            title: "B".into(),
            body: "第二条".into(),
        })
        .await
        .unwrap();

    // agent 自动重连：第二次 Joined → 清单对齐 → 收到错过的 B
    next_matching(&mut rx, |e| {
        matches!(e, AgentEvent::Joined(_)).then_some(())
    })
    .await;
    let item_b = next_matching(&mut rx, |e| match e {
        AgentEvent::Received(i) if i.title == "B" => Some(i),
        _ => None,
    })
    .await;
    assert_eq!(item_b.owner_id, "teacher");

    // 断线期间错过的条目与之前的 A 都在本地索引
    let store = Store::open(agent_dir.path()).unwrap();
    let titles: Vec<String> = store
        .list(ListFilter::default())
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, vec!["B".to_string(), "A".to_string()]);
}

#[tokio::test]
async fn join_rejection_is_fatal() {
    let host = start_host_at_pin(free_port(), Some("9527".into())).await;
    let agent_dir = tempfile::tempdir().unwrap();
    let (tx, _rx) = mpsc::channel(16);
    let cfg = AgentConfig {
        nickname: "alice".into(),
        pin: Some("0000".into()),
        manual_addr: Some(format!("127.0.0.1:{}", host.port).parse().unwrap()),
        data_root: agent_dir.path().to_path_buf(),
    };
    let handle = tokio::spawn(classroom_agent::run_agent(cfg, tx));
    let result = tokio::time::timeout(TIMEOUT, handle)
        .await
        .expect("应快速失败")
        .unwrap();
    assert!(result.is_err(), "错 PIN 必须以致命错误退出而非死循环重试");
}

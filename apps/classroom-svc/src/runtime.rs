//! 核心组装（T-7 接线草图落地）：按 [`SvcConfig`] 启动 host/agent 核心、
//! 桥接事件为 [`SvcNotify`]、运行 IPC 服务，直至停机信号。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, Notify};

use classroom_agent::{run_agent, AgentConfig, AgentEvent};
use classroom_host::{run_host, HostCommand, HostConfig, HostEvent};
use classroom_ipc::{
    serve_ipc, GuiReply, GuiRequest, ItemUpdatedPayload, ListFilter, ServiceState, StatusSnapshot,
    SvcNotify, SvcResult,
};
use classroom_protocol::{Role, ShareItem};
use classroom_store::Store;

use crate::config::{SvcConfig, SvcMode};

/// 核心 + IPC 事件统一视图（桥接输入）。
#[derive(Debug)]
pub enum CoreEvent {
    /// 教师核心事件。
    Host(HostEvent),
    /// 学员核心事件。
    Agent(AgentEvent),
}

/// 服务状态快照（Status 应答；桥任务更新；修订 #6④ 结构化字段）。
pub struct StatusBoard {
    snapshot: Mutex<StatusSnapshot>,
}

impl StatusBoard {
    /// 初始快照：Running、空房间、按角色与昵称。
    pub fn new(role: Role, nickname: String) -> Self {
        Self {
            snapshot: Mutex::new(StatusSnapshot {
                service_state: ServiceState::Running,
                room: String::new(),
                online_count: 0,
                nickname,
                role,
                pending_count: 0, // M1 恒 0（待发队列 M2 接入）
            }),
        }
    }

    /// 当前快照。
    pub fn snapshot(&self) -> StatusSnapshot {
        self.snapshot.lock().unwrap().clone()
    }

    fn update(&self, f: impl FnOnce(&mut StatusSnapshot)) {
        f(&mut self.snapshot.lock().unwrap());
    }
}

/// 运行服务核心直至 `shutdown` 被通知。
///
/// - teacher：`run_host`（HostCommand 通道供 IPC handler 驱动）
/// - student：`run_agent`（致命错误记录后驻留等待，避免 SCM 重启风暴）
/// - 通用：启动即清理 `*.part` 残留（PRD 9.2）、令牌、IPC 服务
pub async fn run_core(
    cfg: &SvcConfig,
    data_root: &Path,
    shutdown: Arc<Notify>,
) -> anyhow::Result<()> {
    let swept = crate::maintenance::sweep_part_files(&data_root.join("repo"));
    if swept > 0 {
        tracing::info!(removed = swept, "已清理残留 .part（PRD 9.2）");
    }

    // E7 令牌分发：服务模式限权 LocalSystem + 主登录用户（GUI 会话可读）；
    // user_sid 缺失时告警明示（GUI 不可用属配置缺失），补装/登录后由
    // SessionChange 自愈路径重授
    let token = classroom_ipc::load_or_create_token_for(data_root, cfg.user_sid.as_deref())?;
    if cfg.user_sid.is_none() {
        tracing::warn!(
            "svc.json 缺少 user_sid：服务模式令牌仅 LocalSystem 可读，GUI 将无法连接；\
             请升级安装或等待登录会话自愈（SessionChange）"
        );
    }
    // handler 专用 store 连接（与核心连接独立；短查询，SQLite 并发安全）
    let handler_store: Arc<Mutex<Store>> = Arc::new(Mutex::new(Store::open(data_root)?));
    let (notify_tx, notify_rx) = mpsc::channel::<SvcNotify>(128);
    let (core_event_tx, mut core_event_rx) = mpsc::channel::<CoreEvent>(128);
    let role = match cfg.mode {
        SvcMode::Teacher => Role::Teacher,
        SvcMode::Student => Role::Student,
    };
    let status = Arc::new(StatusBoard::new(role.clone(), cfg.nickname.clone()));
    let mut host_cmd: Option<mpsc::Sender<HostCommand>> = None;
    let mut agent_task: Option<tokio::task::JoinHandle<()>> = None;
    match cfg.mode {
        SvcMode::Teacher => {
            let (cmd_tx, cmd_rx) = mpsc::channel::<HostCommand>(16);
            let (ev_tx, mut ev_rx) = mpsc::channel::<HostEvent>(128);
            let host_cfg = HostConfig {
                room_code: cfg.room_code.clone(),
                pin: cfg.pin.clone(),
                data_root: data_root.to_path_buf(),
                tcp_port: cfg.tcp_port,
            };
            tokio::spawn(async move {
                if let Err(e) = run_host(host_cfg, cmd_rx, ev_tx).await {
                    tracing::error!(error = %e, "host 核心异常退出");
                }
            });
            let forward = core_event_tx.clone();
            tokio::spawn(async move {
                while let Some(ev) = ev_rx.recv().await {
                    if forward.send(CoreEvent::Host(ev)).await.is_err() {
                        break;
                    }
                }
            });
            host_cmd = Some(cmd_tx);
        }
        SvcMode::Student => {
            let (ev_tx, mut ev_rx) = mpsc::channel::<AgentEvent>(128);
            let agent_cfg = AgentConfig {
                nickname: cfg.nickname.clone(),
                pin: cfg.pin.clone(),
                manual_addr: cfg.manual_addr,
                // E9：自动发现房间过滤——svc.json room_code 为空 → None
                // （连首个发现，单教室 1 步入班）；非空 → 精确匹配（多教室）
                room_filter: cfg.room_filter(),
                data_root: data_root.to_path_buf(),
            };
            agent_task = Some(tokio::spawn(async move {
                // 致命错误（join 被拒等）记录后返回：服务保持运行等待人工处理，
                // 避免 SCM failure 策略造成的重启风暴
                if let Err(e) = run_agent(agent_cfg, ev_tx).await {
                    tracing::error!(error = %e, "agent 核心致命错误，驻留等待人工处理");
                }
            }));
            let forward = core_event_tx.clone();
            tokio::spawn(async move {
                while let Some(ev) = ev_rx.recv().await {
                    if forward.send(CoreEvent::Agent(ev)).await.is_err() {
                        break;
                    }
                }
            });
        }
    }

    // 事件桥：CoreEvent → SvcNotify（tips/列表刷新/状态快照更新）
    let bridge_notify = notify_tx.clone();
    let bridge_status = status.clone();
    tokio::spawn(async move {
        while let Some(ev) = core_event_rx.recv().await {
            for notify in map_notify(&ev) {
                if bridge_notify.send(notify).await.is_err() {
                    return;
                }
            }
            apply_status(&bridge_status, &ev);
        }
    });

    // IPC 服务
    let handler = make_ipc_handler(host_cmd.clone(), handler_store, status.clone());
    let ipc_token = token.clone();
    tokio::spawn(async move {
        if let Err(e) = serve_ipc(role, &ipc_token, handler, notify_rx).await {
            tracing::error!(error = %e, "IPC 服务异常退出");
        }
    });
    tracing::info!(
        mode = ?cfg.mode,
        data_root = %data_root.display(),
        "服务核心就绪"
    );

    shutdown.notified().await;
    tracing::info!("收到停机信号，核心退出");
    if let Some(cmd) = &host_cmd {
        let _ = cmd.send(HostCommand::Shutdown).await;
    }
    if let Some(task) = &agent_task {
        task.abort();
    }
    Ok(())
}

/// IPC 请求处理器（服务核心的 GUI 入口；同步、必须快速返回）。
///
/// - `List` / `OpenItem`：直读本地 store（OpenItem 返回路径，由 GUI 在
///   用户会话打开——Session 0 无法为用户启动进程）
/// - `SendText` / `SendFiles` / `DeleteItem`：teacher 经 HostCommand
///   try_send（student 回传/删除在 M2 提供）
/// - `SetNickname` / `ConfigureRoom`：M1 由安装脚本（svc.json）决定，
///   运行期修改在 M2 提供
/// - `PauseTips` / `ShutdownGuiOnly`：GUI 本地行为，服务仅确认
pub fn make_ipc_handler(
    host_cmd: Option<mpsc::Sender<HostCommand>>,
    store: Arc<Mutex<Store>>,
    status: Arc<StatusBoard>,
) -> Arc<dyn Fn(GuiRequest) -> SvcResult + Send + Sync> {
    Arc::new(move |req| match req {
        GuiRequest::List(filter) => store
            .lock()
            .unwrap()
            .list(filter)
            .map(GuiReply::Items)
            .map_err(|e| e.to_string()),
        GuiRequest::OpenItem { id } => {
            let guard = store.lock().unwrap();
            let item = guard
                .get(&id)
                .map_err(|e| e.to_string())?
                .filter(|i| !i.deleted)
                .ok_or_else(|| "条目不存在或已删除".to_string())?;
            let path: PathBuf = guard.file_body(&item).map_err(|e| e.to_string())?;
            if !path.exists() {
                return Err("文件实体缺失".into());
            }
            Ok(GuiReply::FilePath(path))
        }
        GuiRequest::SendText { title, body } => dispatch_teacher(
            &host_cmd,
            HostCommand::SendText { title, body },
            "学员回传在 M2 提供",
        ),
        GuiRequest::SendFiles { paths } => dispatch_teacher(
            &host_cmd,
            HostCommand::SendFiles { paths },
            "学员回传在 M2 提供",
        ),
        GuiRequest::DeleteItem { id } => {
            dispatch_teacher(&host_cmd, HostCommand::Delete { id }, "学员删除在 M2 提供")
        }
        GuiRequest::SetNickname { .. } => {
            Err("M1 昵称由安装脚本配置（svc.json），运行期修改在 M2 提供".into())
        }
        GuiRequest::ConfigureRoom { .. } => {
            Err("M1 房间由安装脚本配置（svc.json），运行期修改在 M2 提供".into())
        }
        GuiRequest::Status => Ok(GuiReply::Status(status.snapshot())),
        GuiRequest::Submissions => {
            // M2 #94：教师端「学生提交」分组视图。仅 teacher 模式有 host
            // 数据面（student 模式的 store 是学员端本地库，不含任何他人提交）。
            let guard = store.lock().unwrap();
            let items = guard
                .list(ListFilter {
                    audience: Some(classroom_protocol::Audience::Teacher),
                    include_deleted: false,
                    ..Default::default()
                })
                .map_err(|e| e.to_string())?;
            Ok(GuiReply::Submissions(group_submissions(&items)))
        }
        GuiRequest::PauseTips(_) | GuiRequest::ShutdownGuiOnly => Ok(GuiReply::Ok),
    })
}

/// 按 `owner_id` 聚合学员提交（纯函数，便于单测）：
/// 计数 + 最近 `updated_at`；输出按最新时间倒序（教师端左树默认顺序）。
///
/// 隔离说明：仅对传入条目聚合——调用方以 `audience=Teacher` 过滤，
/// 教师自己的分享（`audience=All`）不会进入此视图。
pub(crate) fn group_submissions(
    items: &[classroom_protocol::ShareItem],
) -> Vec<classroom_ipc::SubmissionGroup> {
    let mut groups: std::collections::HashMap<&str, (u32, i64)> = std::collections::HashMap::new();
    for item in items.iter().filter(|i| !i.deleted) {
        let entry = groups.entry(item.owner_id.as_str()).or_insert((0, 0));
        entry.0 += 1;
        entry.1 = entry.1.max(item.updated_at);
    }
    let mut out: Vec<classroom_ipc::SubmissionGroup> = groups
        .into_iter()
        .map(
            |(owner_id, (count, latest_at))| classroom_ipc::SubmissionGroup {
                owner_id: owner_id.to_string(),
                count,
                latest_at,
            },
        )
        .collect();
    out.sort_by(|a, b| {
        b.latest_at
            .cmp(&a.latest_at)
            .then_with(|| a.owner_id.cmp(&b.owner_id))
    });
    out
}

fn dispatch_teacher(
    host_cmd: &Option<mpsc::Sender<HostCommand>>,
    cmd: HostCommand,
    student_reject: &str,
) -> SvcResult {
    match host_cmd {
        Some(tx) => tx
            .try_send(cmd)
            .map(|_| GuiReply::Ok)
            .map_err(|e| format!("教师核心不可用: {e}")),
        None => Err(student_reject.into()),
    }
}

/// CoreEvent → SvcNotify 映射（一条事件可产生多条通知：tips + 列表刷新）。
/// 覆盖判定由 version 推断（>1 视为覆盖，供 E3 提示）。
fn map_notify(ev: &CoreEvent) -> Vec<SvcNotify> {
    let upsert = |item: &ShareItem| -> Vec<SvcNotify> {
        let overwritten = item.version > 1;
        vec![
            SvcNotify::ShareReceived(item.clone()),
            SvcNotify::ItemUpdated(ItemUpdatedPayload {
                item: item.clone(),
                overwritten,
                old_version: if overwritten {
                    Some(item.version - 1)
                } else {
                    None
                },
            }),
        ]
    };
    match ev {
        CoreEvent::Host(HostEvent::ReceivedUpsert(item))
        | CoreEvent::Agent(AgentEvent::Received(item)) => upsert(item),
        CoreEvent::Host(HostEvent::StudentJoined(n)) => {
            vec![SvcNotify::StatusChanged(format!("学员加入：{n}"))]
        }
        CoreEvent::Host(HostEvent::StudentLeft(n)) => {
            vec![SvcNotify::StatusChanged(format!("学员离线：{n}"))]
        }
        CoreEvent::Agent(AgentEvent::Removed(id)) => vec![SvcNotify::ItemRemoved(id.clone())],
        CoreEvent::Agent(AgentEvent::Discovered(beacons)) => {
            vec![SvcNotify::Discovered(beacons.clone())]
        }
        CoreEvent::Agent(AgentEvent::Synced) => {
            vec![SvcNotify::StatusChanged("已与教师机对齐".into())]
        }
        // 评审随包项 3：EX-08「教师机离线只读缓存」信号通路（T-10 前端
        // 消费 StatusChanged 展示离线横幅与待发提示）
        CoreEvent::Agent(AgentEvent::Disconnected) => {
            vec![SvcNotify::StatusChanged(
                "与教师机断开，本地为只读缓存，恢复后自动补发".into(),
            )]
        }
        _ => Vec::new(),
    }
}

/// CoreEvent → 状态快照更新（房间/在线数；修订 #6④ 结构化字段）。
fn apply_status(board: &StatusBoard, ev: &CoreEvent) {
    match ev {
        CoreEvent::Host(HostEvent::Started(s)) | CoreEvent::Host(HostEvent::Status(s)) => {
            board.update(|snap| {
                snap.room = s.room_code.clone();
                snap.online_count = s.online.len() as u32;
            });
        }
        CoreEvent::Agent(AgentEvent::Joined(room)) => {
            board.update(|snap| snap.room = room.clone());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use classroom_ipc::ListFilter;
    use classroom_protocol::{Audience, ItemKind, Role, SyncState};

    fn item(title: &str, version: u64) -> ShareItem {
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
            version,
            deleted: false,
            sync_state: SyncState::Synced,
        }
    }

    #[test]
    fn map_notify_upsert_inference_and_removal() {
        // 新建：ShareReceived + ItemUpdated(未覆盖)
        let n = map_notify(&CoreEvent::Host(HostEvent::ReceivedUpsert(item("课件", 1))));
        assert_eq!(n.len(), 2);
        assert!(
            matches!(&n[1], SvcNotify::ItemUpdated(p) if !p.overwritten && p.old_version.is_none())
        );

        // 覆盖（version 2）：overwritten + old_version=1（E3 提示）
        let n = map_notify(&CoreEvent::Agent(AgentEvent::Received(item("作业", 2))));
        assert!(
            matches!(&n[1], SvcNotify::ItemUpdated(p) if p.overwritten && p.old_version == Some(1))
        );

        // 删除与发现
        assert!(matches!(
            map_notify(&CoreEvent::Agent(AgentEvent::Removed("x".into()))).as_slice(),
            [SvcNotify::ItemRemoved(id)] if id == "x"
        ));
        // 评审随包项 3：Disconnected → StatusChanged（EX-08 信号通路）
        assert!(matches!(
            map_notify(&CoreEvent::Agent(AgentEvent::Disconnected)).as_slice(),
            [SvcNotify::StatusChanged(_)]
        ));
        assert!(matches!(
            map_notify(&CoreEvent::Agent(AgentEvent::Discovered(vec![]))).as_slice(),
            [SvcNotify::Discovered(_)]
        ));
    }

    #[test]
    fn apply_status_updates_room_and_online_count() {
        let board = StatusBoard::new(Role::Teacher, String::new());
        assert_eq!(board.snapshot().room, "");
        assert_eq!(board.snapshot().service_state, ServiceState::Running);

        apply_status(
            &board,
            &CoreEvent::Host(HostEvent::Started(classroom_host::HostStatus {
                room_code: "XK-204".into(),
                tcp_port: 47821,
                online: vec![],
            })),
        );
        let snap = board.snapshot();
        assert_eq!(snap.room, "XK-204");
        assert_eq!(snap.online_count, 0);

        // 学员端：Joined 更新房间描述
        let board = StatusBoard::new(Role::Student, "alice".into());
        apply_status(
            &board,
            &CoreEvent::Agent(AgentEvent::Joined("tcp://1.2.3.4".into())),
        );
        let snap = board.snapshot();
        assert_eq!(snap.room, "tcp://1.2.3.4");
        assert_eq!(snap.nickname, "alice");
        assert_eq!(snap.role, Role::Student);
        assert_eq!(snap.pending_count, 0, "M1 恒 0");
    }

    #[test]
    fn handler_teacher_dispatch_and_local_reads() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let created = store
            .put_text("teacher", Role::Teacher, Audience::All, "通知", "内容", 1)
            .unwrap();
        let item = match created {
            classroom_store::PutOutcome::Created(i) => i,
            _ => panic!(),
        };
        let store = Arc::new(Mutex::new(store));
        let status = Arc::new(StatusBoard::new(Role::Teacher, String::new()));
        let (cmd_tx, mut cmd_rx) = mpsc::channel(4);
        let handler = make_ipc_handler(Some(cmd_tx), store, status);

        // List 直读本地索引
        match handler(GuiRequest::List(ListFilter::default())) {
            Ok(GuiReply::Items(v)) => assert_eq!(v.len(), 1),
            other => panic!("{other:?}"),
        }
        // OpenItem 返回存在路径
        match handler(GuiRequest::OpenItem {
            id: item.id.clone(),
        }) {
            Ok(GuiReply::FilePath(p)) => assert!(p.exists()),
            other => panic!("{other:?}"),
        }
        // SendText 经通道到 host 核心
        assert!(matches!(
            handler(GuiRequest::SendText {
                title: "t".into(),
                body: "b".into()
            }),
            Ok(GuiReply::Ok)
        ));
        match cmd_rx.try_recv() {
            Ok(HostCommand::SendText { title, .. }) => assert_eq!(title, "t"),
            other => panic!("通道应收到 SendText: {other:?}"),
        }
        // Status 返回结构化快照（修订 #6④）
        match handler(GuiRequest::Status) {
            Ok(GuiReply::Status(snap)) => {
                assert_eq!(snap.service_state, ServiceState::Running);
                assert_eq!(snap.role, Role::Teacher);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn handler_student_rejects_send_and_runtime_config() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
        let handler = make_ipc_handler(
            None,
            store,
            Arc::new(StatusBoard::new(Role::Student, String::new())),
        );
        match handler(GuiRequest::SendText {
            title: "t".into(),
            body: "b".into(),
        }) {
            Err(msg) => assert!(msg.contains("M2"), "学员回传 M1 明确拒绝: {msg}"),
            other => panic!("{other:?}"),
        }
        assert!(handler(GuiRequest::ConfigureRoom {
            room: "X".into(),
            pin: None,
            allow_discovery: true
        })
        .is_err());
    }
    /// M2 #94：学员提交分组的纯函数——按 owner 计数 + 最新时间，
    /// 按最新时间倒序；墓碑条目不计。
    #[test]
    fn group_submissions_aggregates_by_owner() {
        use classroom_protocol::{Audience, ShareItem};
        fn item(owner: &str, id: &str, at: i64, deleted: bool) -> ShareItem {
            ShareItem {
                id: id.into(),
                kind: ItemKind::Text,
                owner_id: owner.into(),
                owner_role: Role::Student,
                audience: Audience::Teacher,
                title: format!("t-{id}"),
                size: 1,
                sha256: None,
                rel_path: None,
                created_at: at,
                updated_at: at,
                version: 1,
                deleted,
                sync_state: SyncState::Synced,
            }
        }
        let items = vec![
            item("alice", "a1", 100, false),
            item("alice", "a2", 300, false),
            item("alice", "a3", 900, true), // 墓碑不计
            item("bob", "b1", 200, false),
        ];
        let groups = group_submissions(&items);
        assert_eq!(groups.len(), 2, "按 owner 分组: {groups:?}");
        // 最新时间倒序：alice(300) 在 bob(200) 之前
        assert_eq!(groups[0].owner_id, "alice");
        assert_eq!(groups[0].count, 2, "墓碑不计入 count");
        assert_eq!(groups[0].latest_at, 300);
        assert_eq!(groups[1].owner_id, "bob");
        assert_eq!(groups[1].count, 1);

        // 空输入
        assert!(group_submissions(&[]).is_empty());
    }

    /// M2 #94：教师端 List 契约不变（`GuiRequest::List` 仍原样转发 filter），
    /// Submissions 仅聚合 `audience=Teacher` 的条目（教师自己的分享不进入）。
    #[test]
    fn submissions_view_excludes_teacher_own_items() {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Mutex::new(Store::open(dir.path()).unwrap()));
        {
            let guard = store.lock().unwrap();
            // 教师自己的分享（audience=All）
            guard
                .put_text("teacher", Role::Teacher, Audience::All, "通知", "正文", 1)
                .unwrap();
            // 学员提交（audience=Teacher）
            guard
                .put_text(
                    "alice",
                    Role::Student,
                    Audience::Teacher,
                    "作业",
                    "学生正文",
                    2,
                )
                .unwrap();
        }
        let handler = make_ipc_handler(
            None,
            store,
            Arc::new(StatusBoard::new(Role::Teacher, String::new())),
        );
        match handler(GuiRequest::Submissions) {
            Ok(GuiReply::Submissions(groups)) => {
                assert_eq!(groups.len(), 1, "只有学员提交进入分组: {groups:?}");
                assert_eq!(groups[0].owner_id, "alice");
                assert_eq!(groups[0].count, 1);
            }
            other => panic!("期望 Submissions，实际 {other:?}"),
        }
        // 现有 List 契约不变：空 filter 仍返回全部条目（含教师分享）
        match handler(GuiRequest::List(ListFilter::default())) {
            Ok(GuiReply::Items(items)) => assert_eq!(items.len(), 2),
            other => panic!("期望 Items，实际 {other:?}"),
        }
    }
}

//! 教师端：房间、准入、昵称查重与广播（PRD 3.3、5.x、FR-T-01）。
//!
//! 教师机即局域网唯一服务端（单 Host 多 Client）：教师自身 owner_id 固定
//! [`TEACHER_OWNER`] = `teacher`；学员 join 经版本/PIN/昵称查重；教师
//! upsert 入 store 后广播 `audience=All` 事件（文件随 FileHeader/Chunk/FileEnd
//! 推送）；学员上线/重连可请求 Manifest 版本清单对齐（PRD 7.5）。

mod hub;
mod room;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;

use classroom_net::{spawn_beacon, Beacon, Server, SessionHandler};
use classroom_protocol::PROTOCOL_VERSION;
use classroom_store::Store;

use hub::Hub;
pub use hub::TEACHER_OWNER;
use room::Admitter;

/// 教师端运行配置。
#[derive(Debug)]
pub struct HostConfig {
    /// 房间码（如 XK-204）。
    pub room_code: String,
    /// 房间 PIN；None 表示免 PIN 加入。
    pub pin: Option<String>,
    /// 数据根目录（Store 打开 `repo/` 与 `index.db`）。
    pub data_root: PathBuf,
    /// TCP 监听端口；0 = 自动分配，指定端口被占用时自动更换并上报（EX-11）。
    pub tcp_port: u16,
}

/// 在线学员信息。
#[derive(Debug, Clone)]
pub struct StudentInfo {
    /// 昵称（教师端分组主键）。
    pub nickname: String,
    /// 会话标识。
    pub session_id: String,
}

/// 房间当前状态。
#[derive(Debug, Clone)]
pub struct HostStatus {
    /// 房间码。
    pub room_code: String,
    /// 实际监听端口。
    pub tcp_port: u16,
    /// 在线学员列表。
    pub online: Vec<StudentInfo>,
}

/// 教师端对外通知事件。
#[derive(Debug)]
pub enum HostEvent {
    /// 服务就绪（含实际端口）。
    Started(HostStatus),
    /// 学员加入。
    StudentJoined(String),
    /// 学员离线。
    StudentLeft(String),
    /// 条目新增/覆盖（教师自己 put 或学员提交）。
    ReceivedUpsert(classroom_protocol::ShareItem),
    /// 在线列表变化。
    Status(HostStatus),
}

/// 教师操作入口（GUI/IPC → host；接口扩展 #4：计划无操作通道，测试与
/// T-7 ipc 集成必需）。
#[derive(Debug)]
pub enum HostCommand {
    /// 广播一条文本（FR-T-02）。
    SendText {
        /// 标题（同名覆盖键）。
        title: String,
        /// 正文。
        body: String,
    },
    /// 广播一个或多个文件（FR-T-03）。
    SendFiles {
        /// 本地文件路径列表。
        paths: Vec<PathBuf>,
    },
    /// 删除条目并广播 delete 事件（FR-T-08 骨架）。
    Delete {
        /// 条目 id。
        id: String,
    },
    /// 停止 host（退出 beacon 与服务循环）。
    Shutdown,
}

/// 运行教师端：绑定 TCP（0=自动分配；指定端口被占用自动更换，EX-11），
/// beacon 广播实际端口，处理命令直到 Shutdown 或命令通道关闭。
pub async fn run_host(
    cfg: HostConfig,
    mut cmd: mpsc::Receiver<HostCommand>,
    notify: mpsc::Sender<HostEvent>,
) -> anyhow::Result<()> {
    let store = Store::open(&cfg.data_root)?;
    let hub = Hub::new(
        store,
        cfg.room_code.clone(),
        Admitter::new(cfg.pin.clone()),
        notify.clone(),
    );
    let handler: Arc<dyn SessionHandler> = hub.clone();

    let server = match Server::bind(SocketAddr::from(([0, 0, 0, 0], cfg.tcp_port)), handler).await {
        Ok(s) => s,
        Err(e) if cfg.tcp_port != 0 => {
            tracing::warn!(port = cfg.tcp_port, error = %e, "指定端口被占用，自动更换（EX-11）");
            Server::bind(SocketAddr::from(([0, 0, 0, 0], 0)), hub.clone()).await?
        }
        Err(e) => return Err(e),
    };
    let port = server.local_addr().port();
    hub.set_port(port);
    tokio::spawn(server.run());

    let beacon = spawn_beacon(Beacon {
        protocol_version: PROTOCOL_VERSION,
        room: cfg.room_code.clone(),
        tcp_port: port,
        pin_required: cfg.pin.is_some(),
        host_name: host_name(),
    });

    notify.send(HostEvent::Started(hub.status())).await.ok();
    tracing::info!(room = %cfg.room_code, port, "教师端启动");

    while let Some(command) = cmd.recv().await {
        if matches!(command, HostCommand::Shutdown) {
            break;
        }
        hub.handle(&command).await;
    }
    beacon.abort();
    tracing::info!(room = %cfg.room_code, "教师端停止");
    Ok(())
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "teacher-pc".into())
}

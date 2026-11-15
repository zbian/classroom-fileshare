//! GUI ↔ 后台服务的本地 IPC（PRD 3.2：命名管道 + 令牌，Session 0 服务与
//! 用户会话 GUI 之间的唯一通道）。
//!
//! # 选型定案（计划 Task 7 留下的 lock 决策点）
//! 采用 **tokio 内置 windows named pipe**（`tokio::net::windows::named_pipe`）
//! 而非 interprocess crate：零新依赖、与全栈 tokio 运行时一致、CI 无新增
//! 构建风险；底层细节（多实例 accept、`ERROR_PIPE_BUSY` 客户端重试）封装
//! 在 [`server`]/[`client`] 模块内，不外溢。
//!
//! # 线协议
//! 单管道双工（请求与通知共用一条连接），行分帧 JSON：
//! 握手（token + [`IPC_PROTOCOL_VERSION`] 校验，PRD 4.5/9.4）→
//! `Request{id}` / `Reply{id}` 关联往返 + 服务端异步 `Notify` 推送。

pub mod client;
pub mod codec;
pub mod server;
pub mod token;

pub use client::{connect_ipc, IpcClient};
pub use server::serve_ipc;
pub use token::load_or_create_token;

use std::path::PathBuf;

use classroom_net::Beacon;
use classroom_protocol::{Role, ShareItem};
use serde::{Deserialize, Serialize};

pub use classroom_store::ListFilter;

/// IPC 协议版本（与教学协议 `PROTOCOL_VERSION` 相互独立；不匹配即拒绝，
/// PRD 4.5 服务与 GUI 版本不一致时拒绝连接）。
pub const IPC_PROTOCOL_VERSION: u32 = 1;

/// 服务监听的命名管道名，按角色区分教师机/学员机。
pub fn pipe_name(role: Role) -> String {
    let tag = match role {
        Role::Teacher => "teacher",
        Role::Student => "student",
    };
    format!(r"\\.\pipe\classshare-svc-{tag}")
}

/// GUI → 服务的请求（计划接口 + 修订 #4 全量）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GuiRequest {
    /// 列出条目（复用 store 的过滤条件）。
    List(ListFilter),
    /// 广播/提交一条文本。
    SendText {
        /// 标题（同名覆盖键）。
        title: String,
        /// 正文。
        body: String,
    },
    /// 广播/提交文件。
    SendFiles {
        /// 本地文件路径列表。
        paths: Vec<PathBuf>,
    },
    /// 删除条目。
    DeleteItem {
        /// 条目 id。
        id: String,
    },
    /// 用系统默认程序打开条目文件。
    OpenItem {
        /// 条目 id。
        id: String,
    },
    /// 设置昵称（修订 #4：携带 PIN 加入）。
    SetNickname {
        /// 昵称。
        nickname: String,
        /// 房间 PIN。
        pin: Option<String>,
    },
    /// 教师创建/修改本节课房间（修订 #4 新增，FR-T-01）。
    ConfigureRoom {
        /// 房间码。
        room: String,
        /// 房间 PIN；None 表示关闭 PIN。
        pin: Option<String>,
        /// 是否允许学员自动发现。
        allow_discovery: bool,
    },
    /// 查询服务状态。
    Status,
    /// 暂停/恢复接收提醒（仅抑制 tips，不影响接收落盘）。
    PauseTips(bool),
    /// 退出 GUI 进程（服务继续运行，PRD FR-G-03）。
    ShutdownGuiOnly,
}

/// 条目更新通知载荷（修订 #4：E3 覆盖提示需要 old_version）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ItemUpdatedPayload {
    /// 最新条目。
    pub item: ShareItem,
    /// 是否为同名覆盖（true 时 GUI 展示覆盖结果提示）。
    pub overwritten: bool,
    /// 被覆盖的前一版 version；新建为 None。
    pub old_version: Option<u64>,
}

/// 服务 → GUI 的通知（计划接口 + 修订 #4 全量）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum SvcNotify {
    /// 收到新共享（触发 15 秒 tips）。
    ShareReceived(ShareItem),
    /// 条目新增或覆盖（列表刷新 + E3 覆盖提示）。
    ItemUpdated(ItemUpdatedPayload),
    /// 条目被删除。
    ItemRemoved(String),
    /// 服务状态变化（房间/连接/在线数）。
    StatusChanged(String),
    /// 传输进度（0-100）。
    TransferProgress {
        /// 条目 id。
        item_id: String,
        /// 百分比。
        pct: u8,
    },
    /// 发现的教师机列表（修订 #4 新增，B1 弹窗展示房间码）。
    Discovered(Vec<Beacon>),
}

/// 服务运行状态（GUI 视角三态；修订 #6④）。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServiceState {
    /// 服务运行中。
    Running,
    /// 服务已停止（收到停机确认）。
    Stopped,
    /// IPC 不可达（服务未启动/已退出；GUI 侧本地构造）。
    Unreachable,
}

/// 服务状态快照（Status 应答载荷；修订 #6④：前端状态条结构化字段）。
/// 不 derive Eq：`Role`（protocol）仅 PartialEq。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StatusSnapshot {
    /// 服务状态。
    pub service_state: ServiceState,
    /// 房间码（teacher；student 为已加入的房间描述，未加入为空）。
    pub room: String,
    /// 在线学员数（teacher）。
    pub online_count: u32,
    /// 本机昵称（student；teacher 为空）。
    pub nickname: String,
    /// 本机角色。
    pub role: Role,
    /// 离线待发条数（M1 恒 0，M2 待发队列接入后实化）。
    pub pending_count: u32,
}

/// 请求应答载荷（计划未定义 `SvcResult` 的 Ok 载荷，本 crate 定义）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GuiReply {
    /// 通用成功（无数据）。
    Ok,
    /// List 的结果。
    Items(Vec<ShareItem>),
    /// Status 的结果（结构化快照，修订 #6④，替代原 StatusText 单轨）。
    Status(StatusSnapshot),
}

/// 服务处理结果：`Err` 为人类可读错误文案（可跨 IPC 序列化传输）。
pub type SvcResult = Result<GuiReply, String>;

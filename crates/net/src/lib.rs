//! 课堂共享网络层：UDP 发现 beacon、TCP 帧协议与会话（PRD 3.1）。
//!
//! 端口约定（M1 计划锁定）：UDP 发现 [`DISCOVERY_PORT`] = 47820、
//! TCP 服务 [`DEFAULT_TCP_PORT`] = 47821。

pub mod beacon;
pub mod frame;
pub mod session;

pub use beacon::{spawn_beacon, spawn_discovery, Beacon};
pub use frame::{
    encode_frame, read_frame, FileAck, FileChunk, FileEnd, FileHeader, Frame, Hello, HelloErr,
    HelloReply, MAX_FRAME_SIZE,
};
pub use session::{run_session, Server, SessionHandler};

/// UDP 发现端口（教师机 beacon 广播 / 学员端监听）。
pub const DISCOVERY_PORT: u16 = 47820;
/// TCP 服务默认端口（教师机监听；冲突时由 host 层自动更换并上报）。
pub const DEFAULT_TCP_PORT: u16 = 47821;

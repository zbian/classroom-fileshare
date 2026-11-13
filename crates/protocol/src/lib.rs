//! 课堂共享协议层：数据模型、四类事件、序列化与幂等合并决策（纯逻辑，零 IO，
//! 时间戳由调用方注入）。

pub mod event;
pub mod item;

pub use event::{merge, Event, EventKind, MergeDecision};
pub use item::{
    is_risky_ext, sanitize_file_name, validate_text, Audience, ItemKind, NameReject, Role,
    ShareItem, SyncState, TextReject,
};

/// 协议版本号：服务与 GUI 版本不一致时以其拒绝连接（PRD 4.5）。
pub const PROTOCOL_VERSION: u32 = 1;
/// 文件分块大小：1 MB（PRD 3.1）。
pub const CHUNK_SIZE: usize = 1024 * 1024;
/// 单文件上限：100 MB（PRD 9.4）。
pub const MAX_FILE_SIZE: u64 = 100 * 1024 * 1024;
/// 文本正文上限：10000 字（PRD 9.4）。
pub const MAX_TEXT_CHARS: usize = 10_000;
/// 默认拦截的高风险扩展名（小写，PRD 5.3）。
pub const RISKY_EXTS: &[&str] = &[
    "exe", "bat", "cmd", "com", "scr", "pif", "vbs", "js", "jar", "lnk",
];

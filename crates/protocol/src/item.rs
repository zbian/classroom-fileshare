//! ShareItem 数据模型与入站校验：文本校验、文件名净化、风险扩展名判定。

use serde::{Deserialize, Serialize};

use crate::{MAX_TEXT_CHARS, RISKY_EXTS};

/// 共享条目类型（PRD 3.5 `type`）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ItemKind {
    /// 文本。
    Text,
    /// 文件。
    File,
}

/// 接收范围（PRD 3.5 `audience`）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Audience {
    /// 全体学员。
    All,
    /// 仅教师。
    Teacher,
}

/// 发送者角色（PRD 3.5 `owner_role`）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum Role {
    /// 教师。
    Teacher,
    /// 学生。
    Student,
}

/// 同步状态（PRD 3.5 `sync_state`、7.6 状态字典）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub enum SyncState {
    /// 已同步 / 已送达。
    #[default]
    Synced,
    /// 待发送（离线暂存）。
    PendingSend,
    /// 待同步（目标端未确认）。
    PendingSync,
    /// 延迟删除（墓碑待补做）。
    DeleteDeferred,
    /// 被占用重试（原子替换失败排队）。
    RetryLocked,
    /// 失败。
    Failed,
    /// 已删除（所有目标端确认后的终态，M2 S5 ack 聚合）。
    Deleted,
}

/// 一条共享记录（PRD 3.5 ShareItem）。
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ShareItem {
    /// 全局唯一标识（UUID）。
    pub id: String,
    /// 条目类型。
    pub kind: ItemKind,
    /// 发送者标识（教师或学生昵称）。
    pub owner_id: String,
    /// 发送者角色。
    pub owner_role: Role,
    /// 接收范围。
    pub audience: Audience,
    /// 文本标题；文件为文件名（同名覆盖的名称部分）。
    pub title: String,
    /// 字节数；文本为字符数。
    pub size: u64,
    /// 文件 SHA-256；文本可为空。
    pub sha256: Option<String>,
    /// 相对仓库根目录的存储路径。
    pub rel_path: Option<String>,
    /// 创建时间（调用方注入，本 crate 不取时钟）。
    pub created_at: i64,
    /// 最近一次更新或覆盖时间（调用方注入）。
    pub updated_at: i64,
    /// 从 1 起，每次覆盖加 1。
    pub version: u64,
    /// 软删除标记。
    pub deleted: bool,
    /// 同步状态。
    pub sync_state: SyncState,
    /// 占用退避重试次数（M2 #103，PRD EX-06）。
    ///
    /// 语义：0 = 从未重试；入队后每次退避重试递增；**成功后归零**
    /// （`Synced`）；达到上限（5）后置 `Failed` 并**保持 5** 供 GUI 显示
    /// 「已用尽」。旧库行无此列 → `#[serde(default)]` / store 层默认 0。
    #[serde(default)]
    pub retry_count: u32,
}

impl ShareItem {
    /// 作用域键：owner|audience|name（PRD 5.7 同名覆盖判定）。
    ///
    /// 同名不同 owner、或同 owner 不同接收范围均不会互相覆盖；
    /// name 对文件为文件名、对文本为标题，统一取 [`ShareItem::title`]。
    pub fn scope_key(&self) -> String {
        let audience = match self.audience {
            Audience::All => "all",
            Audience::Teacher => "teacher",
        };
        format!("{}|{}|{}", self.owner_id, audience, self.title)
    }
}

/// 入站文本校验失败原因。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TextReject {
    /// 标题为空或纯空白。
    #[error("标题不能为空")]
    BlankTitle,
    /// 正文为空或纯空白。
    #[error("正文不能为空")]
    BlankBody,
    /// 正文超过 10000 字上限，携带实际字数。
    #[error("正文 {0} 字，超过 10000 字上限")]
    TooLong(usize),
}

/// 文件名净化失败原因。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum NameReject {
    /// 剥离路径成分后没有剩余文件名。
    #[error("文件名为空")]
    Empty,
    /// 净化后仍为路径穿越片段（`..`）。
    #[error("文件名包含路径穿越")]
    Traversal,
    /// 以 `.` 或空格结尾：Win32 落盘会将其剥除（PR #28 绕过链），
    /// 如 `x.exe.` 落盘后实为 `x.exe`，必须拒绝。
    #[error("文件名以点或空格结尾")]
    TrailingDotOrSpace,
    /// NTFS 保留设备名（CON/PRN/AUX/NUL/COM1-9/LPT1-9，含带扩展名形态）。
    #[error("Windows 保留设备名")]
    ReservedDevice,
    /// 含 Windows 非法字符或控制字符。
    #[error("文件名含非法字符")]
    IllegalChar,
}

/// NTFS 保留设备名清单（小写；PR #28 评审补充，PRD 9.4）。
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// 判断是否为 Windows/NTFS 保留设备名（大小写不敏感，含带扩展名形态
/// 如 `CON.txt`——这类名字即使带扩展也会被系统解析为设备）。
///
/// TODO：T-3 store 侧的同名实现改为复用本函数，避免两份清单漂移
/// （rebase t3–t7 链后处理）。
pub fn is_reserved_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("");
    let lower = stem.to_ascii_lowercase();
    RESERVED_DEVICE_NAMES.contains(&lower.as_str())
}

/// 校验入站文本（非空白、≤10000 字）。
pub fn validate_text(title: &str, body: &str) -> Result<(), TextReject> {
    if title.trim().is_empty() {
        return Err(TextReject::BlankTitle);
    }
    if body.trim().is_empty() {
        return Err(TextReject::BlankBody);
    }
    let chars = body.chars().count();
    if chars > MAX_TEXT_CHARS {
        return Err(TextReject::TooLong(chars));
    }
    Ok(())
}

/// 文件名净化：剥路径、拒绝非法字符与穿越，返回安全文件名（PRD 9.4）。
///
/// 带路径成分的名字（含绝对路径，如资源管理器右键传入）剥到最后一段；
/// 剥完为空、`.`/`..`、尾随 `.` 或空格（Win32 落盘会剥除、构成类型伪装，
/// PR #28）、NTFS 保留设备名、含 Windows 非法字符（`<>:"|?*` 及控制字符）
/// 均拒绝。获批语义（T-2 验收）：剥路径是主路径，剥离后仅剩文件名、
/// 写入 owner 子目录不可能逃逸仓库。
pub fn sanitize_file_name(raw: &str) -> Result<String, NameReject> {
    let name = raw.rsplit(['/', '\\']).next().unwrap_or("");
    if name.is_empty() {
        return Err(NameReject::Empty);
    }
    if name == "." || name == ".." {
        return Err(NameReject::Traversal);
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(NameReject::TrailingDotOrSpace);
    }
    if is_reserved_device_name(name) {
        return Err(NameReject::ReservedDevice);
    }
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control())
    {
        return Err(NameReject::IllegalChar);
    }
    Ok(name.to_string())
}

/// 判断文件名的扩展名是否属于高风险类型（大小写不敏感，PRD 5.3）。
///
/// 入口先做尾随 `.`/空格归一（`x.exe.` → `x.exe`），防止调用方绕过
/// [`sanitize_file_name`] 直接判定造成漏拦（PR #28）。
pub fn is_risky_ext(file_name: &str) -> bool {
    let normalized = file_name.trim_end_matches(['.', ' ']);
    match normalized.rsplit_once('.') {
        Some((_, ext)) => RISKY_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

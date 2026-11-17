//! GUI 本地状态与 `%APPDATA%\ClassShare\gui.json` 持久化（Task 8）。
//!
//! gui.json 是 GUI 进程专属的轻量状态（浮动图标位置、tips 暂停开关），
//! 与服务侧仓库数据无关；服务/协议状态（角色、房间）T9 起经 ipc 获取。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Mutex;

/// 图标在某一显示器工作区内的相对位置（百分比）。
/// 按显示器名存储，重启后同名气显示器优先恢复（PRD 4.1：位置按工作区百分比持久化）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconPlacement {
    pub monitor: String,
    pub x_pct: f64,
    pub y_pct: f64,
}

/// icon 窗口持久化状态。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconState {
    /// 「暂停接收提醒」本地开关（T9/T13 接 ipc 后同步给服务）
    #[serde(default)]
    pub paused_tips: bool,
    /// 学员首次昵称已完成（PRD 6.1：重启不再弹 B1）
    #[serde(default)]
    pub nickname_done: bool,
    #[serde(default)]
    pub placement: Option<IconPlacement>,
}

/// gui.json 根结构。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiConfig {
    #[serde(default)]
    pub version: u32,
    /// 本机角色："teacher" | "student" | "unset"。产品语义上由安装/服务模式决定
    /// （T13 svc.json / T7 Status）；M1 期间由 `--role=` 参数写入（debug）。
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub icon: IconState,
}

impl GuiConfig {
    pub fn path() -> PathBuf {
        config_dir().join("gui.json")
    }

    /// 读取失败（不存在/损坏）时静默回退默认值，不阻塞启动。
    /// 注意剥离 UTF-8 BOM：Windows 记事本/PowerShell 5.1 写出的 JSON 带 BOM，
    /// serde_json 不接受前导 BOM（实测会导致图标位置静默丢失，回退默认位）。
    pub fn load() -> Self {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

/// %APPDATA%\ClassShare\（Windows 约定；缺省回退当前目录，便于异常环境下仍可运行）
pub(crate) fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|p| PathBuf::from(p).join("ClassShare"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// GUI 进程运行时状态（tauri manage）。
pub struct AppState {
    pub config: Mutex<GuiConfig>,
    /// Moved 防抖代号：每次移动自增；300ms 后仍是最新值才执行吸附与持久化
    pub move_gen: AtomicU64,
    /// OLE/文件拖拽悬停态（决定 72↔124 投放态切换）
    pub drop_active: AtomicBool,
    /// 菜单展开前的 icon 窗口位置（逻辑 px），收起时精确恢复，避免累计漂移
    pub menu_base: Mutex<Option<(f64, f64)>>,
}

impl AppState {
    pub fn new(config: GuiConfig) -> Self {
        Self {
            config: Mutex::new(config),
            move_gen: AtomicU64::new(0),
            drop_active: AtomicBool::new(false),
            menu_base: Mutex::new(None),
        }
    }
}

/// 解析启动参数 `--role=teacher|student`（非法值忽略）。纯函数便于单测。
pub(crate) fn parse_role_arg(args: &[String]) -> Option<&'static str> {
    args.iter().find_map(|a| match a.strip_prefix("--role=")? {
        "teacher" => Some("teacher"),
        "student" => Some("student"),
        _ => None,
    })
}

/// 角色来源（日志与取证用；E12 补强）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoleSource {
    /// `--role=` 显式参数（安装脚本自启/调试）
    Arg,
    /// svc.json 的 `mode`——**角色权威源**，免疫「安装者≠登录用户」
    SvcJson,
    /// gui.json 的 role（上次运行持久化 / 安装脚本辅助写入，优先级最低的持久源）
    GuiJson,
    /// 服务管道探测（仅某角色管道存在时取之）
    PipeProbe,
    /// 兜底默认（历史行为 teacher）
    Default,
}

/// 角色解析优先级（E12 补强 a > b > c > default），纯函数便于单测：
///
/// 1. `--role=` 显式参数
/// 2. `svc.json` 的 `mode`——管理员安装场景下 `%APPDATA%` 属于安装者账号，
///    GUI 读不到；而 svc.json 在 `%PROGRAMDATA%`（服务模式，全员可读）或
///    当前用户 `%LOCALAPPDATA%`（降级模式），是天然的角色权威源
/// 3. `gui.json` 的 role（install.ps1 辅助写入 / 上次运行持久化）
/// 4. 管道探测：只有一个角色的管道存在时取之（覆盖「服务晚启动/角色未知」）
/// 5. 默认 teacher（历史行为）
pub(crate) fn resolve_role(
    arg: Option<&'static str>,
    svc_json: Option<&'static str>,
    gui_role: &str,
    student_pipe: bool,
    teacher_pipe: bool,
) -> (&'static str, RoleSource) {
    if let Some(role) = arg {
        return (role, RoleSource::Arg);
    }
    if let Some(role) = svc_json {
        return (role, RoleSource::SvcJson);
    }
    match gui_role {
        "student" => return ("student", RoleSource::GuiJson),
        "teacher" => return ("teacher", RoleSource::GuiJson),
        _ => {}
    }
    match (student_pipe, teacher_pipe) {
        (true, false) => ("student", RoleSource::PipeProbe),
        (false, true) => ("teacher", RoleSource::PipeProbe),
        _ => ("teacher", RoleSource::Default),
    }
}

/// 从 svc.json 读 `mode`（服务模式 PROGRAMDATA 优先，降级模式 LOCALAPPDATA）。
/// BOM 容错：Windows 工具写出的 JSON 可能带 BOM（gui.json 曾有同类问题）。
pub(crate) fn role_from_svc_json() -> Option<&'static str> {
    let mut candidates = Vec::new();
    for var in ["PROGRAMDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            candidates.push(
                std::path::PathBuf::from(base)
                    .join("ClassShare")
                    .join("svc.json"),
            );
        }
    }
    role_from_svc_json_at(&candidates)
}

/// 在给定候选路径中解析 svc.json 的角色（抽出来便于用临时文件单测）。
fn role_from_svc_json_at(paths: &[std::path::PathBuf]) -> Option<&'static str> {
    for path in paths {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let text = text.trim_start_matches('\u{feff}');
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        match value.get("mode").and_then(|m| m.as_str()) {
            Some("student") => return Some("student"),
            Some("teacher") => return Some("teacher"),
            _ => continue,
        }
    }
    None
}

/// 应用解析出的角色到内存配置；角色未变化返回 None（避免无意义写盘）。
pub(crate) fn apply_role(state: &AppState, role: &str) -> Option<GuiConfig> {
    let mut cfg = state.config.lock().ok()?;
    if cfg.role == role {
        return None;
    }
    cfg.role = role.to_string();
    Some(cfg.clone())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_camel_case() {
        let cfg = GuiConfig {
            version: 1,
            role: "teacher".into(),
            icon: IconState {
                paused_tips: true,
                nickname_done: false,
                placement: Some(IconPlacement {
                    monitor: "DELL U2415".into(),
                    x_pct: 0.95,
                    y_pct: 0.1,
                }),
            },
        };
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(s.contains("pausedTips"));
        assert!(s.contains("xPct"));
        let back: GuiConfig = serde_json::from_str(&s).unwrap();
        let p = back.icon.placement.unwrap();
        assert_eq!(p.monitor, "DELL U2415");
        assert!((p.x_pct - 0.95).abs() < 1e-9);
        assert!(back.icon.paused_tips);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        let cfg: GuiConfig = serde_json::from_str("{}").unwrap();
        assert!(!cfg.icon.paused_tips);
        assert!(cfg.icon.placement.is_none());
    }

    /// E12 ①：`--role` 解析（仅接受 teacher/student，其余忽略）。
    #[test]
    fn parse_role_arg_accepts_only_known_roles() {
        let args = |s: &str| s.split(' ').map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_role_arg(&args("app.exe --role=student")),
            Some("student")
        );
        assert_eq!(
            parse_role_arg(&args("app.exe --role=teacher")),
            Some("teacher")
        );
        assert_eq!(parse_role_arg(&args("app.exe --role=admin")), None);
        assert_eq!(parse_role_arg(&args("app.exe")), None);
    }

    /// E12 补强：角色优先级 a(--role) > b(svc.json mode) > c(gui.json) > 管道探测 > 默认。
    #[test]
    fn resolve_role_precedence() {
        // 1) --role 最高（显式覆盖一切）
        assert_eq!(
            resolve_role(Some("student"), Some("teacher"), "teacher", true, true),
            ("student", RoleSource::Arg)
        );
        // 2) svc.json 次之——管理员安装场景的关键路径（与 %APPDATA% 无关）
        assert_eq!(
            resolve_role(None, Some("student"), "teacher", false, true),
            ("student", RoleSource::SvcJson)
        );
        // 3) gui.json 再次之
        assert_eq!(
            resolve_role(None, None, "student", false, false),
            ("student", RoleSource::GuiJson)
        );
        // 4) 管道探测：仅学员管道存在 → student
        assert_eq!(
            resolve_role(None, None, "", true, false),
            ("student", RoleSource::PipeProbe)
        );
        assert_eq!(
            resolve_role(None, None, "", false, true),
            ("teacher", RoleSource::PipeProbe)
        );
        // 无信息/双管道（异常）→ 历史默认 teacher
        assert_eq!(
            resolve_role(None, None, "", false, false),
            ("teacher", RoleSource::Default)
        );
        assert_eq!(
            resolve_role(None, None, "", true, true),
            ("teacher", RoleSource::Default)
        );
    }

    /// E12 补强：svc.json 解析（含 BOM 容错、非法 mode 跳过、按候选顺序命中）。
    #[test]
    fn role_from_svc_json_reads_mode_with_bom_tolerance() {
        let dir = tempfile::tempdir().unwrap();
        let student_json = dir.path().join("student.json");
        let teacher_json = dir.path().join("teacher.json");
        let broken_json = dir.path().join("broken.json");
        let bom_json = dir.path().join("bom.json");
        std::fs::write(&student_json, r#"{"mode":"student"}"#).unwrap();
        std::fs::write(&teacher_json, r#"{"mode":"teacher"}"#).unwrap();
        std::fs::write(&broken_json, r#"{"mode":"admin"}"#).unwrap();
        std::fs::write(&bom_json, "\u{feff}{\"mode\":\"student\"}").unwrap();

        assert_eq!(
            role_from_svc_json_at(&[student_json.clone(), teacher_json.clone()]),
            Some("student"),
            "首个可解析候选优先"
        );
        assert_eq!(
            role_from_svc_json_at(&[broken_json.clone(), teacher_json.clone()]),
            Some("teacher"),
            "非法 mode 跳过继续"
        );
        assert_eq!(
            role_from_svc_json_at(&[bom_json]),
            Some("student"),
            "BOM 容错"
        );
        assert_eq!(
            role_from_svc_json_at(&[dir.path().join("missing.json")]),
            None
        );
    }

    /// E12 补强：apply_role 仅在角色变化时产出快照（避免无意义写盘）。
    #[test]
    fn apply_role_saves_only_on_change() {
        let state = AppState::new(GuiConfig::default());
        let snapshot = apply_role(&state, "student").expect("首次设置应产出快照");
        assert_eq!(snapshot.role, "student");
        assert_eq!(state.config.lock().unwrap().role, "student");
        assert!(
            apply_role(&state, "student").is_none(),
            "角色未变化不应重复写盘"
        );
        assert_eq!(
            apply_role(&state, "teacher").expect("切换应产出快照").role,
            "teacher"
        );
    }
}

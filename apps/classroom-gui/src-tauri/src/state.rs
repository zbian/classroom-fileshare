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

/// 应用 `--role` 到内存配置，返回 `(role, 待持久化快照)`。
///
/// **必须在 `ipc_client::start_bridge` 之前调用**（E12 时序竞争）：桥用
/// [`crate::ipc_client`] 内的 role_from_config 选管道名
/// （`classshare-svc-teacher` / `-student`），若角色写入晚于桥启动，首连会用
/// 旧的/默认 teacher 角色 → 学员机必然连不上。
///
/// 持久化与内存更新分离，便于单测在不触碰磁盘的前提下断言状态迁移；
/// 调用方负责 `snapshot.save()`。
pub(crate) fn apply_role_arg(state: &AppState, args: &[String]) -> Option<(String, GuiConfig)> {
    let role = parse_role_arg(args)?.to_string();
    let mut cfg = state.config.lock().ok()?;
    cfg.role = role.clone();
    let snapshot = cfg.clone();
    drop(cfg);
    Some((role, snapshot))
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

    /// E12 ①（续）：role 写入内存状态并产出待持久化快照——
    /// 该动作在 setup 中先于 start_bridge 执行，桥据此选择学员/教师管道。
    #[test]
    fn apply_role_arg_updates_state_before_bridge() {
        let state = AppState::new(GuiConfig::default());
        // 默认角色为空（等价 teacher 兜底），先确认基线
        assert_eq!(state.config.lock().unwrap().role, "");

        let args: Vec<String> = ["app.exe", "--role=student"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (role, snapshot) = apply_role_arg(&state, &args).expect("应解析出 student");
        assert_eq!(role, "student");
        assert_eq!(snapshot.role, "student", "快照用于持久化，必须已带新角色");
        // 内存状态同步更新：start_bridge 之后的 role_from_config 会读到 student
        assert_eq!(state.config.lock().unwrap().role, "student");

        // 无 --role 时不动既有配置
        let before = state.config.lock().unwrap().role.clone();
        assert!(apply_role_arg(&state, &["app.exe".to_string()]).is_none());
        assert_eq!(state.config.lock().unwrap().role, before);
    }
}

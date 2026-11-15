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
    #[serde(default)]
    pub placement: Option<IconPlacement>,
}

/// gui.json 根结构。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuiConfig {
    #[serde(default)]
    pub version: u32,
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
fn config_dir() -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_roundtrip_camel_case() {
        let cfg = GuiConfig {
            version: 1,
            icon: IconState {
                paused_tips: true,
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
}

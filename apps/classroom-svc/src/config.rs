//! 服务配置：`svc.json` 决定角色与连接参数（PRD 3.2、FR-G-05）。
//!
//! 服务模式（Session 0）：`%PROGRAMDATA%\ClassShare\svc.json`，数据根
//! `...\ClassShare\data`（机器级，未登录可接收落盘，PRD 9.5）。
//! 降级模式（`--console`，无管理员权限/RK-06）：`%LOCALAPPDATA%\ClassShare\`。

use std::net::SocketAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Windows 服务名（install.ps1 注册用，取证参数之一）。
pub const SERVICE_NAME: &str = "ClassShareSvc";

/// 运行角色。
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SvcMode {
    /// 教师机：host 核心（房间/广播）。
    Teacher,
    /// 学员机：agent 核心（发现/接收）。
    Student,
}

/// 服务配置（svc.json 载荷）。
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SvcConfig {
    /// 运行角色。
    pub mode: SvcMode,
    /// 房间码（teacher）。
    #[serde(default = "default_room_code")]
    pub room_code: String,
    /// 房间 PIN；None 表示免 PIN。
    #[serde(default)]
    pub pin: Option<String>,
    /// 昵称（student；教师端分组主键）。
    #[serde(default)]
    pub nickname: String,
    /// 手动教师机地址（student，RK-04 备用连接）。
    #[serde(default)]
    pub manual_addr: Option<SocketAddr>,
    /// TCP 监听端口（teacher；0 = 自动分配，冲突自动更换）。
    #[serde(default = "default_tcp_port")]
    pub tcp_port: u16,
    /// 主登录用户 SID（E7 令牌分发：install.ps1 写入；服务模式 GUI 读
    /// 令牌的授权对象；None 时服务模式 GUI 不可用，启动告警）。
    #[serde(default)]
    pub user_sid: Option<String>,
}

impl SvcConfig {
    /// 学员端自动发现的房间过滤（E9/PR #69 发现 1）：`room_code` 为空串
    /// → None（连首个发现，单教室默认）；非空 → 精确匹配。
    /// PRD 2.4「学生 1 步设昵称」——单教室学生无需知道房间码。
    pub fn room_filter(&self) -> Option<String> {
        let code = self.room_code.trim();
        if code.is_empty() {
            None
        } else {
            Some(code.to_string())
        }
    }
}

fn default_room_code() -> String {
    "XK-204".into()
}

fn default_tcp_port() -> u16 {
    classroom_net::DEFAULT_TCP_PORT
}

impl Default for SvcConfig {
    fn default() -> Self {
        Self {
            mode: SvcMode::Teacher,
            room_code: default_room_code(),
            pin: None,
            nickname: String::new(),
            manual_addr: None,
            tcp_port: default_tcp_port(),
            user_sid: None,
        }
    }
}

/// 服务基础目录：服务模式 = %PROGRAMDATA%\ClassShare（机器级、未登录可用），
/// 降级（console）模式 = %LOCALAPPDATA%\ClassShare（仅当前用户）。
pub fn service_base_dir(console_mode: bool) -> PathBuf {
    let var = if console_mode {
        "LOCALAPPDATA"
    } else {
        "PROGRAMDATA"
    };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"))
        .join("ClassShare")
}

/// 数据根目录（仓库 repo/、索引 index.db、令牌、logs/ 都在其下）。
pub fn data_root(console_mode: bool) -> PathBuf {
    service_base_dir(console_mode).join("data")
}

/// 从 JSON 文件加载配置。
pub fn load_config(path: &std::path::Path) -> anyhow::Result<SvcConfig> {
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_with_defaults() {
        // 最小 JSON：仅 mode；其余字段取默认
        let cfg: SvcConfig = serde_json::from_str(r#"{"mode":"teacher"}"#).unwrap();
        assert_eq!(cfg.mode, SvcMode::Teacher);
        assert_eq!(cfg.room_code, "XK-204");
        assert_eq!(cfg.tcp_port, 47821);
        assert_eq!(cfg.pin, None);

        // 完整 student 配置往返（含 manual_addr 与 user_sid）
        let full = SvcConfig {
            mode: SvcMode::Student,
            room_code: "XK-204".into(),
            pin: Some("9527".into()),
            nickname: "alice".into(),
            manual_addr: Some("127.0.0.1:47821".parse().unwrap()),
            tcp_port: 47821,
            user_sid: Some("S-1-5-21-1-2-3-1001".into()),
        };
        let json = serde_json::to_string(&full).unwrap();
        let back: SvcConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, full);
        assert!(json.contains(r#""mode":"student""#), "mode 小写序列化");
    }

    #[test]
    fn load_config_from_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("svc.json");
        std::fs::write(&path, r#"{"mode":"student","nickname":"bob"}"#).unwrap();
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.mode, SvcMode::Student);
        assert_eq!(cfg.nickname, "bob");
    }

    /// PR #69 发现 1：学员端房间过滤——空串 → None（连首个发现，
    /// 单教室 1 步入班）；非空 → Some（多教室精确匹配）。
    #[test]
    fn room_filter_empty_means_first_discovery() {
        let mut cfg: SvcConfig = serde_json::from_str(r#"{"mode":"student"}"#).unwrap();
        cfg.room_code = String::new();
        assert_eq!(cfg.room_filter(), None, "空串 → 连首个发现");

        cfg.room_code = "  ".into();
        assert_eq!(cfg.room_filter(), None, "纯空白同空串");

        cfg.room_code = "XK-204".into();
        assert_eq!(cfg.room_filter(), Some("XK-204".into()), "非空精确过滤");

        // 教师模式默认房间码保持 XK-204（教师建课可改）
        let teacher: SvcConfig = serde_json::from_str(r#"{"mode":"teacher"}"#).unwrap();
        assert_eq!(teacher.room_filter(), Some("XK-204".into()));
    }

    #[test]
    fn base_dirs_distinguished_by_mode() {
        let service = service_base_dir(false);
        let console = service_base_dir(true);
        assert!(service.ends_with("ClassShare"));
        assert!(console.ends_with("ClassShare"));
        assert_ne!(service, console, "服务与降级模式目录不同");
        assert!(data_root(false).starts_with(service));
    }
}

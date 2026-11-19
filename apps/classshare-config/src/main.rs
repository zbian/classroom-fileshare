//! 安装器配置写入小工具（供 NSIS 安装包调用，替代 PowerShell 脚本）。
//!
//! 安装包（ClassShare-Setup.exe）在安装时调用本程序写入：
//! - `svc.json`（UTF-8 无 BOM，serde_json 要求；NSIS 的 FileWrite 是 ANSI，
//!   无法正确写中文，因此由本 exe 负责）
//! - `gui.json`（角色双保险，GUI 手动启动时也正确）
//!
//! 用法：
//! ```text
//! classshare-config.exe --dir C:\ProgramData\ClassShare --mode teacher --room XK-204
//! classshare-config.exe --dir C:\ProgramData\ClassShare --mode student --nick 张三 --addr 192.168.1.7:47821 --gui-file %APPDATA%\ClassShare\gui.json
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut dir: Option<PathBuf> = None;
    let mut mode = String::from("teacher");
    let mut room = String::new();
    let mut pin = String::new();
    let mut nick = String::new();
    let mut addr = String::new();
    let mut gui_file: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut take = |name: &str| -> Result<String, String> {
            args.next().ok_or_else(|| format!("{name} 缺少参数值"))
        };
        let result = match arg.as_str() {
            "--dir" => take("--dir").map(|v| dir = Some(PathBuf::from(v))),
            "--mode" => take("--mode").map(|v| mode = v),
            "--room" => take("--room").map(|v| room = v),
            "--pin" => take("--pin").map(|v| pin = v),
            "--nick" => take("--nick").map(|v| nick = v),
            "--addr" => take("--addr").map(|v| addr = v),
            "--gui-file" => take("--gui-file").map(|v| gui_file = Some(PathBuf::from(v))),
            other => Err(format!("未知参数: {other}")),
        };
        if let Err(e) = result {
            eprintln!("classshare-config: {e}");
            return ExitCode::from(2);
        }
    }

    let Some(dir) = dir else {
        eprintln!("classshare-config: 缺少 --dir");
        return ExitCode::from(2);
    };
    if mode != "teacher" && mode != "student" {
        eprintln!("classshare-config: --mode 必须是 teacher 或 student");
        return ExitCode::from(2);
    }

    if let Err(e) = write_svc_config(&dir, &mode, &room, &pin, &nick, &addr) {
        eprintln!("classshare-config: 写入 svc.json 失败: {e}");
        return ExitCode::from(3);
    }
    let Ok(gui_file) = gui_file.ok_or("") else {
        return ExitCode::SUCCESS;
    };
    if let Err(e) = write_gui_role(&gui_file, &mode) {
        eprintln!("classshare-config: 写入 gui.json 失败: {e}");
        return ExitCode::from(3);
    }
    ExitCode::SUCCESS
}

fn write_svc_config(
    dir: &Path,
    mode: &str,
    room: &str,
    pin: &str,
    nick: &str,
    addr: &str,
) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let config = serde_json::json!({
        "mode": mode,
        "room_code": room,
        "pin": if pin.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(pin.to_string()) },
        "nickname": nick,
        "manual_addr": if addr.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(addr.to_string()) },
        "tcp_port": 47821,
        "user_sid": serde_json::Value::Null,
    });
    let text = serde_json::to_string_pretty(&config)?;
    fs::write(dir.join("svc.json"), text)
}

fn write_gui_role(path: &Path, role: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "version": 1,
        "role": role,
    }))?;
    fs::write(path, text)
}

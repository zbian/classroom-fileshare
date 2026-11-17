//! 课堂共享 GUI 入口。
//!
//! Task 1b：Tauri 2 脚手架；Task 8：程序化创建置顶浮动图标窗（icon），
//! 主界面（main）改为懒创建；tips 窗 T11 接入。
//! 窗口参数与几何策略见 `windows.rs` 与 GUI 技术方案。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod ipc_client;
mod state;
mod tips;
mod windows;

use classroom_ipc::GuiRequest;
use classroom_protocol::Role;
use ipc_client::IpcBridge;
use state::{AppState, GuiConfig};
use tauri::{Emitter, Manager};

/// 资源管理器右键/argv 携带的共享文件处理（T12）：
/// 过滤目录（EX 提示）→ SendFiles 真链路（服务端做大小/风险类型校验）。
/// 由 single-instance 回调与启动 argv 两条路径调用。
fn share_files_from_argv(app: &tauri::AppHandle, argv: &[String]) {
    let files: Vec<String> = argv
        .iter()
        .filter(|a| std::path::Path::new(a.as_str()).is_file())
        .cloned()
        .collect();
    let dirs = argv
        .iter()
        .filter(|a| std::path::Path::new(a.as_str()).is_dir())
        .count();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if !files.is_empty() {
            let result = ipc_client::svc_request(
                app.clone(),
                GuiRequest::SendFiles {
                    paths: files.into_iter().map(std::path::PathBuf::from).collect(),
                },
            )
            .await;
            let toast = match result {
                Ok(_) => "已通过「课堂共享」发送".to_string(),
                Err(e) => e,
            };
            let _ = app.emit_to(windows::ICON_LABEL, "gui://toast", toast);
        }
        if dirs > 0 {
            let _ = app.emit_to(
                windows::ICON_LABEL,
                "gui://toast",
                format!("目录暂不支持共享（{dirs} 个目录已跳过）"),
            );
        }
    });
}

/// single-instance / 启动 argv 中的共享文件提取：形如 exe 文件路径的参数。
fn file_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| {
            std::path::Path::new(a.as_str()).is_file() || std::path::Path::new(a.as_str()).is_dir()
        })
        .cloned()
        .collect()
}

/// 构建口径判定（M1 验收 O-1 防呆）：release 产物但编译期处于 dev 模式
/// （即未启用 `custom-protocol` feature → 前端资源未内嵌）。纯函数便于单测。
///
/// `is_dev` 对应 tauri 自己的语义（`tauri_build::is_dev()` / `cfg!(dev)`），
/// 与 cargo profile 无关，故不能省略该维度只看 `debug_assertions`。
fn needs_build_warning(is_dev: bool, debug_assertions: bool) -> bool {
    is_dev && !debug_assertions
}

/// release 未内嵌前端资源时弹窗明确指引，避免「白窗」被误判为产品缺陷。
/// release 构建带 `windows_subsystem = "windows"` 无控制台，故用 MessageBox。
fn warn_if_unpackaged_release() {
    if !needs_build_warning(cfg!(dev), cfg!(debug_assertions)) {
        return;
    }
    #[cfg(windows)]
    unsafe {
        // 注意 `::windows::`：crate 内有同名 `mod windows`（窗口几何模块），
        // 裸 `windows::` 会解析到本地模块而非 windows crate。
        use ::windows::core::w;
        use ::windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
        MessageBoxW(
            None,
            w!(
                "此构建未内嵌前端资源（编译时未启用 custom-protocol feature），\n\
                 启动后将尝试连接开发服务器 http://localhost:1517；\n\
                 若开发服务器未运行，界面将显示为空白。\n\n\
                 生成可分发版本请使用：scripts\\build-gui.ps1\n\
                 或：cargo build --release -p classroom-gui --features custom-protocol"
            ),
            w!("课堂共享 · 构建口径提示"),
            MB_OK | MB_ICONWARNING,
        );
    }
}

fn main() {
    warn_if_unpackaged_release();
    let config = GuiConfig::load();
    let startup_args: Vec<String> = std::env::args().collect();
    let startup_files = file_args(&startup_args);
    tauri::Builder::default()
        // 单实例：重复启动聚焦既有实例（EX-12）；argv 携带共享文件时经真实链路直发（T12）
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let files = file_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if files.is_empty() {
                windows::focus_icon_or_main(app);
            } else {
                share_files_from_argv(app, &files);
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(config))
        .manage(IpcBridge::new())
        .setup(move |app| {
            // E12 补强：角色解析优先级 a > b > c > default（必须在 start_bridge 之前）
            //   a) --role 显式参数
            //   b) svc.json 的 mode（%PROGRAMDATA% 全员可读 → 免疫「安装者≠登录用户」）
            //   c) gui.json role（上次持久化 / 安装脚本辅助）
            //   d) 服务管道探测（仅一个角色管道存在时取之）
            let gui_role = app
                .state::<AppState>()
                .config
                .lock()
                .map(|c| c.role.clone())
                .unwrap_or_default();
            let (role, source) = state::resolve_role(
                state::parse_role_arg(&startup_args),
                state::role_from_svc_json(),
                &gui_role,
                ipc_client::pipe_available(&Role::Student),
                ipc_client::pipe_available(&Role::Teacher),
            );
            if let Some(snapshot) = state::apply_role(&app.state::<AppState>(), role) {
                if let Err(e) = snapshot.save() {
                    eprintln!("[state] gui.json 角色持久化失败：{e}");
                }
            }
            eprintln!("[role] 解析为 {role}（来源 {source:?}）");
            windows::ensure_icon_window(app.handle())?;
            // tips 预创建隐藏：首条到达才 show（避免到达时才建窗的延迟）
            tips::ensure_tips_window(app.handle())?;
            // 主窗同样预创建隐藏（延迟创建实测 WebView2 白屏，见 windows.rs 注释）
            windows::ensure_main_window(app.handle())?;
            // IPC 桥：首连失败自动退避重试（E12 ②，不依赖 UI 请求触发）
            ipc_client::start_bridge(app.handle().clone());
            // HKCU 右键/argv 携带的共享文件（首次启动路径；已运行实例走 single-instance 回调）
            if !startup_files.is_empty() {
                share_files_from_argv(app.handle(), &startup_files);
            }
            // dev 演示入口：--demo-tips=N 连发 N 条（仅 debug）
            #[cfg(debug_assertions)]
            if let Some(n) = std::env::args().find_map(|a| {
                a.strip_prefix("--demo-tips=")
                    .and_then(|v| v.parse::<u32>().ok())
            }) {
                tips::demo_tips(app.handle(), n);
            }
            Ok(())
        })
        .on_window_event(windows::handle_window_event)
        .invoke_handler(tauri::generate_handler![
            icon_ready,
            open_main,
            open_icon_menu,
            close_icon_menu,
            get_icon_state,
            set_pause_tips,
            gui_quit,
            update_tips_height,
            tips_dismissed,
            open_main_locate,
            set_nickname_done,
            read_clipboard_state,
            paste_and_share,
            open_main_paste,
            svc_request,
            dev_demo_tips
        ])
        .run(tauri::generate_context!())
        .expect("classroom gui 启动失败");
}

/// icon 窗前端就绪信号：visible:false 创建，此时才 show，防止白闪（冷启动 ≤3s 指标）。
#[tauri::command]
fn icon_ready(app: tauri::AppHandle) -> Result<(), String> {
    windows::show_icon(&app)
}

/// 双击图标 / 菜单「打开课堂共享」：主窗懒创建或单例聚焦（FR-G-04）。
#[tauri::command]
fn open_main(app: tauri::AppHandle) -> Result<(), String> {
    windows::open_main(&app).map_err(|e| e.to_string())
}

/// 右键菜单展开（Rust 依工作区计算翻转并调整窗口）。
#[tauri::command]
fn open_icon_menu(app: tauri::AppHandle) -> Result<windows::MenuLayout, String> {
    windows::open_icon_menu(&app)
}

/// 右键菜单收起（恢复 72×72 基准位）。
#[tauri::command]
fn close_icon_menu(app: tauri::AppHandle) -> Result<(), String> {
    windows::close_icon_menu(&app)
}

/// icon 本地状态（暂停提醒开关）。
#[tauri::command]
fn get_icon_state(app: tauri::AppHandle) -> windows::IconStateDto {
    windows::get_icon_state(&app)
}

/// 「暂停接收提醒」本地开关（占位；T9/T13 接服务 PauseTips）。
#[tauri::command]
fn set_pause_tips(app: tauri::AppHandle, paused: bool) -> Result<bool, String> {
    windows::set_pause_tips(&app, paused)
}

/// 退出课堂共享：仅结束 GUI 进程（PRD 4.3；服务存活验证在 T13/TC-12）。
#[tauri::command]
fn gui_quit(app: tauri::AppHandle) {
    windows::quit_gui(&app)
}

/// tips 堆叠高度同步（命中区=可见区；前端 TipsStack 调用）。
#[tauri::command]
fn update_tips_height(app: tauri::AppHandle, h: f64) -> Result<(), String> {
    tips::update_tips_height(&app, h)
}

/// tips 队列清空 → 隐藏窗口（前端调用）。
#[tauri::command]
fn tips_dismissed(app: tauri::AppHandle) {
    tips::tips_dismissed(&app)
}

/// tips 点击直达主窗对应条目（T9 消费 gui://locate intent）。
#[tauri::command]
fn open_main_locate(app: tauri::AppHandle, id: String) -> Result<(), String> {
    windows::open_main_locate(&app, &id).map_err(|e| e.to_string())
}

/// GUI↔服务请求唯一入口（payload 为 GuiRequest 的 serde 外部不可辨名形态；
/// 应答已对称 retag 为前端契约 {"t":…} 形态）。错误文案原样透传（EX-10/EX-08）。
#[tauri::command]
async fn svc_request(
    app: tauri::AppHandle,
    request: GuiRequest,
) -> Result<serde_json::Value, String> {
    ipc_client::svc_request(app, request).await
}

/// 学员首次昵称完成置位（B1 不再弹出；PRD 6.1 重启不再弹）。
#[tauri::command]
fn set_nickname_done(app: tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let snapshot = {
        let mut guard = state.config.lock().map_err(|e| e.to_string())?;
        guard.icon.nickname_done = true;
        guard.clone()
    };
    snapshot.save().map_err(|e| e.to_string())
}

/// 「粘贴并共享」剪贴板探测（CF_UNICODETEXT/CF_HDROP；EX-03 占用文案透传）。
#[tauri::command]
fn read_clipboard_state() -> Result<clipboard::ClipboardStateDto, String> {
    clipboard::read_clipboard_state()
}

/// 「粘贴并共享」动作（icon 菜单）：文件→直发（toast 通知结果）；文本→开主窗
/// 预填 E1 确认（gui://paste-text）；空→EX-03 文案。返回给菜单的提示文本。
#[tauri::command]
fn paste_and_share(app: tauri::AppHandle) -> Result<String, String> {
    let st = clipboard::read_clipboard_state()?;
    if !st.files.is_empty() {
        let dirs = st.dirs;
        let app2 = app.clone();
        let files = st.files.clone();
        tauri::async_runtime::spawn(async move {
            let toast = match ipc_client::svc_request(
                app2.clone(),
                GuiRequest::SendFiles {
                    paths: files.into_iter().map(std::path::PathBuf::from).collect(),
                },
            )
            .await
            {
                Ok(_) => format!("已共享剪贴板中的 {} 个文件", st.files.len()),
                Err(e) => e,
            };
            let _ = app2.emit_to(windows::ICON_LABEL, "gui://toast", toast);
        });
        if dirs > 0 {
            let _ = app.emit_to(
                windows::ICON_LABEL,
                "gui://toast",
                format!("目录暂不支持共享（{dirs} 个目录已跳过）"),
            );
        }
        Ok(String::new())
    } else if let Some(text) = st.text.filter(|t| !t.trim().is_empty()) {
        windows::open_main(&app).map_err(|e| e.to_string())?;
        app.emit_to(windows::MAIN_LABEL, "gui://paste-text", text)
            .map_err(|e| e.to_string())?;
        Ok(String::new())
    } else {
        // EX-03 文案（PRD 8.3）
        Err("剪贴板中没有可共享的文本或文件".into())
    }
}

/// 右键「粘贴并共享」文本路径：开主窗并预填 E1（gui://paste-text intent）。
#[tauri::command]
fn open_main_paste(app: tauri::AppHandle, body: String) -> Result<(), String> {
    windows::open_main(&app).map_err(|e| e.to_string())?;
    app.emit_to(windows::MAIN_LABEL, "gui://paste-text", body)
        .map_err(|e| e.to_string())
}

/// dev 演示：连发 N 条 tips（tips.html#demo=N 触发；仅 debug 生效）。
#[tauri::command]
fn dev_demo_tips(app: tauri::AppHandle, n: u32) {
    #[cfg(debug_assertions)]
    tips::demo_tips(&app, n);
    #[cfg(not(debug_assertions))]
    {
        let _ = (&app, n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// O-1 防呆判定：仅「release + dev 模式（未启用 custom-protocol）」需要提示。
    #[test]
    fn build_warning_only_for_dev_mode_release() {
        // release + dev 模式 → 提示（裸 cargo build --release 的情形）
        assert!(needs_build_warning(true, false));
        // release + 已内嵌（--features custom-protocol）→ 不提示
        assert!(!needs_build_warning(false, false));
        // debug：开发流（连 dev server）属正常，不提示
        assert!(!needs_build_warning(true, true));
        assert!(!needs_build_warning(false, true));
    }

    /// 当前测试构建为 debug，判定必须为 false（保证不影响开发流）。
    #[test]
    fn current_build_does_not_warn_here() {
        assert!(!needs_build_warning(cfg!(dev), cfg!(debug_assertions)));
    }
}

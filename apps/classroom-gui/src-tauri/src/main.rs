//! 课堂共享 GUI 入口。
//!
//! Task 1b：Tauri 2 脚手架；Task 8：程序化创建置顶浮动图标窗（icon），
//! 主界面（main）改为懒创建；tips 窗 T11 接入。
//! 窗口参数与几何策略见 `windows.rs` 与 GUI 技术方案。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod state;
mod tips;
mod windows;

use state::{AppState, GuiConfig};

fn main() {
    let config = GuiConfig::load();
    tauri::Builder::default()
        // 单实例：重复启动聚焦既有实例（EX-12）；argv 转发（资源管理器右键多选）T12 接入
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            windows::focus_icon_or_main(app);
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::new(config))
        .setup(|app| {
            windows::ensure_icon_window(app.handle())?;
            // tips 预创建隐藏：首条到达才 show（避免到达时才建窗的延迟）
            tips::ensure_tips_window(app.handle())?;
            // 主窗同样预创建隐藏（延迟创建实测 WebView2 白屏，见 windows.rs 注释）
            windows::ensure_main_window(app.handle())?;
            // dev 演示入口：--demo-tips=N 连发 N 条（仅 debug）
            #[cfg(debug_assertions)]
            if let Some(n) = std::env::args().find_map(|a| {
                a.strip_prefix("--demo-tips=")
                    .and_then(|v| v.parse::<u32>().ok())
            }) {
                #[cfg(debug_assertions)]
                eprintln!("[t11] demo-tips 启动参数命中：n={n}");
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

//! 课堂共享 GUI 入口。
//!
//! Task 1b：Tauri 2 脚手架；Task 8：程序化创建置顶浮动图标窗（icon），
//! 主界面（main）改为懒创建；tips 窗 T11 接入。
//! 窗口参数与几何策略见 `windows.rs` 与 GUI 技术方案。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod clipboard;
mod dnd;
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

/// 判断参数是否指向**当前进程自身的可执行文件**（E13：argv[0] 不能被当成共享文件）。
///
/// 两条入口的 argv 语义（本机实测，见 PR 描述）：
/// - 进程启动：`std::env::args()` 首项 = 本进程 exe 路径（`is_file()=true`，
///   此前会被误当共享文件发送，仅靠服务端「高风险类型」拦下——防御太靠下游）；
/// - 单实例回调：`tauri-plugin-single-instance` 在 Windows 上经 `second-instance`
///   事件传入第二实例**完整命令行**（含其 argv[0]，即同一 exe 路径）。
///
/// 因此这里**不用 `skip(1)`**（若插件将来只传附加参数，skip(1) 会吞掉第一个真实
/// 文件），而是统一排除「等于本进程 exe 的参数」——对两种 argv 语义都安全。
fn is_self_exe(arg: &str) -> bool {
    match std::env::current_exe() {
        Ok(current) => same_path(arg, &current),
        Err(_) => false,
    }
}

/// 路径等价判定（Windows 大小写不敏感）：优先 canonicalize 归一相对路径/`..`，
/// 失败时退回规范化字符串比较。
fn same_path(candidate: &str, target: &std::path::Path) -> bool {
    let cand = std::path::Path::new(candidate);
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(cand), std::fs::canonicalize(target)) {
        return a == b;
    }
    let norm = |p: &str| p.replace('/', "\\").to_lowercase();
    norm(candidate) == norm(&target.to_string_lossy())
}

/// single-instance / 启动 argv 中的共享文件提取：过滤自身 exe 后，仅保留文件与目录。
fn file_args(args: &[String]) -> Vec<String> {
    args.iter()
        .filter(|a| !is_self_exe(a))
        .filter(|a| {
            let p = std::path::Path::new(a.as_str());
            p.is_file() || p.is_dir()
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

/// 命令表：release 构建**不注册**任何 `dev_*` 命令 → 生产产物无调试入口残留
/// （`dev_drop_preview` 等仅 debug 注册；release 产物已 grep 验证 0 命中）。
#[cfg(debug_assertions)]
macro_rules! app_commands {
    () => {
        tauri::generate_handler![
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
            send_dropped_files,
            set_icon_drop_state,
            svc_request,
            dev_demo_tips
        ]
    };
}

#[cfg(not(debug_assertions))]
macro_rules! app_commands {
    () => {
        tauri::generate_handler![
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
            send_dropped_files,
            set_icon_drop_state,
            svc_request
        ]
    };
}

fn main() {
    warn_if_unpackaged_release();
    let config = GuiConfig::load();
    let startup_args: Vec<String> = std::env::args().collect();
    let startup_files = file_args(&startup_args);
    tauri::Builder::default()
        // 单实例：重复启动聚焦既有实例（EX-12）；argv 携带共享文件时经真实链路直发（T12）
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // E13：记录单实例路径的 argv 语义（首项是否为 exe），便于回归核对
            #[cfg(debug_assertions)]
            eprintln!("[single-instance] argc={} argv={argv:?}", argv.len());
            let files = file_args(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if files.is_empty() {
                windows::focus_icon_or_main(app);
            } else {
                share_files_from_argv(app, &files);
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            dnd::cleanup_root();
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
        .invoke_handler(app_commands!())
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

/// 拖放文件入口（HTML5 DnD）：校验/落盘/发送/清理见 `dnd` 模块。
#[tauri::command]
async fn send_dropped_files(
    app: tauri::AppHandle,
    files: Vec<dnd::DroppedFileDto>,
) -> Result<usize, String> {
    dnd::save_and_send(app, files).await
}

/// 图标窗投放态（release 可用）：关闭原生 drag-drop 后由前端 dragenter/leave 驱动，
/// 窗口在 72×72 ↔ 124×124 之间切换（PRD 4.1 / FR-G-01）。
#[tauri::command]
fn set_icon_drop_state(app: tauri::AppHandle, active: bool, files: usize, dirs: usize) {
    windows::set_icon_drop_state(&app, active, files, dirs);
}
/// dev 演示：连发 N 条 tips（tips.html#demo=N 触发）。release 不注册本命令。
#[cfg(debug_assertions)]
#[tauri::command]
fn dev_demo_tips(app: tauri::AppHandle, n: u32) {
    tips::demo_tips(&app, n);
}

#[cfg(test)]
mod tests {
    use super::*;
    /// E13：argv[0]（本进程 exe）不得被当成共享文件。
    #[test]
    fn file_args_excludes_self_exe() {
        let exe = std::env::current_exe()
            .expect("current_exe")
            .to_string_lossy()
            .to_string();
        assert!(is_self_exe(&exe), "current_exe 应判定为自身");
        assert!(
            file_args(&[exe]).is_empty(),
            "只有 argv[0] 时不应产生待共享文件"
        );
    }

    /// E13：exe + 真实文件时只保留真实文件；多文件与目录不回归。
    #[test]
    fn file_args_keeps_files_and_dirs_after_self_exe() {
        let dir = std::env::temp_dir().join(format!("e13-args-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f1 = dir.join("作业.txt");
        let f2 = dir.join("图.png");
        std::fs::write(&f1, b"a").unwrap();
        std::fs::write(&f2, b"b").unwrap();
        let sub = dir.join("素材");
        std::fs::create_dir_all(&sub).unwrap();
        let exe = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let args = vec![
            exe,
            f1.to_string_lossy().to_string(),
            f2.to_string_lossy().to_string(),
            sub.to_string_lossy().to_string(),
        ];
        let got = file_args(&args);
        assert_eq!(got.len(), 3, "exe 排除、2 文件 + 1 目录保留：{got:?}");
        assert!(got.contains(&f1.to_string_lossy().to_string()));
        assert!(got.contains(&sub.to_string_lossy().to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// E13：大小写不同 / 带 `.` 段等等价路径形式同样识别为自身 exe。
    #[test]
    fn self_exe_detected_case_insensitive_and_dot_segments() {
        let exe = std::env::current_exe().unwrap();
        let plain = exe.to_string_lossy().to_string();
        assert!(is_self_exe(&plain));
        assert!(is_self_exe(&plain.to_uppercase()), "大写形式应识别为自身");
        let dotted = exe
            .parent()
            .unwrap()
            .join(".")
            .join(exe.file_name().unwrap())
            .to_string_lossy()
            .to_string();
        assert!(is_self_exe(&dotted), "带 . 段的等价形式应识别为自身");
    }

    /// E13：空 argv / 不存在的路径不产生条目（不 panic）。
    #[test]
    fn file_args_handles_empty_and_missing_paths() {
        assert!(file_args(&[]).is_empty());
        let missing = std::env::temp_dir()
            .join("e13-missing-not-exist.txt")
            .to_string_lossy()
            .to_string();
        assert!(file_args(&[missing]).is_empty());
    }
}

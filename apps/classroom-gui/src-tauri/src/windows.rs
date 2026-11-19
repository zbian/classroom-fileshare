//! icon / main 窗口的创建、事件与几何逻辑（Task 8：FR-G-01/03/04）。
//!
//! 设计要点：
//! - 命中区 = 可见区：icon 窗常态 72×72；拖拽悬停放大 124×124（投放态，中心不动）；
//!   右键菜单展开为 304×252（必要时向左/向上翻转），收起精确复位（避免漂移）。
//! - 位置持久化：Moved 防抖 300ms → 距工作区边缘 ≤12px 吸附 → 按 monitor.name +
//!   工作区百分比写 gui.json；恢复时同名气显示器优先，缺失回主屏默认位。
//! - 全程 Logical 坐标，抗多显示器 DPI 差异。

use crate::state::{AppState, IconPlacement};
use classroom_ipc::GuiRequest;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{
    AppHandle, DragDropEvent, Emitter, LogicalPosition, LogicalSize, Manager, Monitor, WebviewUrl,
    WebviewWindowBuilder, Window,
};

pub const ICON_LABEL: &str = "icon";
pub const MAIN_LABEL: &str = "main";

/// 浮动图标常态尺寸（逻辑 px，PRD 4.1）
pub const ICON_SIZE: f64 = 72.0;
/// 拖拽投放态尺寸（PRD 4.1：约 124px）
pub const ICON_DROP_SIZE: f64 = 124.0;
/// 菜单展开窗口：圆 72 + 间距 12 + 菜单宽 220；高 ≈ 菜单高 246
pub const MENU_W: f64 = 304.0;
pub const MENU_H: f64 = 252.0;
/// 距工作区边缘 ≤12px 吸附（PRD 4.1：可吸附屏幕边缘）
pub const SNAP_MARGIN: f64 = 12.0;
/// Moved 防抖窗口
const MOVE_DEBOUNCE_MS: u64 = 300;
/// 默认位置边距：主显示器工作区右上角
const DEFAULT_EDGE_MARGIN: f64 = 24.0;

/// 工作区矩形（逻辑 px）：(x, y, w, h)
pub type WorkArea = (f64, f64, f64, f64);

// ---------------------------------------------------------------------------
// 纯几何函数（可单测）
// ---------------------------------------------------------------------------

/// 落点吸附：距工作区任一边 ≤ margin 时贴齐该边。
pub fn snap_position(pos: (f64, f64), size: (f64, f64), work: WorkArea, margin: f64) -> (f64, f64) {
    let (mut x, mut y) = pos;
    let (wx, wy, ww, wh) = work;
    if x <= wx + margin {
        x = wx;
    }
    if ww > size.0 && x + size.0 >= wx + ww - margin {
        x = wx + ww - size.0;
    }
    if y <= wy + margin {
        y = wy;
    }
    if wh > size.1 && y + size.1 >= wy + wh - margin {
        y = wy + wh - size.1;
    }
    (x, y)
}

/// 位置 → 工作区百分比。分母为「工作区 − 窗口尺寸」，保证图标永不压到任务栏。
pub fn to_pct(pos: (f64, f64), size: (f64, f64), work: WorkArea) -> Option<(f64, f64)> {
    let (wx, wy, ww, wh) = work;
    let fx = ww - size.0;
    let fy = wh - size.1;
    if fx <= 0.0 || fy <= 0.0 {
        return None;
    }
    Some((
        ((pos.0 - wx) / fx).clamp(0.0, 1.0),
        ((pos.1 - wy) / fy).clamp(0.0, 1.0),
    ))
}

/// 百分比 → 位置（`to_pct` 的逆）。
pub fn from_pct(pct: (f64, f64), size: (f64, f64), work: WorkArea) -> (f64, f64) {
    let (wx, wy, ww, wh) = work;
    let fx = (ww - size.0).max(0.0);
    let fy = (wh - size.1).max(0.0);
    (
        wx + pct.0.clamp(0.0, 1.0) * fx,
        wy + pct.1.clamp(0.0, 1.0) * fy,
    )
}

/// 显示器工作区（逻辑 px）。
pub fn work_area_logical(m: &Monitor) -> WorkArea {
    let s = m.scale_factor();
    let r = m.work_area();
    (
        r.position.x as f64 / s,
        r.position.y as f64 / s,
        r.size.width as f64 / s,
        r.size.height as f64 / s,
    )
}

// ---------------------------------------------------------------------------
// 窗口创建
// ---------------------------------------------------------------------------

/// 启动即建 icon 窗（visible:false，前端 icon_ready 后 show，防白闪）。
pub fn ensure_icon_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(ICON_LABEL).is_some() {
        return Ok(());
    }
    let placement = app
        .state::<AppState>()
        .config
        .lock()
        .ok()
        .and_then(|c| c.icon.placement.clone());

    // 恢复：同名气显示器优先；无存档/显示器缺失 → 主屏默认位（右上角）
    let (x, y) = placement
        .and_then(|p| {
            app.available_monitors()
                .ok()?
                .iter()
                .find(|m| m.name() == Some(&p.monitor))
                .cloned()
                .map(|m| {
                    from_pct(
                        (p.x_pct, p.y_pct),
                        (ICON_SIZE, ICON_SIZE),
                        work_area_logical(&m),
                    )
                })
        })
        .unwrap_or_else(|| default_position(app));

    let win = WebviewWindowBuilder::new(app, ICON_LABEL, WebviewUrl::App("icon.html".into()))
        .title("课堂共享")
        .disable_drag_drop_handler()
        .inner_size(ICON_SIZE, ICON_SIZE)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    // tao 创建期对 undecorated+shadow 组合会按 calculate_insets_for_dpi 预扩窗口
    // （tao-0.37 window.rs 创建路径），实测 outer 被撑大到 136×72；创建后按逻辑
    // 尺寸显式归一，保证「命中区=可见区」的 72×72 基准。
    win.set_size(LogicalSize::new(ICON_SIZE, ICON_SIZE))?;
    #[cfg(debug_assertions)]
    {
        let o = win
            .outer_size()
            .map(|s| (s.width, s.height))
            .unwrap_or((0, 0));
        let i = win
            .inner_size()
            .map(|s| (s.width, s.height))
            .unwrap_or((0, 0));
        let p = win.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
        eprintln!(
            "[t8] icon created: outer={o:?} inner={i:?} scale={} pos={p:?}",
            win.scale_factor().unwrap_or(0.0)
        );
    }
    Ok(())
}

fn default_position(app: &AppHandle) -> (f64, f64) {
    let work = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| work_area_logical(&m))
        .unwrap_or((0.0, 0.0, 1920.0, 1040.0));
    (
        work.0 + work.2 - ICON_SIZE - DEFAULT_EDGE_MARGIN,
        work.1 + DEFAULT_EDGE_MARGIN,
    )
}

/// 主窗：启动预创建隐藏（与 icon/tips 相同的 setup 线程创建路径），
/// 双击/菜单/tips 点击仅 show+focus。此前从命令线程延迟创建在 WebView2 上
/// 表现为窗口白色、页面不加载（实测），预创建规避。
pub fn ensure_main_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(MAIN_LABEL).is_some() {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, MAIN_LABEL, WebviewUrl::App("main.html".into()))
        .title("课堂共享")
        .disable_drag_drop_handler()
        .inner_size(1024.0, 680.0)
        .min_inner_size(960.0, 640.0)
        .visible(false)
        .build()?;
    Ok(())
}

/// 双击/菜单打开主界面：已存在则 show+unminimize+set_focus（PRD 4.4 单例聚焦），否则建后即显。
pub fn open_main(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        w.show()?;
        let _ = w.unminimize();
        w.set_focus()?;
        return Ok(());
    }
    ensure_main_window(app)?;
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        w.show()?;
        w.set_focus()?;
    }
    Ok(())
}

/// 单实例回调（EX-12）：重复启动聚焦既有窗口。
/// tips 点击直达：打开主窗并定位到对应条目。
/// T9 消费 `gui://locate`（切 Tab + 滚动 + 高亮）；主窗首次创建时 webview 可能
/// 尚未就绪，届时改为 intent 队列，确保事件不丢。
pub fn open_main_locate(app: &AppHandle, item_id: &str) -> tauri::Result<()> {
    open_main(app)?;
    let _ = app.emit_to(MAIN_LABEL, "gui://locate", item_id);
    Ok(())
}

/// 单实例回调（EX-12）：重复启动聚焦既有窗口。
pub fn focus_icon_or_main(app: &AppHandle) {
    let target = app
        .get_webview_window(ICON_LABEL)
        .or_else(|| app.get_webview_window(MAIN_LABEL));
    if let Some(w) = target {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// 前端就绪：显示 icon 窗（不抢焦点）。
pub fn show_icon(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(ICON_LABEL) {
        w.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 退出课堂共享：仅结束 GUI 进程，不触碰后台服务与共享数据（PRD 4.3 / TC-12）。
/// T13 接 ipc 后此处先发 GuiRequest::ShutdownGuiOnly 再退出。
pub fn quit_gui(app: &AppHandle) {
    if let Ok(cfg) = app.state::<AppState>().config.lock() {
        let _ = cfg.clone().save();
    }
    app.exit(0);
}

// ---------------------------------------------------------------------------
// 窗口事件（Builder::on_window_event 全局分发）
// ---------------------------------------------------------------------------

pub fn handle_window_event(window: &Window, event: &tauri::WindowEvent) {
    match window.label() {
        ICON_LABEL => handle_icon_event(window, event),
        // 主窗关闭 = 隐藏（PRD 4.4：关闭窗口不退出程序，图标常驻）
        MAIN_LABEL => {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if let Some(w) = window.app_handle().get_webview_window(MAIN_LABEL) {
                    let _ = w.hide();
                }
            }
        }
        _ => {}
    }
}

fn handle_icon_event(window: &Window, event: &tauri::WindowEvent) {
    let app = window.app_handle().clone();
    match event {
        tauri::WindowEvent::Moved(_) => on_moved(&app),
        // 失焦收起右键菜单（自绘菜单关闭路径之一）
        tauri::WindowEvent::Focused(false) => {
            let _ = app.emit_to(ICON_LABEL, "gui://icon-blur", true);
        }
        // 图标常驻：Alt+F4/系统关闭请求一律阻止，退出只走菜单
        tauri::WindowEvent::CloseRequested { api, .. } => api.prevent_close(),
        tauri::WindowEvent::DragDrop(dd) => handle_drag_drop(&app, dd),
        // 下划线前缀：仅供 debug 日志使用（release clippy 守卫：M1 收尾修复
        // unused variable 盲区）
        tauri::WindowEvent::Resized(_size) => {
            #[cfg(debug_assertions)]
            eprintln!("[t8] icon resized: {_size:?}");
        }
        _ => {}
    }
}

/// Moved 防抖：OS 模态拖动期间事件连发，300ms 静止视为落点。
fn on_moved(app: &AppHandle) {
    let gen = app
        .state::<AppState>()
        .move_gen
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(MOVE_DEBOUNCE_MS)).await;
        let state = app.state::<AppState>();
        if state.move_gen.load(Ordering::SeqCst) != gen {
            return;
        }
        // 菜单展开期间的窗口位移不参与吸附/持久化
        if state.menu_base.lock().map(|m| m.is_some()).unwrap_or(false) {
            return;
        }
        let _ = snap_and_persist(&app);
    });
}

/// 吸附 + 按百分比持久化。
fn snap_and_persist(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window(ICON_LABEL) else {
        return Ok(());
    };
    let Some(mon) = win.current_monitor()? else {
        return Ok(());
    };
    let scale = mon.scale_factor();
    let pos = win.outer_position()?;
    let size = win.outer_size()?;
    let work = work_area_logical(&mon);
    let pos_l = (pos.x as f64 / scale, pos.y as f64 / scale);
    let size_l = (size.width as f64 / scale, size.height as f64 / scale);

    let snapped = snap_position(pos_l, size_l, work, SNAP_MARGIN);
    let moved = (snapped.0 - pos_l.0).abs() > 0.01 || (snapped.1 - pos_l.1).abs() > 0.01;
    if moved {
        win.set_position(LogicalPosition::new(snapped.0, snapped.1))?;
    }

    if let Some((xp, yp)) = to_pct(snapped, size_l, work) {
        let name = mon.name().cloned().unwrap_or_default();
        let snapshot = {
            let state = app.state::<AppState>();
            let Ok(mut guard) = state.config.lock() else {
                return Ok(());
            };
            guard.icon.placement = Some(IconPlacement {
                monitor: name,
                x_pct: xp,
                y_pct: yp,
            });
            guard.clone()
        };
        let _ = snapshot.save();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 拖拽投放态（72 ↔ 124，中心不动；文件拖入即放大，文本 OLE 在 T12 接入）
// ---------------------------------------------------------------------------

fn handle_drag_drop(app: &AppHandle, dd: &DragDropEvent) {
    match dd {
        // A4 投放态：进入即放大并携带「拖入文件/目录数量」，前端据此展示反馈
        DragDropEvent::Enter { paths, .. } => {
            let (files, dirs) = count_paths(paths);
            set_drop_state(app, true, Some((files, dirs)));
        }
        DragDropEvent::Leave => set_drop_state(app, false, None),
        DragDropEvent::Drop { paths, .. } => {
            set_drop_state(app, false, None);
            // T12：接入 SendFiles 真链路；目录过滤并提示（EX：目录暂不支持共享）
            let mut files = Vec::new();
            let mut dirs = 0u32;
            for p in paths {
                let path = p.to_string_lossy().to_string();
                if std::path::Path::new(&path).is_dir() {
                    dirs += 1;
                } else {
                    files.push(path);
                }
            }
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let mut toasts: Vec<String> = Vec::new();
                if !files.is_empty() {
                    match crate::ipc_client::svc_request(
                        app2.clone(),
                        GuiRequest::SendFiles {
                            paths: files.into_iter().map(std::path::PathBuf::from).collect(),
                        },
                    )
                    .await
                    {
                        Ok(_) => toasts.push("已通过拖放共享".into()),
                        Err(e) => toasts.push(e),
                    }
                }
                if dirs > 0 {
                    toasts.push(format!("目录暂不支持共享（{dirs} 个目录已跳过）"));
                }
                for t in toasts {
                    let _ = app2.emit_to(ICON_LABEL, "gui://toast", t);
                }
            });
        }
        DragDropEvent::Over { .. } => {}
        _ => {}
    }
}

/// 拖入内容数量统计（文件 / 目录），供投放态反馈展示（A4）
fn count_paths(paths: &[std::path::PathBuf]) -> (usize, usize) {
    let mut files = 0;
    let mut dirs = 0;
    for p in paths {
        if p.is_dir() {
            dirs += 1;
        } else {
            files += 1;
        }
    }
    (files, dirs)
}

/// 仅调试用：直接进入/退出投放态并下发数量（供 A4 视觉验证/QA 截图；
/// release 下不编译，命令注册处有 cfg 处理）。
#[cfg(debug_assertions)]
pub fn dev_drop_preview(app: &AppHandle, active: bool, files: usize, dirs: usize) {
    set_drop_state(app, active, Some((files, dirs)));
}
fn set_drop_state(app: &AppHandle, active: bool, counts: Option<(usize, usize)>) {
    let state = app.state::<AppState>();
    if state.drop_active.swap(active, Ordering::SeqCst) == active {
        // 尺寸态未变：仍下发一次数量（Over 事件可能在进入后带更精确的列表）
        if active {
            if let Some((files, dirs)) = counts {
                let payload = serde_json::json!({ "active": true, "files": files, "dirs": dirs });
                let _ = app.emit_to(ICON_LABEL, "gui://icon-drag", payload);
            }
        }
        return; // 避免 Over 事件反复触发窗口尺寸动画
    }
    let Some(win) = app.get_webview_window(ICON_LABEL) else {
        return;
    };
    let scale = win
        .current_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let delta = (ICON_DROP_SIZE - ICON_SIZE) / 2.0; // 中心不动
    if let Ok(pos) = win.outer_position() {
        let (dx, dy) = if active {
            (-delta, -delta)
        } else {
            (delta, delta)
        };
        let x = pos.x as f64 / scale + dx;
        let y = pos.y as f64 / scale + dy;
        let _ = win.set_position(LogicalPosition::new(x, y));
    }
    let target = if active {
        LogicalSize::new(ICON_DROP_SIZE, ICON_DROP_SIZE)
    } else {
        LogicalSize::new(ICON_SIZE, ICON_SIZE)
    };
    if win.set_size(target).is_ok() {
        let payload = match counts {
            Some((files, dirs)) if active => {
                serde_json::json!({ "active": true, "files": files, "dirs": dirs })
            }
            _ => serde_json::json!({ "active": active, "files": 0, "dirs": 0 }),
        };
        let _ = app.emit_to(ICON_LABEL, "gui://icon-drag", payload);
    }
}

// ---------------------------------------------------------------------------
// 右键菜单展开/收起（窗口级尺寸切换，Rust 负责屏幕边界翻转）
// ---------------------------------------------------------------------------

/// 前端据 flipX/flipY 镜像布局（菜单在圆的左/右侧、上/下方）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuLayout {
    pub flip_x: bool,
    pub flip_y: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconStateDto {
    pub paused_tips: bool,
    /// 学员首次昵称已完成（B1 不再弹出）
    pub nickname_done: bool,
    /// "teacher" | "student" | "unset"（GuiConfig.role）
    pub role: String,
}

pub fn get_icon_state(app: &AppHandle) -> IconStateDto {
    let (paused, done, role) = app
        .state::<AppState>()
        .config
        .lock()
        .map(|c| (c.icon.paused_tips, c.icon.nickname_done, c.role.clone()))
        .unwrap_or((false, false, "unset".into()));
    IconStateDto {
        paused_tips: paused,
        nickname_done: done,
        role,
    }
}

/// 展开菜单：记录基准位 → 依工作区决定翻转 → 调整窗口位置与尺寸。幂等。
pub fn open_icon_menu(app: &AppHandle) -> Result<MenuLayout, String> {
    let Some(win) = app.get_webview_window(ICON_LABEL) else {
        return Err("icon 窗口不存在".into());
    };
    let mon = win
        .current_monitor()
        .ok()
        .flatten()
        .ok_or_else(|| "无法获取当前显示器".to_string())?;
    let scale = mon.scale_factor();
    let work = work_area_logical(&mon);
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let base = (pos.x as f64 / scale, pos.y as f64 / scale);

    let state = app.state::<AppState>();
    let mut menu_base = state.menu_base.lock().map_err(|e| e.to_string())?;
    if menu_base.is_none() {
        *menu_base = Some(base);
    }
    let base = menu_base.unwrap_or(base);

    let flip_x = base.0 + MENU_W > work.0 + work.2 + 0.5;
    let flip_y = base.1 + MENU_H > work.1 + work.3 + 0.5;
    let x = if flip_x {
        base.0 + ICON_SIZE - MENU_W
    } else {
        base.0
    };
    let y = if flip_y {
        base.1 + ICON_SIZE - MENU_H
    } else {
        base.1
    };

    win.set_position(LogicalPosition::new(x, y))
        .and_then(|_| win.set_size(LogicalSize::new(MENU_W, MENU_H)))
        .map_err(|e| e.to_string())?;
    Ok(MenuLayout { flip_x, flip_y })
}

/// 收起菜单：精确恢复基准位与 72×72。幂等。
pub fn close_icon_menu(app: &AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window(ICON_LABEL) else {
        return Ok(());
    };
    let state = app.state::<AppState>();
    let base = state.menu_base.lock().map_err(|e| e.to_string())?.take();
    if let Some((x, y)) = base {
        win.set_position(LogicalPosition::new(x, y))
            .and_then(|_| win.set_size(LogicalSize::new(ICON_SIZE, ICON_SIZE)))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 「暂停接收提醒」本地开关（占位持久化；T9/T13 接服务 PauseTips）。
pub fn set_pause_tips(app: &AppHandle, paused: bool) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let snapshot = {
        let mut guard = state.config.lock().map_err(|e| e.to_string())?;
        guard.icon.paused_tips = paused;
        guard.clone()
    };
    snapshot.save().map_err(|e| e.to_string())?;
    Ok(paused)
}

// ---------------------------------------------------------------------------
// 单元测试：吸附与百分比几何
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const W: WorkArea = (0.0, 0.0, 1920.0, 1040.0);

    #[test]
    fn snap_near_left_edge() {
        let p = snap_position((5.0, 100.0), (ICON_SIZE, ICON_SIZE), W, SNAP_MARGIN);
        assert_eq!(p, (0.0, 100.0));
    }

    #[test]
    fn snap_near_right_and_bottom() {
        let p = snap_position((1900.0, 1000.0), (ICON_SIZE, ICON_SIZE), W, SNAP_MARGIN);
        assert_eq!(p, (1920.0 - 72.0, 1040.0 - 72.0));
    }

    #[test]
    fn no_snap_when_far_from_edges() {
        let p = snap_position((500.0, 300.0), (ICON_SIZE, ICON_SIZE), W, SNAP_MARGIN);
        assert_eq!(p, (500.0, 300.0));
    }

    #[test]
    fn snap_idempotent() {
        let once = snap_position((1900.0, 1000.0), (ICON_SIZE, ICON_SIZE), W, SNAP_MARGIN);
        let twice = snap_position(once, (ICON_SIZE, ICON_SIZE), W, SNAP_MARGIN);
        assert_eq!(once, twice);
    }

    #[test]
    fn snap_never_outside_work_area_smaller_than_window() {
        // 工作区比窗口还小：右/下吸附分支应跳过，不产生负坐标越界
        let tiny: WorkArea = (0.0, 0.0, 50.0, 40.0);
        let p = snap_position((10.0, 10.0), (ICON_SIZE, ICON_SIZE), tiny, SNAP_MARGIN);
        assert_eq!(p, (0.0, 0.0));
    }

    #[test]
    fn pct_roundtrip() {
        let pos = (727.0, 313.0);
        let pct = to_pct(pos, (ICON_SIZE, ICON_SIZE), W).unwrap();
        let back = from_pct(pct, (ICON_SIZE, ICON_SIZE), W);
        assert!((back.0 - pos.0).abs() < 1e-9);
        assert!((back.1 - pos.1).abs() < 1e-9);
    }

    #[test]
    fn pct_clamped_and_none_for_tiny_area() {
        let pct = to_pct((-50.0, 5000.0), (ICON_SIZE, ICON_SIZE), W).unwrap();
        assert_eq!(pct, (0.0, 1.0));
        assert!(to_pct((0.0, 0.0), (ICON_SIZE, ICON_SIZE), (0.0, 0.0, 60.0, 80.0)).is_none());
    }

    #[test]
    fn pct_uses_work_area_origin() {
        // 副屏（如 1920,0 起）：百分比相对工作区原点
        let work = (1920.0, 0.0, 1920.0, 1040.0);
        let pct = to_pct((1920.0, 0.0), (ICON_SIZE, ICON_SIZE), work).unwrap();
        assert_eq!(pct, (0.0, 0.0));
    }
}

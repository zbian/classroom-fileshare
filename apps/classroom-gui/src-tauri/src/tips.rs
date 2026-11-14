//! tips 气泡窗（Task 11：FR-G-02 / A2 / TC-10）。
//!
//! 独立小窗（主窗未开也能提醒）：启动预创建隐藏，首条到达才 show；
//! 定位依 icon 窗贴边翻转并跟随其移动；窗高随堆叠数 set_size（命中区=可见区）。
//! 数据源：M1 用 dev 演示事件（`--demo-tips=N` 或 tips.html#demo=N）；
//! T7 合并后由 SvcNotify::ShareReceived 走同一条 `push_tip` 入口。

use crate::state::AppState;
use crate::windows::{self, ICON_LABEL};
use serde::Serialize;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};

pub const TIPS_LABEL: &str = "tips";
pub const TIPS_W: f64 = 300.0;
/// 前后端共享布局常量（tips.css 同值），用于 Rust 侧高度钳制
pub const TIPS_MIN_H: f64 = 96.0;

/// 单条 tips 载荷。M1 为演示形态；T7 接入后由 ShareItem 映射出展示字段。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TipDto {
    pub id: String,
    /// "file" | "text"
    pub kind: String,
    pub name: String,
    pub from: String,
    pub size_text: String,
}

/// 启动预创建 tips 窗（隐藏）。尺寸随后端堆叠数调整，初值取单卡高度。
pub fn ensure_tips_window(app: &AppHandle) -> tauri::Result<()> {
    if app.get_webview_window(TIPS_LABEL).is_some() {
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, TIPS_LABEL, WebviewUrl::App("tips.html".into()))
        .title("课堂共享 · 提醒")
        .inner_size(TIPS_W, TIPS_MIN_H)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(false)
        .resizable(false)
        .focused(false) // 被动通知，绝不抢焦点
        .visible(false) // 首条到达才 show
        .build()?;
    // tao 创建期对 undecorated+shadow 组合会预扩窗口（见 windows.rs 注释），建窗后归一
    win.set_size(LogicalSize::new(TIPS_W, TIPS_MIN_H))?;
    Ok(())
}

/// 推送一条 tips（服务通知与演示共用入口）。
/// 数据永远下发；「暂停接收提醒」只抑制窗口显示，不影响接收（PRD 4.3）。
pub fn push_tip(app: &AppHandle, tip: TipDto) {
    if let Some(w) = app.get_webview_window(TIPS_LABEL) {
        let _ = app.emit_to(TIPS_LABEL, "svc://share-received", tip);
        let paused = app
            .state::<AppState>()
            .config
            .lock()
            .map(|c| c.icon.paused_tips)
            .unwrap_or(false);
        if !paused {
            let _ = w.show(); // 不 set_focus：被动通知不抢焦点
            position_tips(app);
        }
    }
}

/// tips 定位：icon 旁弹出（优先左侧，空间不足翻右侧），纵向与 icon 顶对齐并钳制在工作区内。
/// 仅在可见时生效；icon 拖动（Moved 防抖后）与高度变化时调用跟随。
pub fn position_tips(app: &AppHandle) {
    let Some(tips) = app.get_webview_window(TIPS_LABEL) else {
        return;
    };
    if !tips.is_visible().unwrap_or(false) {
        return;
    }
    let icon = match app.get_webview_window(ICON_LABEL) {
        Some(w) => w,
        None => return,
    };
    let Ok(Some(mon)) = icon.current_monitor() else {
        return;
    };
    let scale = mon.scale_factor();
    let work = windows::work_area_logical(&mon);
    let Ok(pos) = icon.outer_position() else {
        return;
    };
    let Ok(size) = icon.outer_size() else {
        return;
    };
    let (ix, iy, iw) = (
        pos.x as f64 / scale,
        pos.y as f64 / scale,
        size.width as f64 / scale,
    );
    let Ok(ts) = tips.outer_size() else {
        return;
    };
    let th = ts.height as f64 / scale;

    let gap = 12.0;
    let x = if ix - gap - TIPS_W >= work.0 {
        ix - gap - TIPS_W
    } else {
        ix + iw + gap
    }
    .clamp(work.0, (work.0 + work.2 - TIPS_W).max(work.0));
    let y = iy.clamp(work.1, (work.1 + work.3 - th).max(work.1));
    let _ = tips.set_position(LogicalPosition::new(x, y));
}

/// 前端在堆叠数变化后同步窗高（命中区=可见区）。
pub fn update_tips_height(app: &AppHandle, h: f64) -> Result<(), String> {
    let Some(w) = app.get_webview_window(TIPS_LABEL) else {
        return Ok(());
    };
    let h = h.clamp(TIPS_MIN_H, 420.0);
    w.set_size(LogicalSize::new(TIPS_W, h))
        .map_err(|e| e.to_string())?;
    position_tips(app);
    Ok(())
}

/// 队列清空：前端通知后隐藏（tips 不落盘，PRD 4.2）。
pub fn tips_dismissed(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(TIPS_LABEL) {
        let _ = w.hide();
    }
}

/// dev 演示：连发 n 条（350ms 间隔），用于验证堆叠/合并/倒计时（仅 debug 构建）。
#[cfg(debug_assertions)]
pub fn demo_tips(app: &AppHandle, n: u32) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        for i in 1..=n {
            let is_file = i % 2 == 1;
            let tip = TipDto {
                id: format!("demo-{i}"),
                kind: if is_file { "file" } else { "text" }.into(),
                name: if is_file {
                    format!("演示文件 {i}.pdf")
                } else {
                    format!("演示通知 {i}：请完成练习")
                },
                from: "教师".into(),
                size_text: if is_file {
                    "2.4 MB".into()
                } else {
                    "128 字".into()
                },
            };
            push_tip(&app, tip);
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    });
}

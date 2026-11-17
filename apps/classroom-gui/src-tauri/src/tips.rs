//! tips 气泡窗（Task 11：FR-G-02 / A2 / TC-10）。
//!
//! 独立小窗（主窗未开也能提醒）：启动预创建隐藏，首条到达才 show；
//! 定位依 icon 窗贴边翻转并跟随其移动；窗高随堆叠数 set_size（命中区=可见区）。
//! 数据源：`ipc_client::forward_loop` 收到 `SvcNotify::ShareReceived`
//! （及覆盖态的 `ItemUpdated`）后映射为 [`TipDto`] 并调用 [`push_tip`]；
//! dev 演示（`--demo-tips=N`）走同一入口。本模块只负责展示与窗口行为。

use crate::state::AppState;
use crate::windows::{self, ICON_LABEL};
use serde::Serialize;
// 仅 dev 演示（cfg(debug_assertions)）使用；release 下不引入（release
// clippy 守卫：M1 收尾修复 unused import 盲区）
#[cfg(debug_assertions)]
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder,
};

pub const TIPS_LABEL: &str = "tips";
pub const TIPS_W: f64 = 300.0;
/// 前后端共享布局常量（tips.css 同值），用于 Rust 侧高度钳制
pub const TIPS_MIN_H: f64 = 96.0;

/// 单条 tips 载荷（展示字段）。生产由 `ipc_client::tip_from_share_item` 从
/// `ShareItem` 映射而来（P0 #79 接线完成）；dev 演示构造同等形态，二者共用
/// [`push_tip`]。
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

/// 是否显示 tips 窗口（暂停只抑制显示，不影响数据下发，PRD 4.3）。
/// 抽成纯函数便于单测断言「暂停语义没被改坏」。
fn tip_should_show(paused: bool) -> bool {
    !paused
}

/// 读取「暂停接收提醒」开关（gui.json）。
fn tips_paused(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .config
        .lock()
        .map(|c| c.icon.paused_tips)
        .unwrap_or(false)
}

/// tips 投递边界：生产实现走 Tauri 窗口；单测用记录器断言「暂停只抑制显示、
/// 数据照常下发」（PRD 4.3 的硬约束，reviewer 清单第 3 项）。
pub(crate) trait TipSink {
    /// 数据下发（永远执行）
    fn emit(&self, tip: &TipDto);
    /// 窗口显示（受暂停开关控制）
    fn show(&self);
}

/// 生产实现：真实 Tauri 窗口。
struct WindowSink<'a> {
    app: &'a AppHandle,
    window: tauri::WebviewWindow,
}

impl TipSink for WindowSink<'_> {
    fn emit(&self, tip: &TipDto) {
        let _ = self
            .app
            .emit_to(TIPS_LABEL, "svc://share-received", tip.clone());
    }

    fn show(&self) {
        let _ = self.window.show(); // 不 set_focus：被动通知不抢焦点
        position_tips(self.app);
    }
}

/// 投递一条 tips：**先 emit 数据，再按暂停开关决定是否 show**。
/// 顺序是本函数的核心契约（暂停期间数据仍进前端队列），勿改成提前 return。
pub(crate) fn deliver_tip(sink: &dyn TipSink, paused: bool, tip: TipDto) {
    sink.emit(&tip);
    if tip_should_show(paused) {
        sink.show();
    }
}

/// 推送一条 tips（服务通知与演示共用入口）。
pub fn push_tip(app: &AppHandle, tip: TipDto) {
    let Some(window) = app.get_webview_window(TIPS_LABEL) else {
        return;
    };
    #[cfg(debug_assertions)]
    eprintln!("[t11] push_tip: {}", tip.name);
    let sink = WindowSink { app, window };
    deliver_tip(&sink, tips_paused(app), tip);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// 记录型 sink：断言投递顺序与暂停语义（真实 push_tip 走同一 deliver_tip）。
    #[derive(Default)]
    struct RecordingSink {
        emitted: RefCell<Vec<TipDto>>,
        shown: Cell<u32>,
    }

    impl TipSink for RecordingSink {
        fn emit(&self, tip: &TipDto) {
            self.emitted.borrow_mut().push(tip.clone());
        }
        fn show(&self) {
            self.shown.set(self.shown.get() + 1);
        }
    }

    fn sample_tip() -> TipDto {
        TipDto {
            id: "id-1".into(),
            kind: "text".into(),
            name: "第三题的疑问".into(),
            from: "小林".into(),
            size_text: "86 字".into(),
        }
    }

    /// PRD 4.3 硬约束：暂停时**数据照常下发**、仅不显示。
    #[test]
    fn paused_still_emits_but_does_not_show() {
        let sink = RecordingSink::default();
        deliver_tip(&sink, true, sample_tip());
        assert_eq!(sink.emitted.borrow().len(), 1, "暂停时数据仍必须下发");
        assert_eq!(sink.emitted.borrow()[0].name, "第三题的疑问");
        assert_eq!(sink.shown.get(), 0, "暂停时不显示窗口");
    }

    /// 未暂停：数据下发 + 显示各一次。
    #[test]
    fn unpaused_emits_and_shows_once() {
        let sink = RecordingSink::default();
        deliver_tip(&sink, false, sample_tip());
        assert_eq!(sink.emitted.borrow().len(), 1);
        assert_eq!(sink.shown.get(), 1);
    }

    #[test]
    fn pause_only_suppresses_display() {
        assert!(tip_should_show(false), "未暂停：显示");
        assert!(!tip_should_show(true), "暂停：不显示");
    }

    /// 前端按 camelCase 消费 TipDto（sizeText/name/from），避免字段名漂移。
    #[test]
    fn tip_dto_serializes_camel_case_for_frontend() {
        let json = serde_json::to_string(&sample_tip()).expect("serialize");
        assert!(
            json.contains("\"sizeText\":\"86 字\""),
            "前端按 sizeText 消费：{json}"
        );
    }
}

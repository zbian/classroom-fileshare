//! 剪贴板读取（Task 12「粘贴并共享」入口，PRD 4.3/5.2）。
//!
//! tauri-plugin-clipboard-manager 只覆盖文本/图片，不含文件列表（CF_HDROP）——
//! 统一自写：一次 OpenClipboard 同时探测 CF_UNICODETEXT 与 CF_HDROP，
//! 返回 {text?, files?}（两者并存时前端优先文件，PRD 5.2「检测到文件走文件链路」）。
//! EX-03：OpenClipboard 被占用时短重试，仍失败返回占用文案（不修改剪贴板内容）。

use serde::Serialize;
use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::System::DataExchange::{
    CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// CF_ 常量直接取 WinUser 数值，避免跨模块路径差异（CF_UNICODETEXT=13, CF_HDROP=15）。
const CF_UNICODETEXT: u32 = 13;
const CF_HDROP: u32 = 15;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStateDto {
    /// 剪贴板文本（非文本格式时 None）
    pub text: Option<String>,
    /// 剪贴板文件列表（CF_HDROP；目录已过滤并计入 dirs）
    pub files: Vec<String>,
    /// 拖入/粘贴内容中的目录数量（目录暂不支持共享，EX 提示用）
    pub dirs: u32,
}

/// 打开剪贴板（EX-03：占用重试 3×80ms）。
unsafe fn open_clipboard_retried() -> Result<(), String> {
    for _ in 0..3 {
        if OpenClipboard(None).is_ok() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    Err("剪贴板被其他程序占用，请稍后再试".into())
}

/// 读取 CF_UNICODETEXT 全文。
unsafe fn read_text() -> Option<String> {
    if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
        return None;
    }
    let h = GetClipboardData(CF_UNICODETEXT).ok()?;
    let hglobal = HGLOBAL(h.0);
    let ptr = GlobalLock(hglobal) as *const u16;
    if ptr.is_null() {
        return None;
    }
    // 以 NUL 截断计算长度（GlobalSize 含额外字节）
    let mut len = 0usize;
    while *ptr.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(ptr, len);
    let s = String::from_utf16_lossy(slice);
    let _ = GlobalUnlock(hglobal);
    Some(s)
}

/// 读取 CF_HDROP 文件列表（目录单独计数，不进 files）。
unsafe fn read_files() -> (Vec<String>, u32) {
    let mut files = Vec::new();
    let mut dirs = 0u32;
    if IsClipboardFormatAvailable(CF_HDROP).is_err() {
        return (files, dirs);
    }
    let h = match GetClipboardData(CF_HDROP) {
        Ok(h) => HDROP(h.0),
        Err(_) => return (files, dirs),
    };
    let mut count = unsafe { DragQueryFileW(h, u32::MAX, None) };
    let mut buf = [0u16; 1024];
    while count > 0 {
        count -= 1;
        let n = unsafe { DragQueryFileW(h, count, Some(&mut buf)) } as usize;
        if n == 0 {
            continue;
        }
        let path = String::from_utf16_lossy(&buf[..n]);
        if std::path::Path::new(&path).is_dir() {
            dirs += 1;
        } else {
            files.push(path);
        }
    }
    (files, dirs)
}

/// 「粘贴并共享」入口：探测剪贴板内容（EX-03 占用文案原样返回前端）。
pub fn read_clipboard_state() -> Result<ClipboardStateDto, String> {
    unsafe {
        open_clipboard_retried()?;
        let text = read_text();
        let (files, dirs) = read_files();
        let _ = CloseClipboard();
        Ok(ClipboardStateDto { text, files, dirs })
    }
}

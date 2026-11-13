//! 课堂共享 GUI 入口（Task 1b 脚手架）。
//!
//! 当前仅初始化插件链 + 由 `tauri.conf.json` 静态配置创建单空白主窗；
//! Task 8/9/11 起改为程序化创建 `icon` / `main` / `tips` 三窗
//! （见 docs/plans/2026-11-13-m1-p0.md 与 GUI 技术方案）。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        // 单实例：重复启动聚焦既有实例（EX-12）。
        // T12 接入：argv 携带资源管理器右键多选文件路径时转发给主实例。
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        // 剪贴板读取（T9 粘贴文本 / T12 粘贴并共享）
        .plugin(tauri_plugin_clipboard_manager::init())
        // 用系统默认程序打开文件（T9 OpenItem）
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("classroom gui 启动失败");
}

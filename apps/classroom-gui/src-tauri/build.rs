//! GUI 构建脚本：tauri 标准初始化 + 构建口径防呆（M1 验收发现 O-1）。
//!
//! 背景：`tauri::is_dev()` = 「tauri crate 未启用 custom-protocol」，与 release/debug
//! **profile 无关**。因此裸 `cargo build --release` 仍按 dev 模式编译（tauri-codegen
//! context.rs：`dev && dev_url.is_some()` 时不内嵌前端资源），产物启动后去连
//! `tauri.conf.json > build.devUrl`，无 dev server 即白窗——这是 M1 验收中
//! 「裸 cargo build 产物图标不显示」的根因。
//!
//! 本脚本在 release + 未启用 custom-protocol 时给出构建期告警（cargo:warning 会
//! 出现在构建日志），配合 main.rs 的运行时提示，避免该故障被误判为产品缺陷。

fn main() {
    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    let has_custom_protocol = std::env::var_os("CARGO_FEATURE_CUSTOM_PROTOCOL").is_some();
    if is_release && !has_custom_protocol {
        println!(
            "cargo:warning=【构建口径】release 未启用 custom-protocol：前端资源不会内嵌，\
             产物启动需 dev server 否则白窗。请改用 scripts/build-gui.ps1 \
             （或 cargo build --release -p classroom-gui --features custom-protocol）。"
        );
    }
    tauri_build::build()
}

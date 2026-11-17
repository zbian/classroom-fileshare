# M1 验收 O-1：GUI 构建口径（裸 `cargo build --release` 白窗）根因与修复

- 发现：M1 单机验收（PR #75）——用裸 `cargo build --release -p classroom-gui` 的产物
  运行时图标/界面不显示，会误导验收者为产品缺陷。
- 修复：`feat/rc2-gui-build`（构建口径 + 脚本 O-3/O-4）。
- 状态：已修，证据见下。

## 根因（源码级）

`tauri` 的「dev 模式」判定**只看是否启用 `custom-protocol` feature，与 cargo profile 无关**：

| 环节 | 依据 |
| --- | --- |
| `tauri-build::is_dev()` | `crates/tauri-build/src/lib.rs:606` — 读 `DEP_TAURI_DEV`，注释明确「`true` when the `tauri` crate is compiled without the `custom-protocol` Cargo feature」 |
| 资源内嵌分支 | `tauri-codegen/src/context.rs:178` — `else if dev && config.build.dev_url.is_some() { EmbeddedAssets::default() }`，即 dev 且配置了 devUrl 时**不内嵌**，运行时走 devUrl |

本仓库 `tauri.conf.json` 配了 `build.devUrl = http://localhost:1517`（开发流必需），
因此裸 release 产物：dev 模式 → 不内嵌 → 连 1517 → 无 dev server → 白窗/无图标。

## 为何不采纳「`[features] default = ["tauri/custom-protocol"]`」

该写法会让 **所有** 构建（含 debug/`cargo tauri dev`）都进入非 dev 模式：

1. `cargo tauri dev` 不再连 dev server → 前端热更新/开发流失效；
2. `generate_context!` 在编译期就要求 `apps/classroom-gui/ui/dist` 存在
   （`context.rs:187` 目录不存在直接 panic）→ 新克隆仓库裸 `cargo check` 直接失败。

结论：feature 保持**非默认**（Tauri 官方模板口径），用专用构建脚本承载「正确命令」。

## 实际改动

| 文件 | 改动 |
| --- | --- |
| `apps/classroom-gui/src-tauri/Cargo.toml` | 新增 `[features] custom-protocol = ["tauri/custom-protocol"]`（非默认，附原因注释） |
| `apps/classroom-gui/src-tauri/build.rs` | release 且未启用该 feature → `cargo:warning=【构建口径】…`（构建日志可见） |
| `apps/classroom-gui/src-tauri/src/main.rs` | release 且 `cfg!(dev)` → 启动弹 MessageBox「构建口径提示」并给出正确命令；判定抽为纯函数 + 2 例单测 |
| `scripts/build-gui.ps1` | 新增：`pnpm build` + `cargo build --release --features custom-protocol`，产物自检 |
| `README.md` | 新增「构建与运行」章节（开发/生产/安装/卸载 + O-1 警告） |
| `scripts/uninstall.ps1` | O-3：注册表改用 PowerShell 原生 cmdlet + `-LiteralPath`（原 `reg.exe … 2>$null` 在 `$ErrorActionPreference='Stop'` 下会把原生 stderr 变成 NativeCommandError 抛错）；原生命令经 `Invoke-Quiet` 静默 |
| `scripts/install.ps1` | O-4：二进制解析增加 `..\target\release|debug` 候选；失败信息列出已查找路径并指引 `-SvcExe/-GuiExe` 与 `scripts\build-gui.ps1` |

## 验证（本机实测）

| 项 | 方法 | 结果 |
| --- | --- | --- |
| 裸 release 的构建期告警 | `cargo build --release -p classroom-gui` | ✅ 输出 `warning: classroom-gui@0.1.0: 【构建口径】release 未启用 custom-protocol…` |
| 裸 release 的运行时提示 | 运行裸产物 → 枚举可见窗口 | ✅ `cls=#32770 title='课堂共享 · 构建口径提示'`（不再静默白窗） |
| 正确构建：**无 dev server** 下图标显示 | 停掉 vite（1517 已关）→ 运行 `--features custom-protocol` 产物 | ✅ 0.48s 可见，蓝圆 2703 px（`waitvisible`） |
| 正确构建：主窗六层 UI | 双击图标 → 主窗 1040×719 → PIL 像素断言 | ✅ `primary-l=37543`、`primary=3063`、`white=248484`、`appgray=11685` |
| O-3 通配符安全删除 | 临时键 `HKCU:\Software\Classes\*\shell\__rc2_selftest` 建/查/删 | ✅ 0.03s 完成、无卡死（`-LiteralPath`） |
| O-4 提示文案 | 把 install.ps1 复制到无二进制的临时目录执行 | ✅ 抛错并列出 3 个已查找路径 + 显式参数与 build-gui.ps1 指引 |
| 脚本语法 | 4 个 `scripts/*.ps1` 经 `[Parser]::ParseFile` | ✅ 全部 0 错误（并统一补 UTF-8 BOM，PS 5.1 中文兼容） |

## 复现命令

```powershell
# 正确构建（生产/验收唯一推荐）
scripts\build-gui.ps1
# 等价手动
pnpm --dir apps/classroom-gui/ui build
cargo build --release -p classroom-gui --features custom-protocol
# 产物
target\release\classroom-gui.exe   # 约 12 MB，内嵌前端，无需 dev server
```

## 口径建议（给验收方案）

验收/分发**只**使用「生产构建」产物；若误用裸 `cargo build --release`，产物会弹出
构建口径提示（构建日志亦有告警），属构建方式问题，不计产品缺陷。

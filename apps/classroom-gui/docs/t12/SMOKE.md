# T12 冒烟记录：右键「粘贴并共享」与剪贴板读取（PRD 4.3/5.2，A3）

- 环境：Windows 11 · vite dev(1517) + debug exe + `classroom-svc --console`（teacher，自起自收）
- 方法：**WebView2 CDP 直证**（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`）
  在页面上下文 invoke Tauri 命令，走「前端→Rust→命名管道→服务→SQLite」完整真实链路；剪贴板内容由 pwsh `Set-Clipboard` 预置

## 三态结果（全过）

| # | 剪贴板态 | 断言 | 结果 |
| --- | --- | --- | --- |
| 1 | 文本 | `read_clipboard_state` → `{text:"T12 冒烟：右键粘贴并共享入口", files:[], dirs:0}`；`paste_and_share` → OK；主窗 E1 打开且 textarea 预填同文本；接收对象 chip=「全体学员」（教师端） | ✅ |
| 2 | 空 | `read_clipboard_state` → `{text:null, files:[], dirs:0}`；`paste_and_share` → `REJ:剪贴板中没有可共享的文本或文件`（PRD 8.3 EX-03 逐字） | ✅ |
| 3 | 文件（CF_HDROP） | 剪贴板含 2 文件 + 1 目录 → `read_clipboard_state` → `files:[T12冒烟二.txt, T12冒烟一.txt], dirs:1`（目录单独计数不入 files）；`paste_and_share` → OK；服务侧落库核对 `List` 返回 `T12冒烟二.txt|v1`、`T12冒烟一.txt|v1` | ✅ |
| 4 | 状态条真实链路 | 主窗状态条「后台服务 运行中·房间 XK-204·在线学员 28」（点「重试」后桥惰性重连自愈 EX-10） | ✅ |

截图：`01-paste-text-e1.png`（E1 打开 + 预填 + 接收对象）

## 实现要点

- `src-tauri/src/clipboard.rs`：一次 `OpenClipboard` 同时探测 CF_UNICODETEXT(13) 与 CF_HDROP(15)；
  EX-03 占用重试 3×80ms 后返回「剪贴板被其他程序占用，请稍后再试」；**从不写剪贴板**（PRD 4.3 要求）
- `paste_and_share`（icon 菜单动作，Rust 编排原子化）：文件→`SendFiles` 真链路 + toast；
  目录→「目录暂不支持共享（N 个目录已跳过）」；文本→开主窗 + `gui://paste-text` intent 预填 E1；
  空→EX-03 错误文案（菜单据此置灰）
- icon 菜单三态：`openMenu` 时调 `read_clipboard_state` 决定「粘贴并共享」可用性，
  不可用时 hint 显示「剪贴板中没有可共享的文本或文件」（禁用态）
- 尺寸/类型校验由服务侧统一（>100MB/风险类型），GUI 只做入口归一（EX-04/EX-05 由服务 Err 文案透传）

## 待办（下回合）

- ~~拖放入口（文件）~~ 已提交（windows.rs Drop→SendFiles + 目录提示）
- ~~HKCU 右键扩展~~ 已提交（scripts/register-context-menu.ps1，实测 roundtrip 0.17s；
  **关键坑**：键路径含 `*` 通配符，读写必须 `-LiteralPath` 否则遍历 Classes 树长时间无响应）
- ~~argv 多选转发~~ 已提交并**实测**（见下）
- 卸载清理与 T-13 `uninstall.ps1` 的右键键名对齐（转 T-13 负责）

## argv 多选转发实测（T-12 收尾）

方法：单实例已运行 → 第二次以 `classroom-gui.exe <文件1> <文件2>` 启动（模拟资源管理器
右键多选调起，HKCU 注册项 command 为 `"...exe" "%*"`）。

| 项 | 结果 |
| --- | --- |
| 第二次启动进程立即退出（EX-12 单实例语义） | ✅ `second exited=True`，仅保留原 pid |
| argv 文件经 single-instance 回调转发 → `SendFiles` 真链路落库 | ✅ sqlite 实证 `右键冒烟一.txt|v2`、`右键冒烟二.txt|v2`（同名覆盖 version+1 亦顺带验证） |

## 已批准的降级项（Deferral → #13 备注）

**Unicode 文本拖放延后 M2**（lead 2026-11-17 裁定）。依据：

- PRD 第 12 章：拖拽投放整体属 **P1**（M2），不在 M1 P0 交付范围
- M1 P0 的文本共享入口已有两个可用：右键图标「粘贴并共享」、主界面「+ 粘贴文本」
- 完整实现需自写 COM `IDropTarget`（`RegisterDragDrop` 于 icon HWND + `CF_UNICODETEXT` 解析，
  约 150–200 行，含 COM 引用计数与线程套间管理），风险/收益不划算
- M1 已实现的拖拽能力：**文件拖放**（Tauri 内置 DragDrop → `SendFiles` 真链路）✅

M1 文本入口矩阵（F2 规则速查口径）：右键粘贴 ✅ / 主界面 + 粘贴文本 ✅ / 选中文本拖到图标 ⏳ M2。


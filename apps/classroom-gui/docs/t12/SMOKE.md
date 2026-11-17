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

- 拖放入口（Unicode 文本 OLE + 目录提示）——文件 DragDrop 已接 SendFiles 链路
- HKCU 右键扩展（脚本已写：`scripts/register-context-menu.ps1`，含 `-Remove`；实测注册/清理/重注册 roundtrip 0.17s，
  **关键坑**：键路径含 `*` 通配符，读写必须 `-LiteralPath` 否则遍历 Classes 树长时间无响应）
- argv 多选转发单实例（`share_files_from_argv` 已实现，待右键盘点后联测）

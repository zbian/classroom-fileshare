# T8 手动测试清单：GUI 浮动图标窗口（FR-G-01/03/04，A1/A3）

- 分支：`feat/t8-icon-window`（基于 `feat/t1b-gui-scaffold` @ 556451e）
- 环境：Windows 11 · 单显示器 2560×1440 @100%（\\.\DISPLAY11）· WebView2 · `pnpm dev` + `target/debug/classroom-gui.exe`
- 方法：Win32 自动化（EnumWindows/GetWindowRect/GetWindowLong/SetCursorPos/mouse_event）+ GDI 截图 + PIL 像素分析（蓝圆定位 `(37,99,235)±14`）

## 已验证项（自动实测）

| # | 项目 | 结果 | 证据 |
| --- | --- | --- | --- |
| 1 | 冷启动 ≤3s 图标可见（PRD 9.1） | ✅ 实测 0.33s / 0.66s / 0.86s（三轮） | `waitvisible` 轮询 + 蓝圆像素 |
| 2 | 常态窗口 72×72，命中区=可见区 | ✅ rect=72×72 | GetWindowRect |
| 3 | always_on_top（TOPMOST） | ✅ exstyle 含 WS_EX_TOPMOST | GetWindowLong(-20) |
| 4 | 可拖动（drag-region） | ✅ 拖至 (60,700)/(40,700) 均跟随 | SetCursorPos+mouse_event 合成拖拽 |
| 5 | 距边 ≤12px 吸附 | ✅ 释放于 x=40（窗左 4px）→ 吸附至 L=0 | rect L=0；单测 5 例 |
| 6 | 位置按 monitor.name+工作区百分比持久化 | ✅ `gui.json` xPct=0.0 / yPct=0.503 与实测吻合 | 文件内容 |
| 7 | 重启位置恢复（同显示器） | ✅ 重启后 0.29s 回到 (0,664) | rect + 截图 04 |
| 8 | 双击打开主界面（懒创建+单例聚焦） | ✅ 双击后出现 1024×680(客户区) 主窗 | EnumWindows 出现第二 Tauri Window |
| 9 | 右键菜单展开（自绘 A3 六项） | ✅ 窗口扩至 304×252；白卡 47k px、禁用项灰字 863 px、退出红字 197 px、蓝圆 2703 px | PIL 分析截图 02/05 |
| 10 | 菜单边缘翻转（flipX） | ✅ 图标在右缘 x=2464：菜单向左展开（x=2464+72−304=2232 ✓） | rect 实测 |
| 11 | Esc / 失焦收起并精确复位 72×72 | ✅ Esc 后 rect=(2464,24) 72×72 | rect 实测 |
| 12 | 「退出课堂共享」仅退 GUI 进程 | ✅ 点击后进程消失（app.exit(0)），`gui.json` 完整保留 | Get-Process gone |
| 13 | 单元测试（吸附/百分比/JSON 往返） | ✅ 10 passed | cargo test -p classroom-gui |
| 14 | fmt / clippy -D warnings / pnpm build | ✅ 全绿 | CI 同口径 |

## 已知实现说明

- **tao 创建期尺寸膨胀修复**：tao 0.37 对 undecorated+shadow 组合在建窗时按 `calculate_insets_for_dpi` 预扩窗口（实测 72→136 宽）。已在本仓 `ensure_icon_window` 创建后 `set_size` 归一（windows.rs 内注释），并保留 debug 日志 `[t8] icon created: outer=…` 便于回归观察。
- skip_taskbar 在 Windows 由 tao 经 ITaskbarList 实现（exstyle 不含 TOOLWINDOW 属预期），任务栏不出现项。

## 待人工/后续任务验证项

| # | 项目 | 说明 |
| --- | --- | --- |
| A | 多显示器拖动与副屏恢复 | 本机单屏；机制已实现（monitor.name 匹配优先、缺失回主屏），需双屏环境人工复验（PRD 4.1：拖到副屏重启仍在） |
| B | 拖拽投放态 124px（A4） | DragDrop 事件链路已接（Enter/Leave/Drop→放大/复原），需真实文件拖入人工验证 + 截图（T12 发送链路联动） |
| C | 任务栏无图标（肉眼） | exstyle 不可直接判定，请肉眼确认 |
| D | 「暂停接收提醒」服务联动 | 本任务为本地开关占位（gui.json 持久化），T9/T13 接 `PauseTips` |
| E | 退出后后台服务仍存活 | M1 无服务可退；T13/TC-12 完整验证 |
| F | 全屏放映遮盖后重新唤起 | EX-01：F1「上课常驻」开关 M1 后期接 |

## 截图索引（本目录）

1. `01-startup-default.png` — 默认位置（主屏工作区右上角 24px 边距）
2. `02-context-menu.png` — 右键菜单（右缘 flipX 向左展开，六项：3 禁用占位/打开/暂停提醒/退出红字）
3. `03-snapped-left.png` — 拖拽后左缘吸附（L=0）
4. `04-restored-after-restart.png` — 重启后位置恢复 (0,664)
5. `05-menu-open-right.png` — 左缘时菜单向右展开

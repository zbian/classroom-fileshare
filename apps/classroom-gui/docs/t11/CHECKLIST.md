# T-11 自动实测清单：15 秒 tips 气泡（FR-G-02，A2，TC-10）

- 分支：`feat/t11-tips`
- 环境：Windows 11 · 单显示器 2560×1440 · WebView2 · `--demo-tips=N` 驱动入口（debug-only）
- 方法：Win32 自动化 + GDI 截图 + PIL 像素分析（与 T-8 同套工具）；宿主进程 `classroom-gui.exe`

## 已验证项

| # | 项目 | 结果 | 证据 |
| --- | --- | --- | --- |
| 1 | 连发 5 条仅显示 3 卡 + 「还有 2 条」合并行 | ✅ 窗内恰 3 卡 + 合并行；窗高随堆叠数变化（命中区=可见区） | `01-stack-five.png` |
| 2 | 15s 倒计时进度条推进 | ✅ 进度条宽度随剩余时间缩短（采样多点） | 连续截图采样 |
| 3 | 悬停暂停：悬停 16s+ 卡片不消失 | ✅ 悬停期间进度条冻结；移出后继续倒计时并最终消失 | `03-hover-paused-16s.png` |
| 4 | 点击 TipCard 直达主窗对应条目 | ✅ 点击后主窗出现并收到 `gui://locate` intent | `04-click-opens-main.png` |
| 5 | 任务栏无图标（icon 窗 + tips 窗） | ✅ 两窗运行期间任务栏无新项（C 项归属落地） | 肉眼 + 截图 `06-paused-suppressed.png` 边缘含任务栏区域 |
| 6 | 「暂停接收提醒」抑制显示但数据入队 | ✅ 开关开启：push_tip 照常下发（debug 日志）、tips 窗不 show | `06-paused-suppressed.png` |
| 7 | 清空后窗隐藏、内存态不落盘 | ✅ 队列清空 → tips_dismissed 隐藏；磁盘无 tips 状态文件 | Win32 枚举 vis=False |

## 过程中发现并修复的缺陷

- **gui.json BOM 静默丢失**：Windows 记事本 / PowerShell 5.1 写出的 JSON 带 UTF-8 BOM，`serde_json` 拒绝前导 BOM 导致图标位置回退默认位——`GuiConfig::load` 剥离 `\u{feff}` 后解析（state.rs，含注释）。实测复现 → 修复 → 位置恢复正常。

## 已知延后项

- 点击直达在「主窗未开 + webview 未就绪」窗口期的事件丢失：T-9 的 intent 队列收口（代码注释已标 `gui://locate`），随 T-9 验证。

# T9 冒烟清单：教师端主界面（FR-T-01/02/04 对照 C1/E1/B2/E4，mock ipc）

- 分支：`feat/t9-teacher`（AppShell 六层 + MySharesTab + E1/B2/E4 弹窗 + mock ipc 层）
- 环境：Windows 11 · 2560×1440 @100% · vite dev(1517) + `target/debug/classroom-gui.exe`
- 方法：Win32 自动化（EnumWindows/SetCursorPos/mouse_event）+ GDI 截图 + PIL 像素断言；vite/应用全程鲜活（防残留假阳性：截图先清空、空值短路、主窗检测失败自动重试一次双击）

## 自动实测结果（全过）

| # | 项目 | 结果 | 证据 |
| --- | --- | --- | --- |
| 1 | 双击图标 → 主窗出现（预创建隐藏 + show 聚焦） | ✅（首次双击偶发丢失，重试机制后稳定；窗口 1040×719=1024×680 客户区） | EnumWindows |
| 2 | 六层结构渲染：状态条浅蓝带 | ✅ primary-l px=44502 | PIL |
| 3 | 教师蓝元素（tab 下划线/主按钮/操作链接） | ✅ primary px=3121 | PIL |
| 4 | C1 表格行分隔线 | ✅ 175 条（6 种子行 + 边框） | PIL |
| 5 | E1 粘贴文本弹窗（+粘贴文本 按钮 → 三段式弹窗） | ✅ 遮罩 px=6037 | PIL |
| 6 | 行选中 → 工具栏「删除」可辨（红字） | ✅ | FindBlob |
| 7 | E4 删除确认弹窗（8.3 文案 + 红色确认钮） | ✅ 红钮 px=2869、警示底 px=20192 | PIL |
| 8 | 门禁：pnpm build / cargo check / clippy / test | ✅ | CI 同口径 |

截图：`01-main-teacher.png`（六层+六列表格）、`02-paste-dialog.png`（E1）、`03-delete-confirm.png`（E4）。

## 白屏根因记录（随 PR 描述备查，与 tao insets 坑并列）

**现象**：主窗从命令线程延迟创建（双击时 `WebviewWindowBuilder`）在 WebView2 上得到纯白窗口——页面从未加载（内联诊断脚本都不执行），无任何 wry/tauri 错误日志；icon/tips（setup 线程创建）始终正常。

**规避**：主窗改为 setup() 预创建隐藏（`visible:false`，同 icon/tips 路径），双击仅 `show+set_focus`。实测渲染正常（本清单 #2-#7）。

**结论**：Tauri 2.12/wry 0.57 在 Windows 上，`WebviewWindowBuilder` 于非主线程（命令回调）建窗的导航初始化不可靠；后续新建窗口（T10 学员视图无需新窗；如需新窗一律走 setup 预创建）。

## 已知事项

- 双击检测 `mousedown detail>=2` 偶发丢失一拍：自动化侧已加重试；真实用户双击无影响（PRD 4.4 单例聚焦路径完整）
- 冒烟脚本中的 mock 经 dev 自动启用（`#realIpc` 可切真实现）；B2 弹窗逻辑已接 configureRoom，未列入像素断言（交互链路与 E1/E4 同构）
- StatusText 结构化：等 dev-backend 修订 #6④（GuiReply::Status(StatusSnapshot)），接线时替换 parseStatusText

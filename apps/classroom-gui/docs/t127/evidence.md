# #127 重命名 GUI 真实链路证据（M2 #19 GUI 部分）

> 契约：#126（已合入 main `fa1c95e`）`GuiRequest::RenameItem { id, new_name }`；
> 教师侧 → HostCommand::Rename → store 改名 → 广播 `EventKind::Rename{new_name}`；学员端应用新名（2 秒内）。
> **服务端语义**：同名冲突 `NameTaken`（M2 拒绝、不静默覆盖）、`sanitize_file_name` 净化、风险扩展名拒绝。
> ⚠️ 该请求是**命令下行**（失败只记服务端日志、不回传错误）→ GUI 侧做「预检 + 提交后确认生效」两段兜底。

## 实现
| 文件 | 内容 |
| --- | --- |
| `ui/src/main/renameRules.ts` | 预检：空/`.`/`..`/尾随 `.`或空格/NTFS 保留名/非法字符 `<>:"\|?*`/控制字符（**C0 <0x20 + C1 0x7F–0x9F，与 Rust `char::is_control()` 对齐**）/风险扩展名（`RISKY_EXTS` 与 Rust 同清单）/同作用域同名 |
| `ui/src/main/dialogs/RenameDialog.tsx` | 预填当前标题并全选；行内错误展示；Enter 提交；成功后关闭 |
| `ui/src/main/store.ts` | `dialog.rename`、`renameItem()`：预检 → `RenameItem` → 轮询确认生效（≤1.5s）→ toast；未生效给出兜底错误 |
| `ui/src/main/App.tsx` | 工具栏「重命名」启用（教师 + 已选中 + 非待删除）；**学员端禁用**（后端 M2 拒绝）；渲染对话框 |
| `ui/src/main/views/MySharesTab.tsx` | 行内入口：教师行文件「打开/重命名」、文本「复制/重命名」；学员行保持只读 |
| `ui/src/shared/ipc/{types,client}.ts` | 契约镜像 `RenameItem`；mock 同步实现（dev 与真实语义一致） |

## 真实链路证据（release GUI + 隔离真实 svc + CDP 19232）
环境：隔离 `LOCALAPPDATA`/`PROGRAMDATA`；教师 svc（房间 RN-127/47841）+ 学员 svc（alice，`manual_addr` 本机）；
教师经真实 IPC 广播两个文件（`第一章课件.pdf` / `练习题.txt`），教师与学员端 List 均可见。

| 场景 | DOM 实测 |
| --- | --- |
| 入口 | 工具栏「重命名」enabled（选中行后）；对话框预填 `练习题.txt` |
| 同名冲突 | 行内错误「**该名称已存在，请换一个**」 |
| 非法字符 | 行内错误「**名称不能包含 < > : " \| ? \* 等字符**」 |
| C1 控制字符（U+0080） | 行内错误「**名称不能包含 < > : " \| ? \* 等字符**」（与 Rust `char::is_control()` 对齐） |
| 风险扩展名 | 行内错误「**该类型不允许共享（EX-05），请换一个扩展名**」 |
| 成功改名 | 列表 `['重命名后的课件_#127.pdf', '第一章课件.pdf']`；toast「**已重命名，正在同步到学员端**」；GUI 内生效 **201ms** |
| 学员端同步 | 学员 svc List：`重命名后的课件_#127.pdf`（rename 后 2s 内断言通过） |

日志佐证：
- 教师 svc：`rename-request item_id=…` → `rename item_id=… version=2`
- 学员 svc：`rename item_id=… new_name=重命名后的课件_#127.pdf version=2`

截图：`01-teacher-list-rename-entry.png`、`02-dialog-duplicate-error.png`、`03-rename-success.png`

## 口径说明
- 学员端**不提供**重命名入口（后端 M2 明确拒绝；按 PRD #19 范围为教师侧），ining 已禁用并给出提示。
- 「学生提交」条目（只读，PRD 5.5）无重命名入口；E14 修复后教师「我共享的」也不含学生提交。
- 服务端 NameTaken 不回传错误：GUI 预检覆盖可预知场景；并发竞态下由「提交后确认生效」兜底——\n  轮询超时提示「**未确认生效，请刷新查看**」（超时 ≠ 失败：可能已成功但广播/刷新延迟，避免「假失败」误报）。

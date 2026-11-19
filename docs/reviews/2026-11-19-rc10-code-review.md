# rc.10 代码评审记录（拖拽 / 粘贴 / 右键菜单 / 操作列）

日期：2026-11-19
评审方式：Agent Team 双评审（Rust/安装包 + 前端），只读评审；Lead 复核并修复。
代码基线：`a4e6afe..3f16b45`（rc.8/rc.9 修复），修复提交 `c9cc065`（rc.10），本轮补测/收口见 `rc.11`。

## 评审结论

| 评审 | 结论 | 数量 |
| --- | --- | --- |
| code-reviewer（Rust/安装包，task-1） | 需修改 | 1 Blocker + 6 Major + 4 Minor + 3 Nit |
| code-reviewer-2（前端，task-2） | 需修改 | 1 Blocker + 3 Major + 5 Minor + 5 Nit |

Lead 未直接采信结论，逐条对照代码核对后修复；对「`send_dropped_files` 已进 release 命令表」「路径穿越双层防护」等判断做了确认。

## Issue 与修复对照

| Issue | 级别 | 问题 | 修复 | 验证 |
| --- | --- | --- | --- | --- |
| #IKWO9P | Blocker | release 下投放态 124px 无入口 | 新增 release 命令 `set_icon_drop_state`，前端 dragenter/leave 驱动 | 命令双表注册；安装后手动拖拽复核（见下方限制） |
| #IKWO9Q | Major | 文件名被 `0-` 前缀污染，破坏同名覆盖 | 每文件独立子目录 `dir/{i}/{原名}`，标题保持原名 | 单测 `sanitize_strips_path_and_illegal_chars`；同名覆盖走既有服务端语义 |
| #IKWO9R | Major | base64 无上限，100MB 峰值约 700MB | 前端 `validateDroppedFiles`（100MB/文件、200MB/总量、50 个/次）+ Rust `validate` padding 修正后的精确长度估算 | 前端 15 用例（node:test）；Rust `decoded_len_*`/`validate_*` 单测 |
| #IKWO9S | Major | 拖放临时文件永不清理 | TTL 清理：发送前清 10 分钟前、启动清 1 小时前（不立即删，避免与服务端异步消费竞争） | `%TEMP%\ClassShareDrop` 校验脚本项；`stale_detection_honours_ttl` 单测 |
| #IKWO9T | Major | 目录名同毫秒冲突/部分写盘 | 目录名加原子自增；先全量解码再写盘，失败回滚 | `unique_dir_names_differ` 单测 |
| #IKWO9U | Major | NSIS 缺 MultiSelectModel | 注册 `MultiSelectModel=Player` | `verify-installer-registry.ps1` 检查 |
| #IKWO9V | Major | UAC 提权时 HKCU 落到管理员 hive | 菜单与 Run 改 HKLM；卸载清理 HKLM + 旧 HKCU | 同上脚本检查 |
| #IKWO9W | Blocker/Major | onDrop 未排除输入框、无去重、dragleave 卡态 | `isEditableTarget` 统一判断；busy 去重；dragend + 1.2s 超时复位 | 前端单测 `isEditableTarget`；手动复核 |
| #IKWO9X | Minor | Op 不可键盘访问、禁用点击冒泡、纯函数无单测 | role/tabIndex/Enter/Space；禁用吞点击；`pasteTitle.ts` + `dnd.test.ts` 14 用例 | `node --test` 14/14 |

## 验证证据

- `cargo test -p classroom-gui`：43 passed / 0 failed（含 dnd 模块 6 个单测）
- `cargo clippy -p classroom-gui -- -D warnings`：通过
- `cargo fmt --all --check`：通过
- `pnpm build`（tsc + vite）：通过
- `node --test tests/*.test.ts`：14 passed / 0 failed
- 安装 rc.10：服务 Running；HKLM 菜单键（Default/Icon/MultiSelectModel/command）存在；HKCU 旧菜单已清理
- `scripts/verify-installer-registry.ps1`：全部 PASS（见发布记录）

## 未自动化项（如实记录）

- 真实鼠标拖拽悬停的 72→124 窗口动画：当前环境用 WinForms `DoDragDrop` 模拟会卡住拖放循环（已复位鼠标/键盘状态），未做自动化；代码路径为 `dragenter → invoke set_icon_drop_state(true) → windows::set_drop_state → set_position/set_size`。
- 资源管理器「多选 3 个文件」右键：注册表 `MultiSelectModel=Player` 已写入，但真机多选拖放需人工复核一次。

## 后续

- 若人工复核通过，关闭 #IKWO9P..#IKWO9X。
- 前端测试已用 Node 内置 test runner（零新增依赖），后续可平滑迁移 vitest。

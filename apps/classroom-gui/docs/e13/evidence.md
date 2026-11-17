# E13 修复证据：GUI 启动不再把自身 exe 当共享文件（issue #93）

## 根因

`apps/classroom-gui/src-tauri/src/main.rs::file_args()` 无 argv[0] 排除：
`std::env::args()` 首项 = 本进程 exe 路径，`is_file()=true` → 被当作待共享文件。
此前仅因 `.exe` 在服务端「高风险类型」清单被拦下（svc 每次启动打 WARN：
`教师文件广播失败 path=…\classroom-gui.exe error=高风险文件类型`），
**防御全靠下游拦截**——清单调整/产物改名即会把程序本体广播出去。

## argv 语义实测（两条入口）

| 入口 | 实测结果 |
| --- | --- |
| 进程启动 `std::env::args()` | 首项 = 本进程 exe 绝对路径 |
| 单实例回调（second-instance） | 本机实测 **argc=2**：`argv=["…\\classroom-gui.exe", "…\\E13证据文件.txt"]` → **同样包含 argv[0]**（插件传的是第二实例完整命令行） |

因此未采用 `skip(1)`：若插件将来只传附加参数，`skip(1)` 会吞掉第一个真实文件。
改为「排除等于本进程 exe 的参数」——对两种语义都安全，且**无法判定时保留
（`current_exe()` 失败 → 不丢弃该参数）**，符合「无法判定时保留而非丢弃」。

## 实现

- `is_self_exe(arg)`：与 `std::env::current_exe()` 比较
- `same_path`：优先 `canonicalize` 归一相对路径/`.`/`..`；失败退回规范化字符串
  比较（Windows 大小写不敏感）
- `file_args`：先排除自身 exe，再按「文件或目录」筛选（目录仍交给上层提示不支持）
- 单实例回调加 debug-only 日志（`[single-instance] argc=… argv=…`），便于回归核对

## 单元测试（gui 33 → 37，workspace 146 → 150）

| 用例 | 断言 |
| --- | --- |
| `file_args_excludes_self_exe` | argv[0]=current_exe → 结果为空 |
| `file_args_keeps_files_and_dirs_after_self_exe` | exe + 2 文件 + 1 目录 → 保留 3 项 |
| `self_exe_detected_case_insensitive_and_dot_segments` | 大写形式、带 `.` 段等价路径均识别为自身 |
| `file_args_handles_empty_and_missing_paths` | 空 argv / 不存在路径 → 空结果，不 panic |

## 真机证据（本机实测）

环境：隔离 `LOCALAPPDATA`/`PROGRAMDATA` 的 console 教师 svc + debug GUI。

| 验证项 | 结果 |
| --- | --- |
| GUI 启动后 svc 日志 | **`classroom-gui.exe` 出现次数 = 0**，`高风险` 告警 = 0（修复前每次启动必现 WARN） |
| GUI 再启动一次（复验） | 累计 exe 告警 **0**、高风险告警 **0** |
| 单实例第二实例带真实文件 | 第二进程立即退出（单实例语义）；日志 `argc=2 argv=[exe, E13证据文件.txt]`；svc 日志 `broadcast item_id=… file=E13证据文件.txt size=14 audience="all"` → 文件链路正常 |
| 拖拽 / 右键发送链路 | 不经过 `file_args`（各自独立入口），本轮无改动；`file_args` 多文件/目录用例覆盖 argv 路径不回归 |

## 影响面

仅 `apps/classroom-gui/src-tauri/src/main.rs`（+ 单测）；`crates/**`、`scripts/**` 零改动。
门禁：`scripts/local-ci.ps1` 六项 ALL PASS（含 workspace 150 passed、release clippy）。

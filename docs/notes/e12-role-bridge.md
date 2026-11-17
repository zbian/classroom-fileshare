# E12 修复：学员机 GUI 角色解析与桥重连（生产 P0，issue #83）

- 背景：qa 实测「按 `install.ps1 -Mode student` 正常安装的学员机 GUI 完全不可用」
  （状态「服务未运行」、列表空、无 tips）。
- 三处根因：① install.ps1 不写角色 → GUI 默认 teacher 连错管道；② `start_bridge`
  首连失败不重试（仅 UI 请求触发惰性连接）→ 常驻图标场景通知流永不建立；
  ③ `--role` 写入晚于桥启动 → 首连仍用旧角色。
- reviewer 补强：原方案写 `%APPDATA%\ClassShare\gui.json`，在「安装者≠登录用户」
  的管理员安装场景失效（APPDATA 是安装者账号的，与 E7 user_sid 同类坑）。

## 角色解析优先级（a > b > c > default）

服务启动时（`setup`，**先于 `start_bridge`**）解析一次并持久化：

| 优先级 | 来源 | 说明 |
| --- | --- | --- |
| 1 | `--role=teacher\|student` | 显式参数（安装脚本自启 / 调试） |
| 2 | **`svc.json` 的 `mode`** | 角色权威源：服务模式 `%PROGRAMDATA%\ClassShare\svc.json`（全员可读）、降级模式 `%LOCALAPPDATA%\ClassShare\svc.json`；**免疫安装者≠登录用户** |
| 3 | `gui.json` 的 `role` | 上次运行持久化 / 安装脚本辅助写入 |
| 4 | 服务管道探测 | 仅一个角色的管道存在时取之（`WaitNamedPipeW(…,0)`） |
| 5 | 默认 `teacher` | 历史行为兜底 |

桥连接阶段（`start_bridge`）另做**双管道兜底**：优先按当前角色尝试，失败则试另一角色；
握手成功即证明角色，若与配置不一致则纠正并持久化。重连为 1s→2s→5s 封顶的**无限退避**，
服务晚于 GUI 启动 / 重启后自动恢复，不依赖任何 UI 请求。

## 安装脚本（辅助，优先级最低）

`scripts/install.ps1`（T-13 领地，已报备）双保险：
- `-Mode student|teacher` → 写 `%APPDATA%\ClassShare\gui.json` 的 `role`（保留既有 icon
  字段、无 BOM）；HKCU Run 值带 `--role=<mode>`。二者仅在「未被 a/b 覆盖」时起作用。

## 验证

### 单元测试（gui 27 → 33，workspace 127 → 133）

| 用例 | 断言 |
| --- | --- |
| `resolve_role_precedence` | 五级优先级（含「诱导 gui.json=teacher + svc.json=student → student」） |
| `role_from_svc_json_reads_mode_with_bom_tolerance` | BOM 容错、非法 mode 跳过、候选顺序 |
| `apply_role_saves_only_on_change` | 仅角色变化才写盘 |
| `parse_role_arg_accepts_only_known_roles` | 仅接受 teacher/student |
| `backoff_is_one_two_five_capped` / `retry_until_connected_recovers_after_failures` | 1s→2s→5s 封顶；失败 2 次后成功 |
| tips 既有 13 例 | 回归 |

### 真实安装路径 E2E（本机实测，ALL PASS）

链路：`install.ps1 -Mode student`（隔离 `LOCALAPPDATA`、**诱导 APPDATA 写 role=teacher**）
→ 教师 svc（房间 XK-204）+ 学员 svc（`manual_addr=127.0.0.1:47821`，自动加入）
→ 学员 GUI（**不传 `--role`**）→ 教师经真实命名管道广播 → 学员 tips。

| 断言 | 结果 |
| --- | --- |
| install.ps1 生成学员 svc.json（mode=student） | ✅ |
| HKCU Run `ClassShareGUI` 含 `--role=student` | ✅ |
| GUI 日志 `[role] 解析为 student（来源 SvcJson）`——**诱导 gui.json 被权威源压过** | ✅ |
| GUI 日志 `[ipc] 服务管道已连接（role=Student）` | ✅ |
| 诱导 `gui.json` role 被纠正持久化为 student | ✅ |
| 教师侧真实 IPC 广播（外部客户端握手→SendText→`{'Ok':'Ok'}`） | ✅ |
| 学员 GUI tips 窗口 `vis=True`（300 宽）+ `[t11] push_tip: E12 冒烟通知` | ✅ |

证据：`e12-tips-real-install.png`（tips 截图）、`e12-gui.log.txt`、`e12-student-svc.log.txt`。

## 遗留

- 角色最终改由 svc `Status` 下发（替换 `--role` 占位与本次文件/管道推断）→ #16
- `start_bridge` 已单点负责连接（`ensure_client` 仅等待就绪），消除双连接/双
  forward_loop 竞态（#16「单飞防 tips 双弹」一并收敛）

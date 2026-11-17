# M1（P0 最小闭环）验收执行方案 —— TC-01 / 02 / 03 / 06 / 11 / 12

| 项目 | 内容 |
| --- | --- |
| 文档 | M1 验收执行方案（先成文备用，代码就绪后按此执行） |
| 作者 | qa-acceptance |
| 日期 | 2026-11-13 |
| 依据 | [PRD V1.0](../../../final_draft.md) 第 2.4 / 4 / 5 / 6 / 9 / 11 章；[M1 实施计划](../plans/2026-11-13-m1-p0.md)（Task 14）；[协作流程](../DEVELOPMENT.md) |
| 状态 | v1.3.8（E3/E4/E5/E6/E10/E11 已闭环；**E12 阻塞 v0.1.0 修复中**（§3.8）；tips 判据已由 rc.3 补测 ①–⑤ 验证（报告 §3.12）） |
| 跟踪 | issue **#15**（M1 验收执行，本方案的任务 issue）；T-13 取证参数回写要求见 issue **#14** 验收标准 |
| 产物 | 执行后产出 `docs/acceptance/M1-report.md` + `docs/acceptance/evidence/M1/` 证据目录（跟踪 issue #15） |

> 判定口径总原则：每条结论必须可追溯到 PRD 用例编号 / FR 编号与实际证据（截图、命令输出、日志、录屏）。凡与 PRD 文字冲突的实现表现，一律按缺陷登记，不做口头放行。

---

## 1 目的与范围

- 对 M1（P0）交付物执行 PRD 第 11 章的 6 个用例：TC-01、TC-02、TC-03、TC-06、TC-11、TC-12。
- 顺带覆盖相关 FR 的验收标准中可同场景执行的负例与边界（重名、错 PIN、超限文件、风险类型、空剪贴板等），在报告中标注为「FR 扩展检查」。
- 不在本次范围：TC-04/05/08/09（M2）、TC-07/10/13（按 Lead 排期）、30 台真机压测正式执行（见 §5.2）。

## 2 用例 ↔ 需求 ↔ 指标 追溯表

| TC | 场景 | 主要 FR / 章节 | 关键量化判定 |
| --- | --- | --- | --- |
| TC-01 | 首次使用（建房间 + 设昵称 + 自动连接） | FR-T-01(5.1)、FR-S-01(6.1)、FR-G-01(4.1)、3.1 发现、2.4 步数 | 点「开始上课」后 ≤3s 学员端发现房间码；教师 1 步 / 学生 1 步；冷启动 ≤3s 图标；重名拒绝；重启后不再弹昵称窗 |
| TC-02 | 广播 10 MB 文件 | FR-T-03(5.3)、FR-G-02(4.2)、FR-G-04(4.4)、FR-T-06(5.6)、7.1/7.2、9.1 | 全员 ≤5s 收到 tips 且双击可打开；到达后 tips ≤1s；主界面 ≤1s；120 MB 拒绝；风险类型拦截 |
| TC-03 | 复制粘贴文本 | FR-T-02(5.2)、FR-G-03(4.3)、FR-S-04(6.4)、FR-S-06(6.6) | 128 字文本；全体学员出现同名条目；一键复制逐字一致；空剪贴板入口置灰；发送不清空剪贴板 |
| TC-06 | 同名覆盖 | FR-T-07(5.7)、7.3、2.4 覆盖正确性、3.5/3.6 | 各端仅 1 条、内容为第二版、version=2、恰好 1 份 .bak；无二次确认弹窗 |
| TC-11 | 开机自启（不登录落盘） | FR-G-05(4.5)、3.2 双进程、2.4 离线可用性、9.5 | 重启且不登录时文件仍落盘（时间戳早于登录）；登录后主界面立即可见；服务随开机启动 |
| TC-12 | 右键退出 | FR-G-03(4.3 退出规则)、FR-G-05(4.5) | 图标与窗口消失；服务仍 Running；数据完整；重启 GUI 数据完整并补见退出期间内容 |

## 3 全局验收环境（基准）

### 3.1 机器矩阵

| 角色 | 数量（目标） | 数量（最低门槛） | 要求 |
| --- | --- | --- | --- |
| 教师机 | 1 | 1 | Windows 11 x64，WebView2 就绪，有线接入 |
| 学员机 | 30（TC-02 全量口径） | 3（缩减门槛，见 §5.2） | Windows 10 与 11 混插（至少 1 台 Win10 x64 覆盖兼容性 9.3）；本地管理员权限可用 |
| 备用机 | 1 | 0 | 复跑失败步骤、隔离变量 |

- 单人本机可完成：TC-03 / TC-06 / TC-12 的核心路径（1 教师 + 1 学员共 2 台，或本机 + 1 台/1 VM）。
- 必须 2 台（真机或真机+VM）：TC-01 重名负例、TC-11（一台停留登录界面、一台当教师机）。

### 3.2 网络

- 同一 L2 网段、同一交换机（有线优先）。**禁用/避开**：VPN、跨网段路由、Wi-Fi AP 客户端隔离（会拦 UDP 广播）、来宾网络。
- 端口（M1 计划已锁定）：UDP 47820（beacon）、TCP 47821（服务）。防火墙已确认（#14 / e42dbd5）：install.ps1 幂等删建入站规则（**E1 已修复**，随 #44 合入）：`ClassShare Service TCP`（TCP/47821）仅 **teacher**；`ClassShare Discovery UDP`（UDP/47820）**两角色都建**（程序限定 classroom-svc.exe）。
- ★环境预检重点（已判定**产品缺陷 E1**，2026-11-15 Lead 确认）：学员机接收 47820 beacon 属入站流量，而 install.ps1 student 模式未建任何入站规则——默认防火墙（入站默认拒绝）下自动发现预计失败。已派 dev-backend 修复（student 模式补**程序限定**入站规则，uninstall 对应清理）。E1 修复后的验证口径（预检流程保留，作为修复验证手段）：
  1. 未动防火墙的学员真机直接安装（student 模式）→ 自动发现**开箱即用**（不依赖手工放行）；
  2. `Get-NetFirewallRule` 查验新增规则为**程序限定**（限定 classroom-svc.exe 程序路径），而非放宽端口全域；
  3. 执行 `uninstall.ps1` 后该规则被清除；
  4. 修复合入前如需临时手工放行继续测试，必须同步登记环境偏差（不得作为通过依据）。
- 环境自检命令（取证入报告）：

```powershell
ipconfig /all                 # 确认同网段
ping <教师机IP>               # 连通性
Get-NetFirewallRule -DisplayName "*ClassShare*" -ErrorAction SilentlyContinue  # 防火墙规则
```

### 3.3 账号与权限

- 教师机、学员机各有一个**本地管理员**账号（安装服务 `scripts/install.ps1` 需要提权）。
- TC-11 需要学员机的登录密码（重启后登录取证），且执行期间无人自动登录（关闭「自动登录」配置）。
- TC-11/TC-12 的安装路径（管理员权限注册服务）在**真机由 QA 执行**（开发环境无管理员权限，#14 已明确留给验收）。

### 3.4 安装方式与版本记录（M1 无安装包）

- M1 交付形态：仓库 release 产物 + Task 13（issue #14）的 `scripts/install.ps1`（注册服务 + SCM failure 自动重启 5s/5s/30s + 防火墙（TCP 47821 仅 teacher；UDP 47820 两角色，E1）+ GUI HKCU Run）与 `uninstall.ps1`。NSIS 安装包属 M3。
- 已确认安装参数（e42dbd5，QA 已读脚本核对）：
  - 教师：`install.ps1 -Mode teacher -Room <房间码> [-Pin <PIN>] -GuiExe <classroom-gui.exe>`；学员：`install.ps1 -Mode student -Nickname <昵称> -GuiExe <…>`。  - ★**学员安装不传 `-Room`（E9 方案 b，2026-11-17 口径变更）**：学员端 `svc.json` 的 `room_code=""` → `room_filter=None` → **自动连接首个发现的教师机**，对齐 PRD 2.4「学生 1 步（设昵称）」与 3.1 自动发现。**多教室场景**才需传 `-Room` 且与教师端一致（install 摘要行会提示）。`[-ManualAddr <ip>:47821]` 仅作 RK-04 备用（广播被交换机限制时）。
  - ★**`-UserSid`（E7 令牌分发，#49）**：服务模式下 GUI 需以登录用户身份读取 `ipc.token`，故 install.ps1 须写入 `user_sid`（`svc.json`）→ 令牌 ACL 双授予「服务账号 + 主登录用户」。缺省时自动探测活动控制台会话用户（`quser` → `Win32_UserAccount` 取 SID）；探测不到（裸机镜像未登录）则 WARN 并**跳过**，待首次登录后由服务 SessionChange 自愈。**验收要求**：安装须以目标登录用户身份执行（或显式传 `-UserSid S-1-…`），并核验令牌 ACL 同时含服务账号与该登录用户。
  - 服务名 `ClassShareSvc`、服务进程 `classroom-svc.exe`、GUI 进程 `classroom-gui.exe`（以打包产物名为准）。
  - **数据根（管理员安装）= `%PROGRAMDATA%\ClassShare\data`**（svc.json 在 `%PROGRAMDATA%\ClassShare\svc.json`）；日志 `<数据根>\logs\svc.log`。降级安装（无管理员）数据根 = `%LOCALAPPDATA%\ClassShare\data`。GUI 侧 `gui.json` / `profile.json` 仍在 `%APPDATA%\ClassShare\`。
  - `-GuiExe` 缺省时 **GUI 登录自启不注册**（TC-11 步骤 5 依赖它，安装时必传）——原为静默跳过，已判定**缺陷 E2**（2026-11-15），修复为**显式警告 + 安装摘要行明示「未注册」**；E2 合入后验收执行时若仍静默跳过即复验失败。HKCU Run 仅对执行安装的用户生效（机房多用户需逐用户或镜像配置，登记为部署注意事项）。
  - 降级模式观察项（#14 验收项）：无管理员安装 → HKCU Run 前台进程 `--console`、`%LOCALAPPDATA%` 数据根、明示局限（未登录不接收、崩溃不自动重启）——在备用机执行一次并取证，对照 FR-G-05 异常与边界。
- 验收执行前记录进报告：git commit / tag、构建命令、产物路径、安装日期与完整命令。#44 已合并：脚本从 main 取用（scripts/ 随 #44 入库）。
- ★★**GUI 生产构建必须用 `scripts/build-gui.ps1`，禁止裸 `cargo build --release -p classroom-gui`**（O-1 升格为强制口径，Lead/reviewer 2026-11-17）：
  - 原因（源码级）：`tauri` 的 dev 判定**只看是否启用 `custom-protocol` feature，与 cargo profile 无关**（`tauri-build::is_dev()` 读 `DEP_TAURI_DEV`）；本仓库 `tauri.conf.json` 配了 `build.devUrl`，故裸 release 产物为 dev 模式 → `tauri-codegen` **不内嵌前端资源** → 运行时连 devUrl → 无 dev server → **白窗/浮动图标永不显示**（实测 `icon_ready` 不触发，WebView2 子进程已起，极易被误判为产品缺陷）。
  - 正确命令：`.\scripts\build-gui.ps1`（内部执行 `pnpm build` + `cargo build --release -p classroom-gui --features custom-protocol`，并做产物自检）。脚本现会打印 "This binary embeds ui/dist and does not need a dev server."
  - 防呆（已入库）：`Cargo.toml` 增非默认 feature `custom-protocol`；`build.rs` 在 release 未启该 feature 时输出 `cargo:warning=【构建口径】…`；`main.rs` 在 release 且 `cfg!(dev)` 时启动弹 MessageBox 提示正确命令（含 2 例单测）。
  - 详细根因与不采纳「default feature」的原因：见 [`docs/notes/m1-o1-gui-build.md`](../../notes/m1-o1-gui-build.md)。
  - 验收要求：GUI 产物必须由 `build-gui.ps1` 生成（或等价显式带 `--features custom-protocol`），报告须记录所用命令；若图标不显示，**先核对构建口径**再判定缺陷。

### 3.5 计时与时钟同步（时延类指标的取证基础）

- 时间源优先级：① 各机服务日志 `<数据根>\logs\svc.log`（RFC3339 含毫秒，RUST_LOG 可调）② **各机文件落盘 mtime**（原子替换时刻，见下方「日志锚点实际清单」的降级口径）③ 样本机录屏画面时钟（Xbox Game Bar，`Win+Alt+R`）④ 秒表（最后手段，双人配合）。
- 执行前各机对时：`w32tm /resync`，并 `w32tm /query /status` 取证；无法对域/NTP 时，记录教师机与各学员机的 `Get-Date` 对拍偏差，计算 Δ 时校正（跨机 mtime 比对**强制**执行此校正，否则 Δ 不可信）。
- p95 口径：各学员机 Δ = (本机落盘时间) − (教师机广播时刻)；报告同时给出「全部节点 ≤5s」与「p95 ≤5s」两个结论（PRD 11 章 TC-02 用前者，9.1 用后者）。

> ⚠️ **日志锚点实际清单（QA 静态复核 3ab34b9 / t13 分支，2026-11-16）**：#14 约定的关键行 `broadcast / received / applied` 在当前实现中**尚不存在**（这三个词只作为 Rust 标识符 `AgentEvent::Received` / `MergeDecision::Overwrite(applied)` 与 GUI 事件名存在，非日志文案）。现有 info 级锚点仅：
> - host：`教师端启动`(room,port)、`学员加入`(nickname,session_id)、`学员离线`(nickname)、`旧连接存活，拒绝重名加入（EX-02）`、`旧连接无响应，已驱逐（昵称可重新加入）`、`教师端停止`
> - agent：`已加入`(addr)、`与教师机断开，稍后自动重连`
> - store：`text put` / 文件落盘等为 **debug 级**（默认 info 不输出，`RUST_LOG=debug` 可开）
>
> **E4 修复后的目标状态（Lead 已派单，字段格式已约定）**：host 补 `broadcast`、agent 补 `received` 与 `applied` 三条 **info 级**锚点，行内字段固定为 `item_id=`、`file=`、`size=`（字段名稳定，QA 按此解析）。
> - ★**行内无模块路径**：实现已 `.with_target(false)` 抑制 target 段（issue #14 评论区示例中的模块路径段以实际输出为准，忽略）。QA 解析**按「关键词 + 字段」匹配，不做整行模板匹配**，并容忍 tracing 的两种行内布局：① 消息在前 `<ts> INFO broadcast item_id=.. file=.. size=..` ② 字段在前 `<ts> INFO item_id=.. file=.. size=.. broadcast`；文件名含空格（如「第一课 讲义.pptx」）也能正确截取。
> - 解析式：`broadcast`/`received`/`applied` + `item_id=<uuid>` + `file=<文件名>` + `size=<字节>`（文本条目 `size` 为字符数）；
> - Δ 计算：`Δ_node = T(applied, 学员机) − T(broadcast, 教师机)`；「到达→落盘」间隔 = `T(applied) − T(received)`；
> - 采集命令见附录 A；脚本见 `docs/acceptance/scripts/collect-m1-evidence.ps1`（单机采集，输出机器可读 TSV 锚点段）与 `docs/acceptance/scripts/build-delta-table.ps1`（跨节点 Δ 汇总 + p95 + 判定 + 时钟偏移修正）。
>
> **应急降级口径（仅当 E4 未能赶在 M1 验收开始前合入时启用）**：以文件 mtime 代理（教师端副本 mtime 早于真实广播 → Δ 为**保守估计**；学员端 mtime = 原子替换落盘时刻，可脚本化批量采集）；「到达」与「落盘」不可区分 → FR-G-02「tips ≤1s」只能靠样本机录屏核对；报告须声明所用口径与保守性，不以代理口径冒充日志级精度。

### 3.6 证据规范

- 目录：`docs/acceptance/evidence/M1/<TC-编号>/`，命名 `TC-XX-<两位序号>-<短语>.png|md|log`。
- 每张截图/输出必须能看出：机器（教师/学员 N）、时间（任务栏时钟或录屏）、被测对象。
- 录屏（.mp4）与 >2 MB 的大文件不入库：本地留存，报告登记绝对路径 + SHA-256。
- 命令输出统一用文本粘贴进对应 `.md` 证据文件（不靠截图承载文字，便于检索）。

### 3.7 缺陷流程

- 发现缺陷：报告 Lead → Lead 建 `type/bug` issue（附我用例编号、复现步骤、证据路径）→ 修复合并后我按原步骤复验并在 issue 留言闭环。
- 分级建议：**阻断**（用例主判据不通过）/ **严重**（FR 验收标准不通过但主判据通过）/ **一般**（负例、边界、文案）/ **轻微**（视觉、日志）。
- ★协议行为的复验要求：**测试桩 ≠ 真实实现**。凡协议层行为（Ping/Pong 应答、帧编解码、事件幂等、重名准入等），复验必须在**真实端点组合**（真实 agent ↔ 真实 host）上验证，不得仅凭使用测试桩的用例通过即判绿——E3 即由此逃逸 CI（桩自动应答 Ping，真实 agent 未实现该行为）。

### 3.8 已登记缺陷与修复跟踪（评审期）

缺陷由 Lead 登记处置，QA 负责修复后复验并在本表闭环：

| 编号 | 缺陷描述 | 处置（Lead 派发） | QA 复验口径 | 状态 |
| --- | --- | --- | --- | --- |
| E1 | install.ps1 student 模式不建任何入站防火墙规则，默认防火墙下学员机收不到 47820 beacon，自动发现失败 | **issue #14**（T-13）：dev-backend 补**程序限定**入站规则；uninstall.ps1 对应清理 | §3.2 验证口径 1–3：开箱即发现 / 规则程序限定 / 卸载后清除 | **修复已推送**（feat/t13-svc @ 73304f7，待合并） |
| E2 | install.ps1 `-GuiExe` 缺省时静默跳过 GUI 登录自启注册，无任何提示 | **issue #14**：修复为显式警告 + 安装取证摘要行明示 | 不传 `-GuiExe` 安装：出现警告与摘要行「GUI 自启：未注册」；TC-11 正向路径传参后注册生效 | **修复已推送**（@ 73304f7，待合并） |
| **E3** | **重名存活探测误判活跃学员为僵死 → TC-01 重名拒绝主判据失效**：host `probe_or_evict`（`crates/host/src/hub.rs:207-228`，调用点 `:333`）向旧连接发 `Frame::Ping` 并等 ≤3s 的 Pong；但**真实 agent 不回 Pong**——`crates/agent/src/lib.rs` 自建读写循环（仅 import `encode_frame/read_frame`，未用 `net::session::run_session`），入站 `Frame::Ping` 落入 `:187` 的 `other => "agent 忽略该帧"` 分支（`:186` 仅忽略 Pong）。net 层的 Ping→Pong 自动应答（`crates/net/src/session.rs:162`）只被 host 使用 → 学员侧探测方向断裂 | **issue #37**（P0 / type/bug）：dev-backend 最小修（agent 补 Ping→Pong）+ 真实 agent↔host 集成用例（活连接拒绝/旧连接不被驱逐双用例）+ 双实现漂移根治评估（结论采 B：net 抽 client 会话运行器，M2 初） | ① 真实 agent（**非测试桩**）在线时，第二台同名学员被**拒绝**并提示「该昵称已被使用」，等待 ≤3s；② 旧学员**不被驱逐**、继续正常接收广播（在线人数不变）；③ 僵死场景（杀进程/断网）仍可驱逐并允许同名加入 | **修复已推送待复验**（feat/t6-agent @ 3ab34b9；QA 静态复核通过 + 自动化实测 10 轮零 flaky，见 §3.8.1） |
| **E4** | **TC-02 计时取证锚点缺失**：#14 约定关键行 `broadcast/received/applied` 在实现中不存在（QA 枚举全部 tracing 调用确认），Δ 取证被迫降级为文件 mtime 代理 | **已派单**（Lead 2026-11-16）：host 补 `broadcast`、agent 补 `received` + `applied` 三条 info 级锚点，行内字段固定 `item_id=` / `file=` / `size=`（字段名稳定，QA 按此解析）；reviewer-2 终审覆盖后进合并序列 | 修复后：日志直采 Δ（`Δ = T(applied,学员) − T(broadcast,教师)`），并与 mtime 代理交叉验证；E4 合入前按 §3.5 应急降级口径执行并声明 | **修复已合入 main**（73e17d3 / PR #39）；QA 真实日志演练通过，见 §3.9 |
| **E5** | `applied` 锚点仅含 `item_id=` + `version=`，**缺 `file=` / `size=`**（与 #14 约定格式 `item_id=/file=/size=` 的轻微偏差；Δ 按 item_id 对齐，不影响判定） | **Lead 定性（2026-11-16）：接受偏差，不修码**——applied 行不含 file/size 属约定，证据表以 item_id 为键，file/size 为冗余 | 已在 issue #14 评论留痕（applied 行不含 file/size 属约定 + 三条锚点真实行格式 + 解析口径） | **已关闭（约定达成）** |
| **E6** | 文本条目的 `size` 两侧语义不一致：teacher `broadcast size=9`（正文字符数，符 PRD 3.5）vs agent `received size=27`（文本正文字节数 UTF-8）；文件条目两侧均为字节，无此问题 | **Lead 定性（2026-11-16）：一般，派单修复**——agent `received` 锚点 `size=` 改用 `ev.item.size`，对齐 PRD 3.5「文本为字符数」；PR #48 → **已合并 main（3603f23）** | 修复后：文本条目的 teacher/agent `size` 两侧相等（均为字符数）；QA 重跑 §3.9 演练核对 | **已闭环**：QA 复验通过（2026-11-16，PR #48）——真实日志实测文本条目两侧均 `size=9`（字符数），文件条目两侧均 3145728（字节）；上游测试 EXIT=0 全绿；PR 新增断言把「字符数 5 / 正文实体 UTF-8 15 字节」固化。**已于 3603f23 合入 main** |
| **E7** | **IPC 令牌 ACL 限权从未生效（PRD 9.4 本地通信鉴权形同失效）**：`crates/ipc/src/token.rs` 的 `restrict_acl_to_current_user` 调用 `icacls <path> /inheritance:r "<USERNAME>:(R,W)"`——**缺 `/grant`**，icacls 报 `Invalid parameter "/inheritance:r"` 并返回 **code=87**（QA 已用真实服务二进制复现，服务日志同款告警）；令牌文件继承权限从未被移除（ACL 仍带 `(I)`）。实测正确写法 `icacls <path> /inheritance:r /grant "*<SID>:(R,W)"` → **EXIT=0 且限权生效**（ACL 仅当前用户 RW）。叠加：命名管道经 tokio `ServerOptions` 创建、**未设显式 DACL**（无第二道屏障）；服务模式数据根 `%PROGRAMDATA%\ClassShare\data` 继承 `BUILTIN\Users:(RX)`（QA 实测新建子目录同样继承）→ **任何本地非管理员用户可读 ipc.token**，再连管道并出示令牌即通过握手，取得 GUI 同等 IPC 权限（列/发/删课堂数据）。降级（`--console`）模式数据根在 `%LOCALAPPDATA%`（天然用户私有），暴露面低 | **issue #49**（P0 / type/bug / area/ipc）：① `/grant` + **SID**；② 失败**升级为启动错误（fail closed，Lead 裁定）**；③ 真实 icacls 单测；④ 管道 DACL 残余风险记录进 #14（M2 纵深防御）。**PR #50（9c1797d）** 另用 `CommandExt::raw_arg` 规避 std `arg()` 给含 `:` 参数加引号导致的第二层根因；追加**存量令牌幂等重限权**、`user_sid` 双授予（svc.json / install.ps1 自动探测+`-UserSid` / SessionChange 登录自愈）、`--console` 打印 `{e}`。**残余加固 `/remove:g`（清理非预期显式 ACE）经 Lead 裁定列为 M2 备案**（低危：显式 ACE 非旧版产生形态；令牌限权 + 双授予已闭合 M1 暴露面）——**注：windows-rs API 路径（7083a5f）因全量重建 DACL 已连显式 ACE 一并清除，该备案项已被覆盖，建议撤销（待 Lead 确认，见 §3.9.3）**——待并入 #14 的 M2 残余风险清单 | ① `icacls <数据根>\ipc.token` 退出码 **0** 且 ACL 仅当前用户（**无 `(I)`**）；② 服务模式下令牌文件对其他用户不可读；③ 管道连接仍须令牌握手通过；④ 限权失败时服务**拒绝启动**（fail closed）而非降级放行；⑤ **存量令牌必须重新限权**（升级场景） | **QA 复验 5/5 通过**（实测记录见 §3.9.2）；**待 #50 合并后关闭**（关闭证据已满足）；残余 `/remove:g` 加固 → M2 备案（Lead 裁定 2026-11-16） |
| **E9** | **学员端无 beacon 自动发现/自动连接**（agent 原仅支持 `manual_addr` 直连）：违反 PRD 2.4「学生 1 步（设昵称）」与 3.1「学员端自动发现并展示可加入的教师机」，属 P0 缺口（reviewer 全检发现） | **已修复并合入 main（e134b76 / PR #69）**，采用**方案 b**：`svc.json.room_code=""` → `room_filter=None` → **自动连接首个发现的教师机**；学员安装**不再需要 `-Room`**（多教室场景才传且须与教师一致，install 摘要行提示）；`manual_addr` 降为 RK-04 备用 | ① 未传 `-Room` 安装的学员机能自动发现并加入（单教室 1 步入班）；② 多教室/多教师场景传 `-Room` 后只连房间码匹配者；③ 无教师可发现时不崩溃、可重连 | **已修复 + 已合入（e134b76）**；QA 单机以 agent 自动发现链路复核（§3.9.4），**双机真实广播链路仍延后** |
| **E10** | **P0 缺口：单文件 100 MB 上限未生效**——修复前超限文件未被拒绝（PRD 5.3/9.4、EX-04「单文件不能超过 100 MB」形同虚设；QA 在 rc.1 前的验收中未覆盖该负例，属验收侧漏项，后由 reviewer 补检发现） | **已修复并合入 main（7eea251 / PR #78）**：store 与 host 双层落地上限 + 端到端拒绝；同批 PR #77 完成 O-1 构建防呆与 O-3/O-4 | ① 120 MB → 拒绝且文案为 EX-04 逐字「单文件不能超过 100 MB」；② 恰好 100 MB → **接受**（边界不误杀）；③ `.exe`/`.bat`/`.lnk` → 拒绝（EX-05 语义）；④ 拒绝后立即发小文件**成功**（不污染后续发送） | **已闭环（QA rc.2 补测通过，见报告 §3.11）** |
| **E11** | **P0 缺口：tips 生产路径未接线**——`push_tip` 在 release **无调用者**（唯一调用方 `demo_tips` 为 `cfg(debug)`），`ipc_client` emit `svc://notify` 而 `TipsStack` 监听 `svc://share-received` → **release 产物中 FR-G-02 永不触发「收到但从不提醒」** | **已修复并合入 main（1fea3d1 / #82）**：`SvcNotify::ShareReceived → tips::push_tip`，含同源双通知去重（`should_push_tip`）与本人条目不自弹（`owner_role == self_role` 排除） | ① 教师广播 → 学员 GUI tips 可见且**到达→可见 ≤1 s**；② UIA 三要素（名称/来源/类型）；③ 15 s 自动消失；④ 连发 5 条 → 最多 3 条 + 「还有 N 条」；⑤ 暂停提醒抑制显示但数据入库；⑥ 本人条目不自弹、同源重复通知不重复弹 | **已闭环（QA rc.3 补测 ①–⑤ 全通过，到达→可见 27 ms、消失 14.96 s，见报告 §3.12）** |
| **E12** | **P0 阻塞：按 install.ps1 安装的学员机 GUI 连不上本机服务**——① `install.ps1` 注册 GUI 自启**不传 `--role` 且从不写 `gui.json`**，GUI `role` 默认 `teacher` → 连 `classshare-svc-teacher`，而学员 svc 监听 `classshare-svc-student`；② `start_bridge` 首连失败**不重试**（`ensure_client` 惰性重连仅在有 UI 请求时触发）→ 常驻图标时通知流永不建立；③ `--role=` 写盘在 `start_bridge` 之后（时序竞争） | **issue #83（Lead 裁定阻塞 v0.1.0，dev-gui 修复中）**：install.ps1 写 GUI 角色（或 HKCU Run 加 `--role`）+ `start_bridge` 退避重连 + `--role` 时序前置 | ① 真实 `install.ps1 -Mode student` 安装 → GUI 自启后**自动连 `classshare-svc-student`**（日志 `服务管道已连接（role=Student）`）；② 状态/列表正常（无 EX-10「服务未运行」）；③ 教师广播 → tips 正常；④ 服务晚于 GUI 启动时，GUI 能自动重连（无需用户操作）；⑤ teacher 模式对称正确 | **修复中（阻塞 v0.1.0）**；rc.4 后由 QA 用真实安装路径复验 |
| **E13** | 一般：**GUI 每次启动把自身 exe 当作待共享文件**——`file_args()` 未跳过 `argv[0]`（exe 是文件），走真实发送链路；实测多轮共 12 条 `教师文件广播失败 … classroom-gui.exe 高风险文件类型`。目前仅被 EX-05 拦下，若类型限制变化会外发程序本体 | **已开 issue #93**（修复中，PR 随后；建议 `file_args` 加 `.skip(1)` + 单测） | — | **待修复（最新 main 仍存在）**，见报告 §3.14 |

### 3.8.1 QA 静态复核记录
| 检查点 | 结论 |
| --- | --- |
| student 模式建 UDP 47820 入站规则 | ✓ 两角色均建，且 `program="$svcExe"` 程序限定 |
| TCP 47821 仍仅 teacher | ✓ student 分支无 TCP 规则，摘要行明示「student 无 TCP 入站」 |
| 降级模式说明 | ✓ 摘要注明「无法配置防火墙 + manual_addr 兜底」 |
| E2 显式警告 | ✓ `-GuiExe` 缺省 → `Write-Warning`（文案含「GUI 登录自启未注册」） |
| uninstall 清理 | ✓ 幂等删除 TCP/UDP 两条规则 |
| 残留待真机复核 | 规则是否真正程序限定（`Get-NetFirewallRule`）、卸载后是否清除、学员机开箱发现 |

**E3（3ab34b9）——静态复核通过（待真机复验）**

| 检查点 | 结论 |
| --- | --- |
| agent 读循环是否真回 Pong | ✓ `crates/agent/src/lib.rs` 读循环新增 `Frame::Ping =>` 分支，`writer.write_all(&encode_frame(&Frame::Pong))`；与心跳分支同一 `writer`，`tokio::select!` 分支串行执行无并发写冲突 |
| 新用例是否用真实 agent 非桩 | ✓ `crates/agent/tests/agent.rs`：`start_agent` → `classroom_agent::run_agent`；`start_host_at_inner` → `classroom_host::run_host`——真实 agent × 真实 host，无 MiniAgent 桩 |
| 口径①（活连接同名被拒） | ✓ 用例 `live_agent_answers_probe_and_keeps_receiving`：同名第二真实 agent 收到 `DupNickname` 错误并致命退出 |
| 口径②（旧连接不被驱逐、仍收广播） | ✓ 同一用例：探测后教师广播「探测后广播」，旧 agent 仍收到并断言 `owner_id == "teacher"` |
| 口径③（僵死后同名可加入） | ⚠️ 用例 `killed_agent_frees_nickname` 覆盖的是 **abort（连接干净关闭 → host 感知断线直接释放昵称）**；**未被自动化覆盖**的是「静默死亡（TCP 仍在但不响应，无 FIN/RST，即合盖休眠/断网）」——恰是探测机制的设计目标 |
| 残余风险处置 | 口径③ 的静默死亡变体列入**真机复验**（§4.1）：强杀（`taskkill /F`，产生 RST）与挂起/断网（不响应）两种情形分别验证「≤3s 探测后驱逐、同名可加入」；并确认活连接不被误驱逐 |
| 根治方向 | 采方案 B（net 抽 client 会话运行器，M2 初）→ 真机复验时额外核对 agent 其余帧分支（Manifest/FileChunk/FileEnd）迁移后行为等价 |

**E3 独立验证（QA 实测，2026-11-16）**

| 项目 | 结果 |
| --- | --- |
| 验证环境 | 独立 worktree `.wt/verify-t6` @ 3ab34b9，Rust 1.100.0 / cargo 1.100.0（MSVC） |
| 核心 crate 全量测试 | `cargo test -p classroom-agent -p classroom-host -p classroom-net -p classroom-protocol -p classroom-store` → **EXITCODE=0**，agent 8 + host 9 + net 1/6/7 + protocol 25 + store 15 全绿（两轮） |
| E3 两条新用例 | `live_agent_answers_probe_and_keeps_receiving`、`killed_agent_frees_nickname` → 连续 **10 轮全通过，0 flaky**（`test result: ok. 2 passed` ×10） |
| 结论 | E3 修复静态复核通过，自动化覆盖口径①②③；口径③ 的「静默死亡」变体仍未覆盖 → 真机复验（§4.1）补测 |

**E4（已派单，修复中）——TC-02 计时取证锚点缺失（#14 约定参数与实现不符）**

| 项目 | 内容 |
| --- | --- |
| 缺陷 | #14 验收标准要求回写「关键行格式（broadcast/received/applied 含毫秒）」，但当前实现**无这三条日志**（QA 静态复核：host/agent 全部 tracing 调用枚举完毕，info 级仅 join/leave/start 与昵称探测相关文案；broadcast/received/applied 仅作 Rust 标识符存在） |
| 已约定修复格式 | 三条 info 级锚点：host `broadcast`、agent `received`、agent `applied`；行内字段 `item_id=` / `file=` / `size=`（字段名稳定，QA 按此解析，Lead 2026-11-16 锁定） |
| 影响（修复前） | ① TC-02 的 Δ 取证降级为**文件 mtime 代理**（教师端 mtime 早于真实广播 → Δ 保守估计；学员端 mtime = 原子替换落盘时刻）；② 「到达」与「落盘」不可区分 → FR-G-02「tips ≤1s」只能靠样本机录屏核对；③ 多节点 Δ 表精度受限，30 台 p95 正式压测（M3）需要日志级锚点 |
| QA 复验口径 | 修复后：① 三条锚点在 svc.log 中可见且含毫秒时间戳与 `item_id/file/size` 字段；② 按约定格式解析出 `broadcast`（教师）与 `applied`（学员）时间戳，计算 `Δ = T(applied) − T(broadcast)`；③ 与 mtime 代理值交叉验证（偏差应在合理范围内）；④ 「到达→落盘」间隔 `T(applied) − T(received)` 可分列 |
| 计划影响 | Lead 承诺 E4 在 M1 验收开始前合入 main → §3.5 日志级取证口径按原计划执行，**降级口径仅作应急备份** |
| 建议 | dev-backend 补三条 info 级 tracing 锚点（改动小、风险低，且属 #14 已约定范围） |

### 3.9 E4 真实格式演练（QA 实测，2026-11-16，origin/main @ 73e17d3）

在独立 worktree `.wt/verify-main` 以**真实 host + 真实 agent**（非桩）+ **真实 tracing sink**（复刻 apps/classroom-svc/src/logging.rs：文件+stderr 双写、`.with_target(false)`、EnvFilter 默认 info）跑一次真实广播（3MB 文件 + 文本），产出**真实日志**并用本方案脚本解析。实测结论：

| 项目 | 真实输出 | QA 影响与处置 |
| --- | --- | --- |
| 行内布局 | **消息在前、字段在后**：`<ts>  INFO broadcast item_id=… file=… size=… audience="all"`（同 `received` / `applied`；非锚点行如「教师端启动 room=… port=…」亦然） | 解析器两种布局均已兼容，实测命中 |
| target 模块路径 | 无（`.with_target(false)`） | 与 #14 更正一致，按关键词+字段匹配 |
| **时间戳时区** | **UTC**（`2026-11-16T03:52:57.646034Z`） | ★日志时间戳为 UTC，而文件 mtime 为**本地时区**（本地 UTC+8 时相差 8 小时）→ **两种口径绝不可混用**：日志级 Δ 用日志两端对比，mtime 降级口径两端都用 mtime；报告中须注明所用时间基准 |
| `applied` 锚点字段 | 仅 `item_id=` + `version=`，**无 file=/size=** | 与约定格式的轻微偏差；Δ 按 item_id 对齐，**不受影响**（登记 E5） |
| 文本条目 `size` 语义 | teacher `broadcast size=9`（=正文字符数，符 PRD 3.5）；agent `received size=27`（=文本正文字节数 UTF-8） | 同名字段两侧单位不同（文件条目两侧均为字节，无差异）→ 仅影响证据表 size 列（登记 E6） |
| 物理折行 | **文件内为完整单行**（实测 len=134/131/120）；此前观察到的「折行」是控制台视觉折行 | 解析器保留折行展开为防御性逻辑（续行以空白开头即并入上一条），不影响正确性 |
| 文件名净化差异 | teacher `broadcast file=` 取 `item.title`（=**净化后**文件名），agent `received file=` 取 `header.file_name`（**原始**文件名） | 文件名含非法字符时两侧 file 列可能不同；Δ 按 item_id 对齐不受影响，证据表以 item_id 为准 |
| 端到端链路 | 真实日志 → collect 脚本 → TSV → build-delta-table → Δ 与判定 | 实测 Δ=0.054s，与原始时间戳 57.700370−57.646034 精确吻合，判定 PASS/FAIL 逻辑正确 |

> 演练脚本（QA 本地，未入上游）：`.wt/verify-main/crates/agent/tests/qa_anchor_rehearsal.rs`——真实 host+agent、真实 sink 写 `svc.log`、真实广播文件与文本各一条。

### 3.9.1 真实服务二进制复跑（T-13 已合入 main @ 4e275b5）

以 `origin/main`（含 T-13）构建的**真实** `classroom-svc.exe`，以 `--console`（降级模式，无需管理员）真实启动教师端，产出**真实** svc.log：

| 检查项 | 实测结果 |
| --- | --- |
| 日志路径 | `%LOCALAPPDATA%\ClassShare\data\logs\svc.log`（降级模式，与 §3.4 约定一致；服务模式为 `%PROGRAMDATA%\ClassShare\data\logs\svc.log`） |
| 行格式 | 与 §3.9 演练**完全一致**（UTC 毫秒、消息在前、字段在后、无 target），解析脚本零改动即可解析 |
| 数据根布局 | `data\logs`、`data\repo`、`data\index.db`、`data\ipc.token` 均按文档生成 |
| 启动日志 | `前台降级模式启动（--console）` / `IPC 令牌已创建` / `服务核心就绪` / `IPC 服务监听中 pipe=\\.\pipe\classshare-svc-teacher` / `教师端启动 room=XK-204 port=47821` |
| **异常** | `WARN icacls 限权返回非零，由握手校验兜底 code=87` → **登记 E7**（§3.8）：令牌 ACL 限权因 icacls 语法错误从未生效 |


### 3.9.2 E7 修复复验（PR #50 @ acf4a72，QA 实测）

以**真实** `classroom-svc.exe`（PR #50 分支构建）以 `--console` 真实启动，**实测**而非纸面判定：

| 复验口径 | 实测结果 | 判定 |
| --- | --- | --- |
| ① 新建令牌：icacls 退出 0、ACL 仅当前用户、无 `(I)` | 删除令牌后真实启动 → 日志 `IPC 令牌已创建（ACL 仅当前用户）`；`icacls` 输出仅 `Bian:(R,W)`，无继承项 | ✅ 通过 |
| ② 非管理员不可读 | 同上（ACL 仅当前用户 RW） | ✅ 通过（服务模式待真机复核） |
| ③ 管道仍须令牌握手 | `classroom-ipc` 回环测试 `wrong_token_rejected_at_handshake` 通过 | ✅ 通过 |
| ④ 限权失败 fail closed（拒绝启动） | `bootstrap` 失败 → `std::process::exit(1)`；服务模式打印 `{e}` 含手工修复命令 | ✅ 通过（见下方「次要发现」） |
| ⑤ **存量令牌重新限权（升级场景）** | **QA 实测通过（修复后）**：预置「真实继承形态」的脆弱 ACL（`BUILTIN\Users:(I)(RX)` + SYSTEM/Administrators/用户继承项，与 `%PROGRAMDATA%` 同构）→ 修复版真实启动后 ACL **仅剩 `Bian:(R,W)`**，继承项与 Users 可读全部清除 → **升级到修复版的机器可自愈，PRD 9.4 满足**。（首版修复未覆盖：令牌已存在时提前 `return Ok(token)` 跳过限权，QA 曾实测 ACL 完全未变；已在 9c1797d 修正并补单测 `load_upgrades_legacy_weak_acl`） | ✅ **通过** |
| ⑥ 显式 ACE 残余（低危） | `/inheritance:r` 只清**继承**项，管理员手工授予的**显式** ACE 不被移除（QA 注入 `BUILTIN\Users:(R)` 显式 ACE 后仍残留）——非旧版产生的形态，属加固建议（可加 `/remove:g` 清理非预期 ACE） | 备案（不阻断） |

**修复建议（最小改动，已由 dev 于 9c1797d 采纳）**：`path.exists()` 分支在 `return Ok(token)` 前也调用一次限权（icacls 幂等，成本极低），即可自愈所有已安装旧版的机器；并补单测「存量令牌文件被重新限权」（`load_upgrades_legacy_weak_acl`，QA 复核该单测以**真实继承形态**构造脆弱 ACL——首版单测用显式 ACE 会被 `/inheritance:r` 意外通过，属测试缺陷，已由 dev 自纠）。

**次要发现（建议一并处理）**：`--console` 模式的 bootstrap 失败分支用 `let Ok(..) = .. else` **丢弃了原始错误**，只打印「svc.json 配置损坏，无法启动」——ACL 限权失败时也会打印这句，运维会查错方向（服务模式分支正确打印 `{e}`）。建议 console 分支改为打印 `{e}`。

### 3.9.3 E7 在 windows-rs API 路径下的复验（7083a5f，QA 实测）

E7 的 icacls 路线在 **GHA runner 上「退出 0 + 自报成功但 ACL 未变」**（外部进程路线在该环境不可信，run 68 raw 命令行直证）→ dev 落地**假设 3：icacls 全线退役，改 `windows-rs` API 直写**（`SetNamedSecurityInfoW` + `SE_DACL_PROTECTED` + `SetEntriesInAclW` **全量重建 DACL**）。QA 以**真实** `classroom-svc.exe` 复验：

| 复验项 | 实测结果 | 判定 |
| --- | --- | --- |
| 最恶劣形态自愈 | 预置「继承 `BUILTIN\Users:(I)(RX)` + **显式** `BUILTIN\Users:(R)` + SYSTEM/管理员/本用户继承项」→ 修复版真实启动后 DACL **仅剩** `SYSTEM:(R,W)`、`Administrators:(R,W)`、`Bian:(R,W)`，**继承项与显式 ACE 全部清除** | ✅ 通过（强于 icacls 路线） |
| 新建令牌 | 同上形态，ACL 同样仅含三类受托人 | ✅ |
| 本地测试套件 | `cargo test -p classroom-ipc` → **9 单测 + 3 回环全绿**（含 `acl_restriction_removes_inheritance_and_grants_only_current_user`、`load_upgrades_legacy_weak_acl`、`dual_grant_includes_user_sid`、`grant_for_session_rebuilds_dacl_for_sid`） | ✅ 本地（真实 NTFS）全绿 |
| CI 差异归因 | 上述测试用 `tempfile::tempdir()`（Temp 卷）→ 在 GHA runner Temp 卷上 ACL 写入语义异常；**本地实机 Temp 卷全部通过** | 结论：**CI 失败为 runner 卷环境特性，非产品缺陷**；真机（真实 NTFS）不受影响 |

★**M2 备案项影响**：`/remove:g`（清理非预期**显式** ACE）原为低危 M2 备案——API 路径因**全量重建 DACL**（`SetEntriesInAclW` 传空旧 DACL）**连显式 ACE 一并清除**，该加固项已被实现覆盖，**建议撤销 M2 备案**（请 Lead 确认）。

★**取证脚本的 Temp 依赖已消除**（按 Lead 预案）：`collect-m1-evidence.ps1` 临时辅助文件（w32tm 输出、Python 索引助手）改落**工作目录**（优先级 `-WorkDir` > `-OutFile` 所在目录 > 数据根 > 当前目录），不再使用 `%TEMP%`；QA 演练脚本的产出目录同样锚定仓库工作区卷 `target/qa-rehearsal/`。真机取证亦受益。

## 4 分用例方案

### 4.1 TC-01 首次使用 —— 建房间、设昵称、自动连接

**(a) 前置环境**

- 教师机 1 台、学员机 ≥2 台（重名负例需第 2 台；不足时用 1 台 VM 顶替，网络须桥接，见 (e)）。
- 两端均为「首次运行」状态：服务侧 `%PROGRAMDATA%\ClassShare\`（svc.json 与 data\）与 GUI 侧 `%APPDATA%\ClassShare\`（gui.json / profile.json）均已备份清空；服务已安装且 Running（重跑用例前同样清理）。
- 教师机与学员机接入同一交换机；学员机 GUI 未启动（或已退出）。
- 录屏工具开启于教师机与学员机 A。

**(b) 执行步骤（点击级）**

1. 教师机：双击桌面「课堂共享」→ 出现浮动图标（冷启动，记时：≤3s 出现，FR-G-01）→ 首次启动弹出「创建本节课」窗口（B2）。
2. 观察窗口：房间码已自动生成（记下其值与格式）；PIN 开关保持**关闭**；「允许学员自动发现」默认开启（截图）。
3. 点击「开始上课」。此刻记 T0（教师机录屏 + 服务日志 Started 事件时间戳）。
4. 学员机 A：双击「课堂共享」→ 首次运行弹出设置昵称窗口（B1），观察教师机与房间码出现在发现列表中，记 T1（T1−T0 ≤ 3s，FR-T-01）。
5. 学员机 A：输入昵称「张三」→ 点击加入（**单选场景无需选房间/输房间码**：E9 起 `room_code=""` → 自动连接首个发现的教师机，对齐 PRD 2.4「学生 1 步」；多教室场景才在安装时用 `-Room` 限定）。
6. 学员机 A 主界面状态区显示「已连接」与房间码（B3，截图）；教师机主界面在线人数 +1、服务状态正常（截图）。—— **TC-01 主判据完成点**
7. 学员机 B（FR 扩展）：同样进入昵称窗口，输入相同昵称「张三」加入 → 预期提示「该昵称已被使用，请换一个」（8.3 文案）且无法进入；改为「李四」后可进入。
8. （可选轮次，FR 扩展）教师重建房间并开启 PIN：学员用错误 PIN 加入 → 拒绝；正确 PIN → 加入。
9. 学员机 A：退出并重新启动 GUI → 不再弹昵称窗口，直接进入并自动重连（FR-S-01 验收标准，截图）。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 学员端显示已连接与房间码；教师端在线计数正确 | 全部满足 | TC-01 主判据 |
| 2 | 点击「开始上课」→ 学员端发现房间码 | ≤3 秒 | FR-T-01 |
| 3 | 首次配置步数 | 教师 1 步（开始上课）、学生 1 步（设昵称） | 2.4 |
| 4 | 冷启动图标出现 | ≤3 秒 | FR-G-01 / 9.1 |
| 5 | 重名昵称 | 第二台被明确拒绝、无法进入 | FR-S-01 / EX-02 |
| 6 | （如执行）错误 PIN | 无法加入 | FR-T-01 |
| 7 | 重启 GUI | 不再弹昵称窗，自动重连 | FR-S-01 |

★执行前置（阻断依赖）：第 5 项依赖 **E3 修复**（§3.8）——修复前重名拒绝语义在真实 agent 上失效（活跃学员被误判僵死并驱逐，同名反而被接受）。E3 未修复时本用例第 5 项判**不通过**，并连带影响 TC-02/TC-03（被驱逐学员不再收到广播）。

**(d) 需采集的证据**

- B2 创建窗口、B1 昵称窗口（含发现列表）、B3 已连接状态截图（教师机 + 学员机）。
- 教师机点击「开始上课」→ 学员机出现房间码的录屏（计时证据）；两端服务日志 beacon/join 时间戳文本。
- 重名拒绝提示截图；重启后直接进入主界面截图；`profile.json` 存在的命令输出。

**(e) 本机限制与替代**

- 重名负例需第 2 台学员机：可用 VM（Hyper-V **外部虚拟交换机/桥接**；Default Switch 的 NAT 不保证 UDP 广播可达），或重置学员机 A 的昵称数据后重跑（次选，等价性略弱）。
- PIN 负例需教师重建房间：与首轮共环境时注意清空两端数据或更换房间码，避免残留状态干扰。
- ★时延观察（#34 B 存活探测引入）：同名冲突时 join 拒绝不再瞬时，**预期 0–3s 内**返回「该昵称已被使用」；若旧连接尚未被教师机感知断开，同名重连也可能经历 ≤3s 探测后才被接受。记录实测值（PRD 对该项无时限要求，仅记录行为，不判缺陷）；「≤3s 发现房间码」指标走 beacon 发现链路，与 join 探测分离，不受此影响。
- ★E3 修复的真机复验步骤（§3.8.1 残余风险：静默死亡变体未被自动化覆盖）：
  1. 学员机 A 正常在线（教师机在线人数显示 +1）。
  2. 学员机 B 用同名加入 → 必须被拒（提示「该昵称已被使用」），A 仍在线、仍能收到教师广播。
  3. 学员机 A **静默死亡**（模拟合盖休眠/断网：挂起进程或拔网线，TCP 不断）→ 教师机应 ≤3s 探测后驱逐 → 同名 B 可加入成功。
  4. 学员机 A **强杀**（`taskkill /F`，产生 RST）→ 同名 B 可加入成功（立即或 ≤3s 内）。
  5. 取证：教师机 `svc.log` 中重名 join 的 info 行（活连接拒绝路径与僵死驱逐路径文案不同，各取一条）+ 在线人数截图。

### 4.2 TC-02 广播文件（10 MB，全员 5 秒）

**(a) 前置环境**

- 教师机 1 台 + 学员机 N 台（N=30 为全量口径；M1 实际执行按 §5.2 Lead 决策的规模），全部已按 TC-01 入班在线。
- 测试文件（执行前生成并记录 SHA-256，入报告附录）：
  - 主判据文件 1 个 ≈10 MB：优先用现成真实 PDF/PPTX；无则 `10mb.bin`（随机字节）打包 `payload10mb.zip`（随机数据不压缩，体积≈10 MB）。
  - 观察项文件：3 个小文件（多选右键）、120 MB 超限文件、`x.exe` 风险类型文件。
- 各机录屏/日志就绪；时钟已按 §3.5 校准。

```powershell
# 随机 10MB 与 120MB 测试文件（PowerShell，30 秒内）
$r=[Random]::new(42); $b=[byte[]]::new(10MB); $r.NextBytes($b); [IO.File]::WriteAllBytes("f10mb.bin",$b)
Compress-Archive f10mb.bin payload10mb.zip
fsutil file createnew big120.bin 125829120
Get-FileHash payload10mb.zip, big120.bin -Algorithm SHA256
```

**(b) 执行步骤（点击级）**

1. 教师机：资源管理器打开测试目录，选中 `payload10mb.zip`（单选）。
2. 右键 → 「课堂共享」（菜单显示「发送 1 个文件」，A4）→ 点击发送。记 T0 = 教师日志 `broadcast` 行时间戳（E4 约定格式；录屏同步）。
3. 各学员机：观察浮动图标旁 tips 气泡（名称、来源、类型、倒计时条，A2）；样本机（≥2 台）录屏记出现时刻 T_tip；各机以日志 `received`/`applied` 时间戳参与 Δ 计算。
4. 学员机：双击浮动图标 → 主界面 ≤1s 打开（FR-G-04）→「教师共享」Tab 出现该文件，状态「已下载」。
5. 学员机：双击该条目 → 系统默认程序打开（zip 即资源管理器展开视图；PDF 即阅读器），内容可读。
6. 抽样 ≥3 台（不足 3 台则全量）：`Get-FileHash` 对比教师端源文件，一致。
7. 观察项（入口等效性，FR-T-03）：教师把另一文件**拖到浮动图标**（图标放大为投放区）发送，学员正常收到。
8. 观察项（多选）：选中 3 个文件右键 → 菜单显示「发送 3 个文件」→ 全部到达。
9. 负例（FR-T-03/EX-04/EX-05）：拖入 120 MB 文件 → 拒绝并提示「单文件不能超过 100 MB」；拖入 `x.exe` → 「该类型不允许共享」；两者之后正常文件仍可发送。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 全员收到 tips 且双击可打开 | 每台 Δ = T(applied) − T0 ≤ 5 秒；打开成功 | TC-02 主判据 |
| 2 | tips 出现时延 | 到达后 ≤1 秒弹出（样本机录屏核对） | FR-G-02 |
| 3 | 主界面打开 | 双击图标后 ≤1 秒 | FR-G-04 / 9.1 |
| 4 | 内容完整性 | 抽样哈希全一致 | FR-T-03 / 3.1 |
| 5 | 120 MB / 风险类型 | 拒绝、提示文案正确、不影响后续发送 | EX-04 / EX-05 |
| 6 | 多选与拖拽入口 | 与右键入口等效 | FR-T-03 |
| 7 | （30 台口径时）p95 | ≤5 秒 | 9.1 |

**(d) 需采集的证据**

- 教师机：右键菜单截图、发送时刻录屏、服务日志 `broadcast` 行（E4 约定格式 `item_id=/file=/size=`）。
- 每台学员机：服务日志 `received` 与 `applied` 行（文本汇总成一张 Δ 表进报告）；样本机 tips 录屏与列表「已下载」截图、双击打开截图。
- 哈希比对命令输出（教师端 + 抽样学员端）。
- 负例提示截图 + 之后成功发送的日志行。

**(e) 本机限制与替代**

- **30 台真机本机不可得** → 执行口径见 §5.2（已批准方案 B：≥3 台真机/VM 全绿为功能门禁 + 多 agent 压测凑 30 连接看时延趋势，正式 30 台 p95 压测留 M3）。
- VM 跑学员端：必须桥接/外部虚拟交换机；NAT 网络下 UDP 广播不可达会假失败（先做 beacon 连通性预检）。
- 「双击可打开」依赖文件关联：测试文件类型须选目标机房镜像常见类型（PDF/ZIP），否则「无关联程序」会误报（那属于 FR-T-06 异常分支，另行记录）。

### 4.3 TC-03 复制粘贴文本

**(a) 前置环境**

- 教师机 1 台 + 学员机 ≥2 台（「全体」按在场规模，同 §5.2），已入班在线。
- 128 字标准文本（确定性构造，字符数可核对）：

```
课堂通知012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789测试结尾
```

（构造：`"课堂通知"`(4) + `"0123456789"`×12(120) + `"测试结尾"`(4) = 128 字符；执行时以确认框显示的字数为准，若显示口径 ≠128 记观察项。）
- 可选观察变体：含中英文、标点、空格、换行的混合 128 字文本各一份。

**(b) 执行步骤（点击级）**

1. 教师机：打开记事本，粘贴上述 128 字文本 → `Ctrl+A` 全选 → `Ctrl+C`（「任意界面复制」）。
2. 右键浮动图标 → 菜单出现，「粘贴并共享」为可点击态（非置灰）。
3. 点击「粘贴并共享」→ 弹出确认框（E1）：内容来自剪贴板、显示字数 128、标题必填；输入标题「课堂通知-第1版」→ 点确认发送。
4. 学员机：到达后 ≤1s 弹 tips；双击图标打开主界面 →「教师共享」出现同名文本条目，带「新」标记（D1，截图）。
5. 学员机：点击该条目进详情（E2）→ 点「复制」→ 打开新记事本 `Ctrl+V` → 与原文**逐字比对**（推荐：两端各存 `.txt` 后 `Get-FileHash` 比对 + 并排截图）。
6. 剪贴板不被清空（FR-T-02）：教师发送后立即在记事本 `Ctrl+V`，内容仍为原文。
7. 观察项（入口等效性，FR-T-02）：主界面「+ 粘贴文本」入口与「选中文本拖到浮动图标」入口任选其一再跑一轮，行为一致。
8. 负例（EX-03/FR-T-02）：清空剪贴板（复制一个文件夹图标/用 `Set-Clipboard -Value $null`）后右键 → 「粘贴并共享」置灰；纯空白内容/无标题 → 禁止发送。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 全体在线学员出现同名文本条目 | 全部满足 | TC-03 主判据 |
| 2 | 一键复制内容一致 | 哈希一致 / 逐字一致 | TC-03 主判据、FR-S-06 |
| 3 | 确认框行为 | 字数显示 128、标题必填、内容=剪贴板 | FR-T-02 |
| 4 | tips | 到达后 ≤1s；条目带「新」标记 | FR-G-02 / FR-S-04 |
| 5 | 剪贴板保持 | 发送后原内容仍可粘贴 | FR-T-02 |
| 6 | 空剪贴板置灰 / 空白禁止发送 | 全部满足 | EX-03 / FR-T-02 |

**(d) 需采集的证据**

- 原文与粘贴结果并排截图 + 两个 `.txt` 的哈希输出。
- 右键菜单可点击态、确认框（E1）、学员列表（D1）、详情与复制成功提示（E2）截图。
- 置灰态截图；两端口服务日志 send/receive 行。

**(e) 本机限制与替代**

- 无硬性限制：1 台教师机 + 1 台学员机（或本机+VM）即可完成核心路径；「全体」规模随 §5.2。
- 「任意界面」不可能穷举：覆盖记事本 + 浏览器各 1 次即可，其余界面记录为不逐项验证。

### 4.4 TC-06 同名覆盖与 1 份备份

**(a) 前置环境**

- 教师机 1 台 + 学员机 ≥1 台（目标全部在场机器），已入班在线。
- 同名两版文件（文件名相同、内容不同、可肉眼区分）：
  - `课堂讲义.docx`（V1：正文首页含大字标记 `VERSION-1`）
  - `课堂讲义.docx`（V2：含 `VERSION-2`）
  - 观察项：V3（含 `VERSION-3`）
  - 制作方式：同一 docx 复制两份、改首页标记文字后保存；分别记录 SHA-256。
- 预先在教师机与至少 1 台学员机确认数据根路径与 `index.db` 位置（向 dev 确认，见 §6）。

**(b) 执行步骤（点击级）**

1. 教师机：右键发送 V1 → 等全部学员收到（列表出现，状态已下载）。
2. 教师机「我共享的」：确认该条目存在，记当前 version（界面上如显示版本/时间则截图；版本号权威以 `index.db` 为准）。
3. 教师机：右键发送 V2（同名同目录覆盖源文件后选它）→ **预期无二次确认弹窗**直接覆盖（PRD 5.7）。
4. 教师机列表：仍只有 1 条「课堂讲义.docx」；若出现覆盖结果提示（E3「检测到同名文件，已直接覆盖…」）截图。
5. 学员端：≤5s 内列表该文件仍只有 1 条；双击打开内容为 **V2**（肉眼见 `VERSION-2`，截图）。
6. 磁盘取证（教师机 + 每台学员机，PowerShell）：

```powershell
$root = "$env:ProgramData\ClassShare\data\repo\teacher"   # 管理员安装数据根（降级安装为 $env:LOCALAPPDATA\ClassShare\data）
Get-ChildItem $root -Filter "课堂讲义*" | Select-Object Name,Length,LastWriteTime
Get-FileHash "$root\课堂讲义.docx","$root\课堂讲义.docx.bak" -Algorithm SHA256
```

   预期：恰好 `课堂讲义.docx` + `课堂讲义.docx.bak` 各 1 份；主文件哈希=V2，.bak 哈希=V1。
7. 索引取证（同机）：

```powershell
python -c "import sqlite3,os;p=os.path.expandvars(r'%PROGRAMDATA%\ClassShare\data\index.db');c=sqlite3.connect(p);[print(r) for r in c.execute('select id,title,version,deleted,sync_state from share_items').fetchall()]"
```

   （列名以 T-03 实际 `schema.sql` 为准；降级安装机器换 `%LOCALAPPDATA%\ClassShare\data\index.db`。）预期：该作用域仅 1 行、`version=2`、`deleted=0`。
8. 观察项（只留 1 份备份）：再发 V3 → 预期 version=3、.bak 被替换为 V2（仅 1 份）。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 教师端与各学员端列表仅 1 条该文件 | 全部满足 | TC-06 主判据 / 2.4 |
| 2 | 内容为第二版 | 打开可见 V2；主文件哈希=V2 | TC-06 主判据 |
| 3 | version = 2 | index.db 查询结果 | TC-06 主判据 / 3.5 |
| 4 | 恰好 1 份 .bak，且为 V1 | 目录列举 + 哈希 | TC-06 主判据 / 5.7 |
| 5 | 无二次确认弹窗 | 直接覆盖 | 5.7 |
| 6 | 学员端同步时延 | ≤5s 更新为 V2 | 参照 TC-02 口径 |
| 7 | （观察）三发后 | version=3、.bak=V2、仍仅 1 份 | 5.7「只保留最近 1 份」 |

**(d) 需采集的证据**

- 教师/学员列表（覆盖前后各一张）；学员打开 V2 的截图。
- 目录列举与双哈希命令输出（教师 + 每台学员）；`index.db` 查询输出。
- （E3）覆盖提示截图（如有）；两端口服务日志 upsert/覆盖行。

**(e) 本机限制与替代**

- 多台学员机时逐台敲命令成本高：用统一取证脚本一轮跑完收集输出（脚本放 `evidence/M1/TC-06/collect.ps1`）。
- 数据根已确认（§3.4）：管理员安装统一 `%PROGRAMDATA%\ClassShare\data`，collect.ps1 按该路径取数即可，降级安装机器按 `%LOCALAPPDATA%` 变体。无其他不可完成项。

### 4.5 TC-11 开机自启（重启且不登录仍落盘）

**(a) 前置环境**

- **2 台机器**：教师机 + 学员机（真机最佳；VM 可替但须桥接网络且 VM 宿主机≠学员角色）。
- 学员机：服务已按 `install.ps1` 安装、设为自启、当前 Running（截图基线）；已入班过一次（昵称已存）；**关闭 Windows 自动登录**；持有该机登录密码；手机（拍登录界面）。
- 教师机：已开课（房间沿用或重建，学员机重连依赖 beacon 自动发现）。
- 两机时钟已按 §3.5 校准（文件时间戳与登录时刻比对依赖它）。

**(b) 执行步骤（点击级）**

1. 学员机：`Get-Service ClassShareSvc` 确认 Running（截图）→ 开始菜单 → 电源 → 重启。
2. 等学员机到达 Windows 登录界面，**保持不登录**；手机拍摄带画面时间的登录界面照片（记录拍摄时刻）。
3. 教师机：发送 1 个小文件 `不登录测试.docx`（约 1 MB 即可，TC 未限定大小）；记 T0（服务日志）。
4. 等待 ≥30s（覆盖 beacon 周期与重连）。
5. 学员机：输入密码登录进入桌面 → 观察浮动图标**自动出现**（GUI 登录自启，记时）→ 双击图标打开主界面。
6. 「教师共享」列表出现 `不登录测试.docx` 且状态已下载（截图）→ 双击打开成功。 —— **TC-11 主判据完成点**
7. 取证（登录后在学员机执行）：

```powershell
Get-Item "$env:ProgramData\ClassShare\data\repo\teacher\不登录测试.docx" | Select-Object CreationTime,LastWriteTime
Get-WinEvent -FilterHashtable @{LogName='System';Id=7036} -MaxEvents 30 |
  Where-Object Message -match 'ClassShareSvc' | Select-Object TimeCreated,Message
# 日志锚点：E4 已合入 main（#39）：直接采集 broadcast/received/applied 三行（§3.5 格式）
Select-String -Path "$env:ProgramData\ClassShare\data\logs\svc.log" -Pattern "学员加入|学员离线|已加入"
```

   预期：文件 CreationTime 与日志接收时间**早于本次登录时刻**（证明未登录期间落盘）；System 日志 7036 显示服务开机即启动。
8. 快速回归变体（可选）：以「注销（sign out）」代替重启重跑一遍——同属无用户会话场景；**正式判据仍以重启路径为准**。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 不登录期间教师发送的文件落盘 | 文件时间戳、日志接收时间 < 登录时刻 | TC-11 主判据 / 2.4 |
| 2 | 登录后打开主界面立即可见并可打开 | 全部满足 | TC-11 主判据 |
| 3 | 服务随开机启动 | System 7036 时间 ≈ 开机时间，早于登录 | FR-G-05 / 3.2 |
| 4 | GUI 登录自启 | 登录后图标自动出现 | 3.2 / 9.5 |
| 5 | 全程无人工干预接收 | 学员机无人操作即完成接收 | 2.3 运维诉求 |

**(d) 需采集的证据**

- 重启前 `Get-Service` 截图；登录界面手机照片（含可辨时间）。
- 登录后列表与打开成功截图；文件时间戳、System 7036、服务日志行（无 E4 时取会话生命周期行）的命令输出（与登录时刻同框对比，可写进同一 `.md` 证据文件）。
- ★令牌 ACL 取证（E7/#49）：安装后与首次登录后各取一次 `icacls "<数据根>\ipc.token"`，应显示**双授予**（服务账号 + 主登录用户）、无继承项 `(I)`、无其他用户 ACE；若无 `user_sid` 则首次登录后由 SessionChange 自愈，需取到自愈后的 ACL 作为证据。
- 教师机发送侧录屏/日志行。

**(e) 本机限制与替代**

- 单机无法同时扮演「教师」与「不登录学员」：最低 2 台真机，或「真机教师 + 桥接 VM 学员」（VM 重启到登录界面即可；宿主机不得兼任学员）。
- 登录界面无法截屏：手机拍照（记录拍照时刻）；教师机侧日志时间戳补强。
- 安装（管理员，install.ps1 + `-GuiExe` 必传）在真机由 QA 执行（#14 明确：开发无管理员权限，此路径留给验收）。
- 锁屏（`Win+L`）**不等价**（会话仍在）：只可作为预检，不作为判据。
- 若机房镜像开启自动登录，该项无法按原步骤执行：需先关闭自动登录或换机，并在报告登记环境约束。

### 4.6 TC-12 右键退出（服务存活、数据完整）

**(a) 前置环境**

- 任一已装「服务 + GUI」的机器（教师机、学员机各跑一遍更佳）；机器上已有若干共享数据（直接复用 TC-03/TC-06 产物，记下基线数量）。
- 已知服务名与进程名（§6 确认），取证命令就绪。

**(b) 执行步骤（点击级）**

1. 基线取证（PowerShell，输出存档）：

```powershell
Get-Service ClassShareSvc                          # 预期 Running
tasklist /FI "IMAGENAME eq classroom-gui.exe"       # 预期可见（以打包产物名为准）
tasklist /FI "IMAGENAME eq classroom-svc.exe"       # 预期可见（服务进程）
Get-ChildItem "$env:ProgramData\ClassShare\data" -Recurse -File | Measure-Object | Select-Object Count
python -c "import sqlite3,os;p=os.path.expandvars(r'%PROGRAMDATA%\ClassShare\data\index.db');c=sqlite3.connect(p);print(c.execute('select count(*) from share_items').fetchone())"
icacls "$env:ProgramData\ClassShare\data\ipc.token"   # E7：双授予（服务账号+登录用户）、无继承项 (I)
```

2. 右键浮动图标 → 菜单六项齐全（A3：粘贴并共享/粘贴为文本草稿/打开课堂共享/暂停接收提醒/设置/退出课堂共享，截图）。
3. 点击「退出课堂共享」。
4. 观察：浮动图标消失、主界面（若开着）关闭、无残留窗口（录屏）。
5. 复查：GUI 进程不在 `tasklist`；`Get-Service` 仍 **Running**。 —— **TC-12 主判据完成点**
6. 数据完整：重复步骤 1 的文件计数与 `index.db` 条目数，与基线一致。
7. 退出期间接收（2.4 呼应）：GUI 退出状态下，由教师机（对学员机场景）或另端发送一条新文本 → 本机服务日志显示接收并落盘（无 GUI 时无 tips，符合设计）。
8. 重新启动 GUI → 列表完整，且步骤 7 那条新内容出现。
9. 观察项（FR-G-03 异常边界）：发起一次大文件（如 60 MB）接收，传输途中退出 GUI → 预期当前分块落盘或由服务续传，正式条目不得呈「已下载」；允许存在 `.part`（属中间态）。

**(c) 预期结果与判定标准**

| # | 判定项 | 标准 | 来源 |
| --- | --- | --- | --- |
| 1 | 图标与窗口消失 | 无残留 GUI 窗口/进程 | TC-12 主判据 |
| 2 | 服务仍运行 | `Get-Service` = Running | TC-12 主判据 / FR-G-03 |
| 3 | 数据完整 | 文件数与索引条目数与基线一致 | TC-12 主判据 |
| 4 | 重启 GUI | 列表完整 + 补见退出期间内容 | FR-G-03 / 2.4 |
| 5 | （观察）传输中退出 | 无半成品被标记为完整 | FR-G-03 / 9.2 |

**(d) 需采集的证据**

- 前后两次 `Get-Service` / `tasklist` / 计数命令输出（同格式对比）；右键菜单与退出过程录屏/截图；重启 GUI 后列表截图；服务日志（退出前后对比行）。

**(e) 本机限制与替代**

- 无硬性限制，本机即可完成（前提：该机已按 M1 形态安装服务与 GUI）。
- 若在开发机上以 `cargo tauri dev` 跑 GUI（未装服务），「服务仍运行」无从判定：必须用安装形态执行，dev 形态结果只算预检。

## 5 结果判定与放行口径

### 5.1 单用例判定

- **通过**：主判据全绿且无「阻断/严重」缺陷（一般/轻微缺陷可挂账跟进）。
- **有条件通过**：主判据绿但因环境规模/工具缺失存在登记偏差（如 §5.2 缩减规模），须 Lead 书面确认放行条件。
- **不通过**：任一主判据不满足，或存在阻断缺陷。

### 5.2 TC-02 规模口径（已决策：方案 B，Lead 批准于 2026-11-13）

PRD TC-02 与 9.1 要求 30 台，M1 现实可用机器有限。备选与决策如下：

> **执行口径（已批准）**：M1 以 **≥3 台（目标 5–8 台）真机/VM 全绿**为功能门禁；同网「多 agent 进程压测」（几台机器各跑 N 个 agent 实例、不同昵称与 data_root，凑 30 连接）观测扇出与时延趋势；**正式 30 台 p95 压测留 M3**；TC-02 按「**有条件通过**」计入放行，M1 验收报告与 v0.1.0 release note 均须注明该口径。
>
> 压测代理可行性已确认（#14 ⑤）：命名管道支持同机多实例并发（≥2 已测）；服务本体 SCM 单实例、同机不并存双角色（设计如此）——压测代理须以多 **agent 进程**（非多服务）方式搭建，具体吞吐口径在验收准备复核中实测确定。

| 方案 | 内容 | 代价/风险 | 结论 |
| --- | --- | --- | --- |
| A. 等真机 30 台 | 机房/实验室整批执行 | M1 放行时间不可控 | 未采纳 |
| B. 缩减门槛 + 压测代理 + M3 补全 | 见上方执行口径 | 代表性弱于真机，须注明口径 | ✅ 已批准（2026-11-13） |
| C. VM 池凑 30 节点 | Hyper-V/VMware 桥接网络批量克隆 | 磁盘/网络同宿主干扰，数据代表性弱于真机 | 未采纳 |

> 无论选哪项，报告中都必须明确标注实际规模，不得以缩减环境的结论冒充 30 台结论。

### 5.3 v0.1.0 放行条件（对照 DEVELOPMENT.md Tag 策略）

- TC-01/03/06/11/12「通过」；TC-02 按 §5.2 已批准的方案 B 以「有条件通过」计入（报告与 release note 注明口径）。
- `M1-report.md` 完成、证据齐备、缺陷清单与 issue 对应齐全 → 建议 Lead 打 `v0.1.0`（release note 注明 TC-02 缩减口径与 M3 压测计划）。

## 6 执行前确认清单（第 1–5 项已确认 ✅，来源 issue #14 / e42dbd5，QA 已读 install.ps1 核对；第 6/8 项已关闭）

| # | 问题 | 已确认值 / 状态 | 来源 |
| --- | --- | --- | --- |
| 1 | 服务名 / 进程名 | 服务 `ClassShareSvc`；服务进程 `classroom-svc.exe`；GUI 进程 `classroom-gui.exe`（以打包产物名为准）；无管理员降级为 `--console` 前台进程 | ✅ #14 / e42dbd5 |
| 2 | 数据根 | 管理员安装 `%PROGRAMDATA%\ClassShare\data`（svc.json 在 `%PROGRAMDATA%\ClassShare\svc.json`）；降级安装 `%LOCALAPPDATA%\ClassShare\data`；GUI 侧 gui.json/profile.json 仍在 `%APPDATA%\ClassShare\` | ✅ |
| 3 | 日志 | `<数据根>\logs\svc.log`，RFC3339 含毫秒，RUST_LOG 可调；关键行 broadcast / received / applied | ✅ **已确认且已落地**（E4 合入 main 73e17d3）：三条锚点经真实 host+agent 演练与真实服务二进制复跑验证（§3.9/§3.9.1）；行内布局=消息在前、字段在后；**时间戳为 UTC**，与文件 mtime（本地时区）不可混用 |
| 4 | 防火墙 | E1 已修复（#44 合入）：TCP/47821 仅 teacher；UDP/47820 两角色均建且程序限定 classroom-svc.exe | ✅ 已闭合（§3.2 复验口径待真机） |
| 5 | 多实例 | 命名管道同机多实例并发（≥2 已测）；SCM 服务单实例；同机不并存双角色（设计如此）→ 压测代理以多 agent 进程搭建 | ✅ |
| 6 | TC-02 规模口径 | 放行结论 | ✅ 已关闭：批准方案 B（2026-11-13，见 §5.2） |
| 7 | 可用真机台数与 OS 版本（Win10/11 各几台）、是否有 Hyper-V 主机 | §3.1 矩阵落地 | **Lead**（向用户收集信息中，不阻塞方案） |
| 8 | Task 14 验收 issue 编号 | 流程合规 | ✅ 已关闭：issue #15（2026-11-14），本方案执行以其跟踪 |

## 附录 A 命令速查

```powershell
# 安装/卸载（管理员；-GuiExe 必传，否则 GUI 登录自启不注册）
.\scripts\install.ps1 -Mode teacher -Room XK-204 -Pin 9527 -GuiExe <classroom-gui.exe>
# 学员：不传 -Room（E9 默认自动发现首个教师机）；多教室时才传 -Room 与教师一致
.\scripts\install.ps1 -Mode student -Nickname alice -GuiExe <classroom-gui.exe>
.\scripts\uninstall.ps1
Get-Content "$env:ProgramData\ClassShare\svc.json"    # 服务配置（mode/room_code/pin/nickname/manual_addr/tcp_port）

# 服务与进程
Get-Service ClassShareSvc; sc.exe query ClassShareSvc; sc.exe qfailure ClassShareSvc
tasklist /FI "IMAGENAME eq classroom-svc.exe"; tasklist /FI "IMAGENAME eq classroom-gui.exe"
Get-WinEvent -FilterHashtable @{LogName='System';Id=7036} -MaxEvents 30 | Where-Object Message -match 'ClassShareSvc'

# 数据与哈希（管理员安装数据根；降级安装换 $env:LOCALAPPDATA\ClassShare\data）
Get-ChildItem "$env:ProgramData\ClassShare\data" -Recurse -File | Select-Object FullName,Length,LastWriteTime
Get-FileHash <文件> -Algorithm SHA256

# 索引（列名以 schema.sql 为准）
python -c "import sqlite3,os;p=os.path.expandvars(r'%PROGRAMDATA%\ClassShare\data\index.db');c=sqlite3.connect(p);[print(r) for r in c.execute('select id,title,version,deleted,sync_state from share_items').fetchall()]"

# 日志锚点（E4 约定格式：broadcast/received/applied + item_id=/file=/size=；行内无 target 模块路径）
Select-String -Path "$env:ProgramData\ClassShare\data\logs\svc.log" -Pattern "broadcast|received|applied"
# 应急降级（E4 未合入时）：用会话生命周期行 + 落盘 mtime 代理
Select-String -Path "$env:ProgramData\ClassShare\data\logs\svc.log" -Pattern "教师端启动|学员加入|学员离线|已加入|旧连接"
Get-Item "$env:ProgramData\ClassShare\data\repo\teacher\<文件名>" | Select-Object LastWriteTime

# —— 脚本化（推荐，证据可复现）——
# 1) 每机采集（含 [3c] 机器可读锚点 TSV 段）
.\docs\acceptance\scripts\collect-m1-evidence.ps1 -Role student -ItemName "课堂讲义.docx" -OutFile "TC-06-学员A.txt"
# 2) 跨节点汇总 Delta 表 + p95 + 判定（-Offsets 为各机时钟偏差，秒）
.\docs\acceptance\scripts\build-delta-table.ps1 -Teacher "teacher.txt" -Students "stuA.txt,stuB.txt" -Offsets "stuA=0.4" -Out "delta.csv"

# 时钟
w32tm /resync; w32tm /query /status; Get-Date -Format "yyyy-MM-dd HH:mm:ss.fff"

# 测试数据
$r=[Random]::new(42); $b=[byte[]]::new(10MB); $r.NextBytes($b); [IO.File]::WriteAllBytes("f10mb.bin",$b)
Compress-Archive f10mb.bin payload10mb.zip
fsutil file createnew big120.bin 125829120
```

## 附录 B M1-report.md 模板骨架

````markdown
# M1 验收报告（TC-01/02/03/06/11/12）
版本 / 构建与安装记录（commit、tag、命令、日期）／环境矩阵（机器、OS、IP、角色）／时钟校准记录
## 结论总表（用例 × 通过/有条件通过/不通过 + 一句话依据）
## TC-XX 各节：环境 / 步骤结果（逐条 ✓✗）/ Δ 时延表 / 证据清单（路径）/ 发现缺陷（编号）
## 缺陷清单（级别、FR/TC 引用、issue 链接、复验状态）
## 放行建议（对照 §5.3）
````

## 附录 C 测试资产清单（执行前备齐）

- [ ] 10 MB 主判据文件（含 SHA-256 记录）
- [ ] 120 MB 超限文件、`x.exe` 风险文件、3 个多选小文件、60 MB 传输中断文件
- [ ] 128 字标准文本（附录 §4.3）与混合内容变体
- [ ] `课堂讲义.docx` V1/V2/V3 三版（含各版 SHA-256）
- [x] 取证脚本 `docs/acceptance/scripts/collect-m1-evidence.ps1`（**已就绪并实测**：服务/进程状态、时钟与时区、E4 日志锚点 broadcast/received/applied（机器可读 TSV 段）、条目落盘属性+哈希+.bak 计数、SQLite 索引概览、仓库统计；只读；兼容 Windows PowerShell 5.1 与 PS7；UTF-8 BOM 编码，勿用无 BOM 编辑器改存，否则中文机乱码）
- [x] 汇总脚本 `docs/acceptance/scripts/build-delta-table.ps1`（**已就绪并实测**：按 item_id 对齐教师 broadcast 与各机 received/applied → Δ 表、p95、两个判定口径、时钟偏移修正、CSV 输出）
- [ ] 各机录屏工具可用（Xbox Game Bar `Win+Alt+R`）、手机（TC-11）

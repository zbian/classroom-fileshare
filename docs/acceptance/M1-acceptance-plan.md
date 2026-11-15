# M1（P0 最小闭环）验收执行方案 —— TC-01 / 02 / 03 / 06 / 11 / 12

| 项目 | 内容 |
| --- | --- |
| 文档 | M1 验收执行方案（先成文备用，代码就绪后按此执行） |
| 作者 | qa-acceptance |
| 日期 | 2026-11-13 |
| 依据 | [PRD V1.0](../../../final_draft.md) 第 2.4 / 4 / 5 / 6 / 9 / 11 章；[M1 实施计划](../plans/2026-11-13-m1-p0.md)（Task 14）；[协作流程](../DEVELOPMENT.md) |
| 状态 | v1.2（TC-02 口径已批准 §5.2；§6 取证五项已确认并全文落实，来源 #14 / e42dbd5；PR #25 评审中） |
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
- 端口（M1 计划已锁定）：UDP 47820（beacon）、TCP 47821（服务）。防火墙已确认（#14 / e42dbd5）：install.ps1 仅 **teacher** 角色幂等删建两条入站规则——`ClassShare Service TCP`（TCP/47821）、`ClassShare Discovery UDP`（UDP/47820）；**student 机不加任何入站规则**。
- ★环境预检重点（新增）：学员机在**默认防火墙**（入站默认拒绝）下能否收到 47820 beacon——接收广播属入站流量，存在被默认策略拦截的风险。预检须在未动防火墙的学员真机上先跑发现流程；若被拦截即登记缺陷（学员侧未放行 UDP 47820），如临时手工放行继续测试必须同步登记环境偏差。
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

- M1 交付形态：仓库 release 产物 + Task 13（issue #14）的 `scripts/install.ps1`（注册服务 + SCM failure 自动重启 5s/5s/30s + teacher 防火墙 + GUI HKCU Run）与 `uninstall.ps1`。NSIS 安装包属 M3。
- 已确认安装参数（e42dbd5，QA 已读脚本核对）：
  - 教师：`install.ps1 -Mode teacher -Room <房间码> [-Pin <PIN>] -GuiExe <classroom-gui.exe>`；学员：`install.ps1 -Mode student -Nickname <昵称> [-ManualAddr <ip>:47821] -GuiExe <…>`（对应 RK-04 备用手动地址）。
  - 服务名 `ClassShareSvc`、服务进程 `classroom-svc.exe`、GUI 进程 `classroom-gui.exe`（以打包产物名为准）。
  - **数据根（管理员安装）= `%PROGRAMDATA%\ClassShare\data`**（svc.json 在 `%PROGRAMDATA%\ClassShare\svc.json`）；日志 `<数据根>\logs\svc.log`。降级安装（无管理员）数据根 = `%LOCALAPPDATA%\ClassShare\data`。GUI 侧 `gui.json` / `profile.json` 仍在 `%APPDATA%\ClassShare\`。
  - `-GuiExe` 缺省时 **GUI 登录自启不注册**（TC-11 步骤 5 依赖它，安装时必传）；HKCU Run 仅对执行安装的用户生效（机房多用户需逐用户或镜像配置，登记为部署注意事项）。
  - 降级模式观察项（#14 验收项）：无管理员安装 → HKCU Run 前台进程 `--console`、`%LOCALAPPDATA%` 数据根、明示局限（未登录不接收、崩溃不自动重启）——在备用机执行一次并取证，对照 FR-G-05 异常与边界。
- 验收执行前记录进报告：git commit / tag、构建命令（`cargo build --release` / `cargo tauri build`）、产物路径、安装日期与完整命令。#34 合并前可暂从 `feat/t13-svc` 分支取脚本，版本记录写明 e42dbd5；合并后改从 main 取用。

### 3.5 计时与时钟同步（时延类指标的取证基础）

- 时间源优先级：① 各机服务日志 `<数据根>\logs\svc.log`（已确认 RFC3339 含毫秒；关键行 broadcast / received / applied，RUST_LOG 可调，#14）② 样本机录屏画面时钟（Xbox Game Bar，`Win+Alt+R`）③ 秒表（最后手段，双人配合）。
- 执行前各机对时：`w32tm /resync`，并 `w32tm /query /status` 取证；无法对域/NTP 时，记录教师机与各学员机的 `Get-Date` 对拍偏差，计算 Δ 时校正。
- p95 口径：各学员机 Δ = (本机 applied/落盘时间) − (教师机 broadcast 时间)；报告同时给出「全部节点 ≤5s」与「p95 ≤5s」两个结论（PRD 11 章 TC-02 用前者，9.1 用后者）。

### 3.6 证据规范

- 目录：`docs/acceptance/evidence/M1/<TC-编号>/`，命名 `TC-XX-<两位序号>-<短语>.png|md|log`。
- 每张截图/输出必须能看出：机器（教师/学员 N）、时间（任务栏时钟或录屏）、被测对象。
- 录屏（.mp4）与 >2 MB 的大文件不入库：本地留存，报告登记绝对路径 + SHA-256。
- 命令输出统一用文本粘贴进对应 `.md` 证据文件（不靠截图承载文字，便于检索）。

### 3.7 缺陷流程

- 发现缺陷：报告 Lead → Lead 建 `type/bug` issue（附我用例编号、复现步骤、证据路径）→ 修复合并后我按原步骤复验并在 issue 留言闭环。
- 分级建议：**阻断**（用例主判据不通过）/ **严重**（FR 验收标准不通过但主判据通过）/ **一般**（负例、边界、文案）/ **轻微**（视觉、日志）。

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
5. 学员机 A：输入昵称「张三」→ 点击加入。
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
| 5 | 重名昵称 | 第二台被明确拒绝、无法进入 | FR-S-01 |
| 6 | （如执行）错误 PIN | 无法加入 | FR-T-01 |
| 7 | 重启 GUI | 不再弹昵称窗，自动重连 | FR-S-01 |

**(d) 需采集的证据**

- B2 创建窗口、B1 昵称窗口（含发现列表）、B3 已连接状态截图（教师机 + 学员机）。
- 教师机点击「开始上课」→ 学员机出现房间码的录屏（计时证据）；两端服务日志 beacon/join 时间戳文本。
- 重名拒绝提示截图；重启后直接进入主界面截图；`profile.json` 存在的命令输出。

**(e) 本机限制与替代**

- 重名负例需第 2 台学员机：可用 VM（Hyper-V **外部虚拟交换机/桥接**；Default Switch 的 NAT 不保证 UDP 广播可达），或重置学员机 A 的昵称数据后重跑（次选，等价性略弱）。
- PIN 负例需教师重建房间：与首轮共环境时注意清空两端数据或更换房间码，避免残留状态干扰。

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
2. 右键 → 「课堂共享」（菜单显示「发送 1 个文件」，A4）→ 点击发送。记 T0 = 教师服务日志 broadcast/upsert 时间戳（录屏同步）。
3. 各学员机：观察浮动图标旁 tips 气泡（名称、来源、类型、倒计时条，A2）；样本机（≥2 台）录屏记出现时刻 T_tip；其余机器以服务日志 received/applied 时间戳参与 Δ 计算。
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

- 教师机：右键菜单截图、发送时刻录屏、服务日志 broadcast 行。
- 每台学员机：服务日志 received/applied 行（文本汇总成一张 Δ 表进报告）；样本机 tips 录屏与列表「已下载」截图、双击打开截图。
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
python -c "import sqlite3,os;p=os.path.expandvars(r'%PROGRAMDATA%\ClassShare\data\index.db');c=sqlite3.connect(p);[print(r) for r in c.execute('select file_name,title,version,deleted,sync_state from share_items').fetchall()]"
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
Select-String -Path "$env:ProgramData\ClassShare\data\logs\svc.log" -Pattern "不登录测试"
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
- 登录后列表与打开成功截图；文件时间戳、System 7036、服务日志接收行的命令输出（与登录时刻同框对比，可写进同一 `.md` 证据文件）。
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

- 前后两次 `Get-Service` / `tasklist` / 计数命令输出（同格式对比）；右键菜单与退出过程录屏/截图；重启 GUI 后列表截图；服务日志接收行。

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
| 3 | 日志 | `<数据根>\logs\svc.log`，RFC3339 含毫秒，RUST_LOG 可调；关键行 broadcast / received / applied（host/agent tracing 事件） | ✅ |
| 4 | 防火墙 | 仅 teacher 角色幂等删建入站规则：TCP/47821 + UDP/47820；★新增预检重点：学员机默认防火墙下 beacon 接收（§3.2），预检不过即缺陷 | ✅（预检重点为 QA 新增） |
| 5 | 多实例 | 命名管道同机多实例并发（≥2 已测）；SCM 服务单实例；同机不并存双角色（设计如此）→ 压测代理以多 agent 进程搭建 | ✅ |
| 6 | TC-02 规模口径 | 放行结论 | ✅ 已关闭：批准方案 B（2026-11-13，见 §5.2） |
| 7 | 可用真机台数与 OS 版本（Win10/11 各几台）、是否有 Hyper-V 主机 | §3.1 矩阵落地 | **Lead**（向用户收集信息中，不阻塞方案） |
| 8 | Task 14 验收 issue 编号 | 流程合规 | ✅ 已关闭：issue #15（2026-11-14），本方案执行以其跟踪 |

## 附录 A 命令速查

```powershell
# 安装/卸载（管理员；-GuiExe 必传，否则 GUI 登录自启不注册）
.\scripts\install.ps1 -Mode teacher -Room XK-204 -Pin 9527 -GuiExe <classroom-gui.exe>
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
python -c "import sqlite3,os;p=os.path.expandvars(r'%PROGRAMDATA%\ClassShare\data\index.db');c=sqlite3.connect(p);[print(r) for r in c.execute('select file_name,title,version,deleted,sync_state from share_items').fetchall()]"

# 日志（RFC3339 毫秒）
Select-String -Path "$env:ProgramData\ClassShare\data\logs\svc.log" -Pattern 'broadcast|received|applied'

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
- [ ] 取证脚本 `collect.ps1`（TC-06/12 复用：服务状态 + 进程 + 目录计数 + 索引查询一屏输出）
- [ ] 各机录屏工具可用（Xbox Game Bar `Win+Alt+R`）、手机（TC-11）

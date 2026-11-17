# M1（P0 最小闭环）验收报告 —— 单机部分验收

| 项目 | 内容 |
| --- | --- |
| 报告 | M1 验收报告（部分验收：单机可做项 + 多 agent 代理） |
| 执行人 | qa-acceptance |
| 日期 | 2026-11-17（初版）/ 2026-11-17 修订（rc.2 补测） |
| 基线 | **`main @ 46470fe`（v0.1.0-rc.2）**，含 E9 自动发现、**E10 100 MB 上限**、O-1 构建防呆、O-3/O-4 |
| 环境 | 单机（用户确认仅 1 台可用）；Windows 11 专业版 26100；**非管理员**；PowerShell 5.1；WebView2 157.0.4341.60 |
| 依据 | [PRD](../../../final_draft.md) 第 11 章 / 2.4 指标；[验收方案 v1.3.7](../M1-acceptance-plan.md)；[冻结报告](../../reports/M1-freeze.md) |
| 结论 | **未见阻断缺陷，但发现两个 P0 缺口（E10 100 MB 上限未生效、E11 tips 生产路径未接线）——均已修复并验证闭环**；另在 tips 补测中发现 **E12（学员机安装路径 GUI 连错管道，阻塞 v0.1.0，修复中）**；6 项通过 / 1 项部分通过 / 若干项延后 |
| 证据 | `docs/acceptance/evidence/M1/single-machine/`（命令输出、日志、截图）；rc.2 补测证据前缀 `C-*` |

> 本报告只声明**单机可验范围**内的结论。凡需 2 台以上真机或管理员权限的判据，一律标注「延后」，不以单机结果替代。
> 修订说明：rc.1 报告结论为「未发现阻断级产品缺陷」；rc.2 补测确认存在 P0 缺口 **E10**（单文件 100 MB 上限未生效），已在 `7eea251` 修复并由本报告 §3.11 验证闭环。该缺口属**验收侧负例覆盖不足**（初版未测超限文件），已同步补测。

---

## 1 结论总表

| 用例 | 场景 | 判定 | 依据摘要 |
| --- | --- | --- | --- |
| **TC-01** | 首次使用（建课/房间码/PIN/在线数/EX-11） | **通过（教师侧 + 单机代理学员侧）** | 房间码广播、状态、EX-11 换端口、PIN 拒绝/放行、重名拒绝、E3 静默死亡/强杀、E9 自动发现均实测通过；**真实双机发现链路延后** |
| **TC-02** | 广播 10 MB 文件 | **通过（单机规模，见 §3 口径）** | 7 个真实 agent 进程全部落盘，最大 147.2 ms（保守上界）≤5 s；**30 台真机 p95 延后（M3）** |
| **TC-03** | 复制粘贴文本 | **通过（数据层）** | 128 字文本，教师端 size=128；7/7 已入班 agent 内容**逐字节一致**；**tips/剪贴板 UI 交互延后** |
| **TC-06** | 同名覆盖 | **通过** | 列表仅 1 条、version=2、主文件=V2 哈希、`.bak`=V1 哈希、恰 1 份 .bak、索引 1 行；UI 截图单行 |
| **TC-11** | 开机自启（不登录落盘） | **延后** | 需 2 台机器 + 重启不登录 + 管理员注册服务 |
| **TC-12** | 右键退出 | **通过（真实菜单点击）** | 点击「退出课堂共享」→ GUI 进程消失、0 残留窗口、服务仍 Running、数据完整、重启 GUI 数据完整 |
| **TC-13 前半** | 卸载清理 | **部分通过** | **降级路径通过**（HKCU Run 双项安装/清理、数据保留）；**管理员路径（服务注册/防火墙/PROGRAMDATA）延后** |
| **E7 取证** | 令牌双授予/ACL | **通过（降级模式）** | ACL 仅 SYSTEM/Administrators/当前用户 (R,W)，**无继承项、无 Users**；64 hex；服务模式延后 |
| **E3** | 重名探测（静默死亡/强杀） | **通过（单机进程级）** | 静默死亡 3 s 驱逐、强杀 0.18 s 释放、活连接拒绝且旧端不被驱逐 |
| **E10** | 100 MB 上限 / 风险类型（rc.2 补测） | **通过（修复闭环）** | 120 MB 拒绝（EX-04 逐字）、100 MB 边界接受、`.exe/.bat/.lnk` 拒绝、拒绝后小文件正常发；见 §3.11 |
| **O-1 补测** | 冷启动图标 ≤3 s | **通过** | 冷启动 439–646 ms（4 次）≤3 s |
| **TC-03 补测** | 空剪贴板置灰 / 不清空剪贴板 | **通过** | 空剪贴板时「粘贴并共享」`IsEnabled=False` 且显示 EX-03 文案；有文本时 `True`；读取后剪贴板仍为原文 |

## 2 执行环境与构建口径（重要）

| 项 | 实测值 |
| --- | --- |
| 机器 | 单机；Win11 Pro 26100；非管理员（`IsInRole(Administrator)=False`） |
| 用户 SID | `S-1-5-21-2068859581-1204002046-625995256-1001` |
| 数据根 | 降级模式 `%LOCALAPPDATA%\ClassShare\data`（服务模式 `%PROGRAMDATA%` 需管理员） |
| 服务二进制 | `cargo build --release -p classroom-svc` → `classroom-svc.exe` |
| GUI 二进制 | **`.\scripts\build-gui.ps1`**（内部 `pnpm build` + `cargo build --release -p classroom-gui --features custom-protocol`，含产物自检）——**禁止裸 `cargo build --release`**，见 O-1 |
| QA 探针 | `qa-tools/ipc-cli`（真实命名管道 + 令牌握手客户端）、`qa-tools/agent-probe`（真实 agent 进程），**QA 本地工具，不入上游** |

> ★**O-1（已升格为强制口径）**：GUI 若用裸 `cargo build --release -p classroom-gui`（不加 `custom-protocol` feature），release 产物仍为 tauri dev 模式 → 前端不内嵌 → 连 devUrl → **图标窗口始终不可见**（实测 `vis=False`，WebView2 子进程已创建，极易误判产品缺陷）。**生产/验收一律用 `.\scripts\build-gui.ps1`**；`build.rs` 会输出构建警告、`main.rs` 会在误用时弹 MessageBox 提示。根因见 [`docs/notes/m1-o1-gui-build.md`](../../notes/m1-o1-gui-build.md)。

## 3 分用例结果

### 3.1 TC-01 首次使用

**教师侧（通过）** — 证据 `B-TC01.txt` / `B-TC01b.txt`

| 判据 | 实测 |
| --- | --- |
| 房间码生成与状态 | IPC `Status`：`role=Teacher room=XK-204 online=0 state=Running`；TCP 47821 `Listen` |
| 房间码广播（FR-T-01/3.1） | UDP 47820 捕获 beacon：`{"protocol_version":1,"room":"XK-204","tcp_port":47821,"pin_required":false,"host_name":"BIAN-小白机"}` |
| EX-11 端口冲突 | 预先占用 47821 → `WARN 指定端口被占用，自动更换（EX-11）… os error 10048` → 自动改用 **10019**，服务 Running |
| PIN 开启 | `pin=2468` → beacon `pin_required=true` |
| PIN 错误拒绝 | `AGENT_RESULT error: 加入被教师机拒绝: WrongPin`（服务端明确拒绝） |
| PIN 正确放行 | `EVENT joined`，在线数 =1 |

**学员侧（单机代理方式，通过）** — 证据 `B-E9.txt` / `TC-01-dup-final.txt` / `TC-01-silent-death.txt` / `TC-01-kill.txt`

| 判据 | 实测 |
| --- | --- |
| **E9 自动发现并连接**（不传 manual_addr、不传 -Room） | agent 日志 `未配置 manual_addr，进入自动发现模式` → `EVENT discovered n=1` → `已加入 addr=172.22.48.1:47821` → `synced` → 收到历史条目 |
| 重名拒绝（EX-02） | 第二个同名 agent `AGENT_RESULT error: 加入被教师机拒绝: DupNickname`，**耗时 0.21 s**；旧 agent 仍在线、在线数仍为 1、随后仍收到广播 |
| E3 静默死亡（挂起进程，TCP 无 FIN/RST） | 挂起后仍有 1 条 Established；新同名 agent **加入成功**；svc.log：`旧连接无响应，已驱逐（昵称可重新加入）` |
| E3 强杀（taskkill /F → RST） | `学员离线` → 新同名 agent **0.13 s 加入成功** |
| 活连接拒绝日志 | svc.log：`旧连接存活，拒绝重名加入（EX-02）` |

> 单机代理说明：真实 agent 二进制 + 真实教师服务，同一台机器；**真实双机 UDP 广播链路（跨网卡/交换机）延后**。
> 同机第二个自动发现 agent 无法绑定 UDP 47820（`os error 10048`）——属「一机一学员」设计下的代理方式局限，**非产品缺陷**。

### 3.2 TC-02 广播 10 MB（通过·单机规模）

证据 `B-TC02.txt`

- 载荷：随机 10 MB（10485760 B，SHA-256 `38396E6D…59F04`，不可压缩）
- 拓扑：1 教师服务（真实 svc）+ **7 个真实 agent 进程**（6 手动地址 + 1 自动发现）
- 结果：**7/7 全部落盘**；`applied − IPC 请求发起` 最大 **147.2 ms**、p95 **147.2 ms**、全部 ≤5 s

| 节点 | received | applied | Δ(received) | Δ(applied) |
| --- | --- | --- | --- | --- |
| auto01 | 12:13:58.164 | 12:13:58.176 | 98.4 ms | 110.3 ms |
| man01 | .181 | .190 | 115.3 | 124.3 |
| man02 | .175 | .182 | 109.8 | 116.4 |
| man03 | .192 | .198 | 127.0 | 132.3 |
| man04 | .151 | .157 | 86.1 | 91.4 |
| man05 | .163 | .167 | 97.5 | 102.2 |
| man06 | .207 | .212 | 141.5 | 147.2 |

**口径与限制（必须随结论一起引用）**
1. **规模**：单机 loopback、7 个虚拟端点，非 30 台真机；按已批准 §5.2 口径，30 台 p95 正式压测留 M3。
2. **计时锚点语义**：教师侧 `broadcast` 锚点在**推送完成后**记录（`send_one_file`/`send_text` 中 `broadcast_event`+`broadcast_file` 之后），故 `applied − broadcast` 为**下界**（实测有 2 个节点为负值）。本报告改用 **`applied − IPC 请求发起时刻`** 作为**保守上界**，若其上界 ≤5 s 则真实时延必然 ≤5 s。
3. 建议（M2/M3）：补 `send_received`（服务收到请求即记）锚点，使 PRD 2.4「分发时延」可被直接测量。

### 3.3 TC-03 复制粘贴文本（通过·数据层）

证据 `B-TC03.txt`

- 文本：`"课堂通知" + "0123456789"×12 + "测试结尾"` = **128 字符**，SHA-256 `76FBEDCE…5C810`
- 教师端列表：`kind=Text title=课堂通知-第1版 size=128 version=1 sync=Synced`（size=字符数，符 PRD 3.5）
- 学员端：**7/7 已入班 agent** `received`+`applied` 且落盘文本 **逐字节一致**（哈希比对）
- **延后**：tips 气泡显示时延与「一键复制」剪贴板动作属 GUI 交互；本次以内容保真度验证数据层，UI 层延后（需学员端 GUI + 真机）

### 3.4 TC-06 同名覆盖（通过）

证据 `B-TC06.txt`、UI 截图 `B-TC12-main-window.png`

| 判据（PRD TC-06） | 实测 |
| --- | --- |
| 各端仅 1 条 | 列表 `items n=1`；UI「我共享的」中该文件仅 1 行 |
| 内容为第二版 | 主文件 SHA-256 = `92F3D8B1…A498`（= V2 源） |
| version=2 | 列表 `version=2`；`index.db`：`('课堂讲义.docx', 2, 0, 'Synced')`，`rows=1` |
| 恰 1 份 .bak | 目录 `课堂讲义.docx` + `课堂讲义.docx.bak`；`.bak` 数=1；`.bak` SHA-256 = `6BDD1E92…8167`（= V1 源） |
| 无二次确认弹窗 | 服务路径无确认环节（符合 PRD 5.7 直接覆盖） |

### 3.5 TC-12 右键退出（通过·真实菜单点击）

证据 `B-TC12.txt` / `B-TC12b.txt` / 截图 `TC-01-icon-visible.png`、`B-TC12-menu.png`、`B-TC12-main-window.png`

执行（真实鼠标输入，非模拟）：
1. 启动 GUI（`--role=teacher`）→ 图标窗口可见 `2464,24 72×72`（截图 `TC-01-icon-visible.png`）
2. 右键图标 → 窗口展开为 **304×252**（菜单展开），UIA 枚举到六项菜单：粘贴并共享 / 粘贴为文本草稿… / 打开「课堂共享」 / 暂停接收提醒 / 设置 / **退出课堂共享**
3. 真实左键点击「退出课堂共享」（坐标 2342,234）
4. 结果：

| 判据 | 实测 |
| --- | --- |
| 图标与窗口消失 | GUI 进程存在=False；残留窗口数=0 |
| 服务仍运行 | svc 进程存活；IPC `Status` 仍返回 `Running` |
| 数据完整 | 退出前后一致：仓库文件 4 个、`index_rows=3` |
| 重启 GUI 数据完整 | 重启后双击图标打开主界面，列表 3 条完整（文本 128 字 / 10.0 MB / 195 KB，均「已同步」），状态条「后台服务·运行中·房间 XK-204」 |

### 3.6 TC-13 前半 卸载清理（部分通过）

证据 `B-TC13.txt`

**降级路径（无管理员）——通过**
- `install.ps1 -Mode teacher -Room XK-204 -SvcExe <svc> -GuiExe <gui>`：
  - 输出 `WARNING: 无管理员权限：已降级为仅当前用户登录自启…`（EX-10 语义）
  - `WARNING: 未捕获主登录用户 SID…`（E7/user_sid 提示，符合设计）
  - HKCU Run 写入 **两条**：`ClassShareSvc = <svc> --console`、`ClassShareGUI = <gui>`
  - **未注册服务**（符合降级）、**未加防火墙规则**（符合降级）、`svc.json` 字段正确
  - 安装摘要五项齐全（服务名/进程/运行方式/数据根/日志/防火墙/GUI 自启/令牌授权/房间/多实例）
- `uninstall.ps1`：服务「未注册（跳过）」、HKCU Run 两条**已清除**、防火墙规则 0、**数据保留**（repo 4 个文件，符合 PRD 9.5）
- 观察（轻微）：无右键扩展注册项时 `reg.exe delete` 打印 `ERROR: The system was unable to find the specified registry key or value` 到 stderr（脚本继续、清理结果正确）——建议静默化以免运维误判。

**管理员路径——延后**：服务注册（`sc create`/SCM failure 重启）、teacher 防火墙规则（TCP 47821 + UDP 47820 程序限定）、`%PROGRAMDATA%` 数据根、GUI 自启注册均需管理员真机。
> 注：当前脚本在非脚本同目录场景需显式 `-SvcExe`（`Resolve-Binary` 只查 `$PSScriptRoot`），建议文档注明。

### 3.7 TC-11 开机自启（延后）

需：2 台机器（教师 + 不登录学员）、管理员注册服务、重启至登录界面、教师发送、登录后核验文件时间戳早于登录时刻。单机 + 非管理员环境**无法执行**，按要求标注延后。

### 3.8 E7 令牌 ACL 取证（通过·降级模式）

证据 `B-E7.txt`

```
C:\Users\Bian\AppData\Local\ClassShare\data\ipc.token NT AUTHORITY\SYSTEM:(R,W)
                                                      BUILTIN\Administrators:(R,W)
                                                      BIAN-小白机\Bian:(R,W)
```
- **无继承项 `(I)`、无 `Users` 可读**；令牌长度 64 hex
- SYSTEM/Administrators 为设计内「管理可达」受托人，符合预期
- **服务模式取证延后**（需管理员安装 + `%PROGRAMDATA%` + user_sid 双授予 + 首次登录后 SessionChange 自愈）

### 3.9 E3 重名探测（通过·单机进程级）

证据 `TC-01-dup-final.txt`、`TC-01-silent-death.txt`、`TC-01-kill.txt`（基线 6e778c0，E9 改动仅涉 agent 发现链路，不影响 host 准入逻辑）

- 活连接同名：**0.21 s** 拒绝 `DupNickname`，旧端未被驱逐并继续收广播
- 静默死亡（进程挂起）：探测 ~3 s 后驱逐，同名可重新加入
- 强杀（RST）：离线感知后 **0.13 s** 同名可重新加入
→ E3 三条复验口径在**真实二进制**（非测试桩）上全部满足，可据此关闭 #37 的复验部分。

### 3.10 E9 自动发现（通过·单机）

见 §3.1；`auto01` 在无 manual_addr、无房间码过滤下发现并加入本机教师，符 PRD 2.4「学生 1 步」。**双机真实广播链路延后**。

### 3.11 rc.2 补测（基线 `46470fe`）

证据前缀 `C-*`（`C-E10.txt`、`C-TC01-coldstart.txt`、`C-TC03-clip.txt`、`C-TC06.txt`、`C-TC12.txt`、`C-list-race.txt`）

#### 3.11.1 E10：100 MB 上限与风险类型（P0 缺口修复验证）—— 通过

| 场景 | 实测 | 判定 |
| --- | --- | --- |
| 120 MB（125829120 B） | 拒绝：svc.log `WARN 教师文件广播失败 … error=**单文件不能超过 100 MB（125829120 字节）**`（EX-04 逐字） | ✅ |
| 恰好 100 MB（104857600 B） | **接受**并广播（`broadcast … file=exact100.bin size=104857600`），索引 `('exact100.bin', 104857600, 1, 0)` | ✅ 边界不误杀 |
| `x.exe` / `x.bat` / `x.lnk` | 三者均拒绝：`error=**高风险文件类型: x.exe / x.bat / x.lnk**`（EX-05 语义） | ✅ |
| 拒绝后立即发小文件 | `小文件.txt` 成功入库并广播（磁盘 2 字节、索引在列） | ✅ 不污染后续发送 |
| 最终列表 | 仅 `exact100.bin` + `小文件.txt` 两条（被拒项未入库） | ✅ |

> EX-05 文案差异记录：实现为「高风险文件类型: <文件名>」，PRD 8.3 文案表为「该类型不允许共享」——语义等价、信息更具体；不判缺陷，建议 M2 统一文案口径时对齐。

#### 3.11.2 O-1 补测：冷启动图标可见时延 —— 通过

判据 FR-G-01「冷启动后 3 秒内出现图标」。

| 轮次 | 图标可见耗时 |
| --- | --- |
| 第 1 次（首次） | **646 ms** |
| 第 2 次 | 439 ms |
| 第 3 次 | 438 ms |
| 第 4 次 | 467 ms |

→ 全部 ≤3000 ms，**PASS**。测量方式：启动进程后以 40 ms 间隔轮询枚举该进程的 `课堂共享` 窗口（宽度 ≤200 判为图标窗）直至 `IsWindowVisible`。

#### 3.11.3 TC-03 补测：空剪贴板置灰 / 发送不清空剪贴板 —— 通过

判据 FR-G-03 / EX-03（单机可验，经真实右键菜单 + UIA 读取菜单项启用态）：

| 场景 | 实测 |
| --- | --- |
| 空剪贴板（`Set-Clipboard $null`） | 右键菜单展开 304 px；「粘贴并共享」`IsEnabled=**False**`，且菜单项文案为 **「粘贴并共享 剪贴板中没有可共享的文本或文件」**（EX-03 逐字） |
| 剪贴板有文本 | 「粘贴并共享」`IsEnabled=**True**` |
| 菜单读取剪贴板后 | 剪贴板内容**仍为原文**（`True`）——读取不修改剪贴板（FR-G-03「不得修改其内容」） |

#### 3.11.4 TC-06 / TC-12 在 rc.2 基线复验 —— 通过（无回归）

- **TC-06**：两次同名发送 → 列表 `version=2` 单条；主哈希 = V2 `92F3D8B1…A498`；`.bak` = V1 `6BDD1E92…8167`；`.bak` 数 = 1。
- **TC-12**：GUI 由 `scripts/build-gui.ps1` 产物启动 → 真实右键「退出课堂共享」→ GUI 进程消失、残留 GUI 数 0、服务仍 Running（IPC Status 正常）、仓库文件数退出前后一致（10）。

#### 3.11.5 观察：List 的最终一致性（新增 O-6）

`SendFiles` 返回 `ok` 后**紧随**的一次 `List` 可能暂不包含该条目（连续 8 轮 `send→list` 实测出现 2 次计数滞后，随后自动一致；见 `C-list-race.txt`）。属**最终一致**表现，不影响 TC-06 判据（验收按合理等待取证）。建议：GUI 发送后以服务端通知（SvcNotify）驱动刷新，而非立即重列表。

### 3.12 rc.3 补测：TC-02 tips 判据（E11 修复验证）

基线 `1fea3d1`（v0.1.0-rc.3，E11 生产接线 `ipc_client.rs: tips::push_tip` 已合入）。证据前缀 `E-*/F-*/G-*/H-*`。

**测试形态**：真实三进程链路——QA 教师探针（真实 `classroom_host::run_host`，端口 47822/47823/47825）+ **真实学员 svc**（`--console`，`mode=student`，manual_addr 指向探针）+ **真实学员 GUI**（`role=student`，由 `scripts/build-gui.ps1` 构建）。
> ⚠️ 前置说明（重要）：本轮 tips 判据通过的前提是 `gui.json` 中 `role=student` 已就位。这一前提在**真实安装路径下并不成立**——由此发现 **E12**（见 §3.12.6），已裁定阻塞 v0.1.0，另行修复复验。

#### 3.12.1 修复前 vs 修复后对比（同一链路、同一判据）

| 观测 | 修复前（89e786c） | 修复后（1fea3d1） |
| --- | --- | --- |
| 学员 svc 是否收到 | ✅ `received` + `applied`（数据链路正常） | ✅ 同左 |
| tips 窗口是否可见 | ❌ 窗口存在但 **`vis=False`**，14 s 轮询从未可见（`D-TIPS-before.txt`） | ✅ **`vis=True`**（`E-TIPS-final.txt`） |
| 用户可感知结果 | **收到但从不提醒**（FR-G-02 失效） | 正常弹出并自动消失 |

#### 3.12.2 ① tips 可见且 ≤1 s —— 通过

同一台机同一时钟（epoch ms）实测：

| 区间 | 实测 |
| --- | --- |
| 发送 → 到达（`received`） | 96 ms |
| 到达 → 落盘（`applied`） | 6 ms |
| **落盘 → tips 可见** | **21 ms** |
| **到达 → tips 可见** | **27 ms** |

判据 FR-G-02「发送后 1 秒内出现气泡」/「到达后 ≤1 s」→ **27 ms ≪ 1000 ms，PASS**。
（按 Lead 提醒，**未复用 `applied−broadcast`** 这一 O-2 下界口径，而是从窗口可见轮询的墙钟独立测量并与 svc 锚点同钟对齐。）

#### 3.12.3 ② 三要素（名称/来源/类型） —— 通过

UIA 读取 tips 窗口文本（`G-TIPS-stack.txt`）：
`收到文本：连发测试#1` / `来自 教师` / `· 4 字` / `15 s` 倒计时 —— **内容名称、来源、类型（文本/字数）齐备**，符 FR-G-02 与 7.2。

#### 3.12.4 ③ 15 秒自动消失 —— 通过

`tips 首次可见 epoch=1794922359912 → 消失 epoch=1794922374871` ⇒ **14.96 s**（判据 15 s，容差内 PASS）。

#### 3.12.5 ④ 连发 5 条 → 最多 3 条 + 合并提示 —— 通过

教师连发 5 条（`连发测试#1..#5`，间隔 200 ms）→ UIA 实测 tips 窗口内**恰好 3 张卡片**（#1/#2/#3）+ **「还有 2 条新共享 · 点击打开列表」**；截图 `G-TIPS-stack.png` 另拍到 #4/#5 轮换显示（前序到期后补位），与「最多 3 条堆叠 + 合并」一致。

#### 3.12.6 ⑤ 暂停提醒：抑制显示但数据入库 —— 通过

| 步骤 | 实测 |
| --- | --- |
| 菜单点「暂停接收提醒」 | `gui.json` `pausedTips=true`；菜单项变为「**恢复接收提醒**」 |
| 暂停期间教师广播 | 18 s 轮询内 tips **从未可见**（抑制生效） |
| 同时数据是否入库 | svc `received item_id=47234b8b…` + `applied … version=1`（**数据入队/入库不受影响**，符 FR-G-03「只抑制 tips，不影响接收与落盘」） |
| 点「恢复接收提醒」 | `pausedTips=false`，恢复成功 |

#### 3.12.7 附：本轮暴露的 E12（阻塞 v0.1.0）

补测时先用 `--role=student` 手工启动 GUI，实测 GUI 仍连 `\\.\pipe\classshare-svc-teacher` 并失败、45 s 内 tips 全无；改为**预先写 `gui.json{"role":"student"}`** 后才正常。根因三处（`install.ps1` 不写 GUI 角色/`start_bridge` 首连失败不重试/`--role` 与 `start_bridge` 时序竞争）详见 §4 的 E12 与 issue #83。**该缺陷意味着按 install.ps1 正常安装的学员机 GUI 连不上本机服务**，故 tips 的上述通过结论须在 E12 修复后由**真实安装路径**复验（rc.4）。

## 4 缺陷与观察项| 编号 | 级别 | 内容 | 处置 |
| --- | --- | --- | --- |
| **E10** | **P0 缺口（已修复闭环）** | 单文件 100 MB 上限修复前未生效（PRD 5.3/9.4/EX-04）；QA 初版验收未覆盖该负例 | 已由 `7eea251` 修复，本报告 §3.11.1 验证闭环 |
| **E11** | **P0 缺口（已修复闭环）** | tips 生产路径未接线：`push_tip` 在 release 无调用者（唯一调用方 `demo_tips` 为 debug-only），`ipc_client` emit `svc://notify` 而 `TipsStack` 监听 `svc://share-received` → **release 产物中 FR-G-02 永不触发**（「收到但从不提醒」）。QA 初版报告写「无自动化 → 未验证」**低估了问题**（实为生产路径未接线） | 已由 `1fea3d1` 修复（`ShareReceived → push_tip` + 同源双通知去重 + 本人条目不自弹）；本报告 §3.12 补测 ①–⑤ 全部通过。措辞已在 §5 更正 |
| **E12** | **P0 阻塞（修复中，issue #83）** | **按 install.ps1 正常安装的学员机 GUI 连不上本机服务**：① `install.ps1` 注册 GUI 自启不传 `--role`、且从不写 `gui.json`，GUI 角色默认 `teacher` → 连 `classshare-svc-teacher`，而学员 svc 监听 `classshare-svc-student`；② `start_bridge` 首连失败仅记录、**不重试**（`ensure_client` 惰性重连只在有 UI 请求时触发）→ 常驻图标场景通知流永不建立；③ `--role=` 写盘在 `start_bridge` 之后（时序竞争）。实测：`--role=student` 手工启动仍连 teacher 管道失败、45 s 无 tips；预写 `gui.json role=student` 后一切正常 | 已裁定**阻塞 v0.1.0**，dev-gui 修复（写 GUI 角色 + `start_bridge` 退避重连 + `--role` 时序）；rc.4 后由 QA 用**真实 install.ps1 -Mode student** 复验 |
| O-1 | **口径要求（已升格为强制）** | GUI 生产构建必须用 `scripts/build-gui.ps1`（或显式 `--features custom-protocol`），**禁止裸 `cargo build --release`**——否则前端不内嵌、图标永不显示，极易被误判为产品缺陷 | 已入库：`build-gui.ps1` + `build.rs` 警告 + `main.rs` MessageBox 防呆；方案 §3.4 已写入强制口径，详见 [`docs/notes/m1-o1-gui-build.md`](../../notes/m1-o1-gui-build.md) |
| O-2 | 观察（可观测性） | `broadcast` 锚点在**推送完成后**记录，`Δ=applied−broadcast` 为下界（可负），无法直接测 PRD 2.4 分发时延；本报告改用「请求发起→applied」保守上界 | M2/M3 补 `send_received` 锚点 |
| O-3 | 轻微（已修） | `uninstall.ps1` 在右键项不存在时向 stderr 打印 reg.exe 原生错误 | `46470fe` 已修（PowerShell 原生 cmdlet + 静默） |
| O-4 | 轻微（已修） | `install.ps1` 非脚本同目录需显式 `-SvcExe` | `46470fe` 已修（增加 `..\target\release|debug` 候选 + 失败指引） |
| O-5 | 观察（代理局限） | 同机第二个自动发现 agent 无法绑定 UDP 47820 | 属一机一学员设计，非缺陷 |
| O-6 | 轻微（新增） | `SendFiles` 返回 ok 后紧随的 `List` 可能暂不含该条目（最终一致） | 建议 GUI 以 SvcNotify 驱动刷新；不影响判据 |
| O-7 | 观察（范围） | 右键菜单「设置」项为置灰不可用 | 设置面板 F1 不在 PRD 12 章 P0 清单内，属 M1 范围外，非缺陷 |

## 5 延后项与风险（交 Lead 决定放行口径）

| 延后项 | 原因 | 风险 |
| --- | --- | --- |
| TC-11 不登录落盘 | 需 2 台 + 管理员 + 重启 | 服务模式数据根与 Session 0 落盘未验证；**M1 P0 指标之一** |
| TC-13 管理员路径 | 非管理员 | 服务注册/SCM 重启/防火墙/`%PROGRAMDATA%` 未验证 |
| E7 服务模式双授予 | 非管理员 | 服务模式令牌仅 LocalSystem 或双授予需真机核验 |
| TC-01 真实双机发现 | 单机 | 跨机 UDP 广播链路（交换机/防火墙）未验证 |
| TC-01「重启 GUI 不再弹昵称」 | 学员客户端需教师端配合入班（双机） | 已**代码级确认**条件为 `role==='student' && !nickname_done`，`nickname_done` 持久化于 `gui.json`（`set_nickname_done`）；运行期复验待双机 |
| TC-02 30 台 p95 | 单机 | 已批准口径：留 M3 |
| TC-03/TC-02 的 tips UI 时延 | ~~无学员端 GUI 自动化~~ → **已修正措辞**：根因是**生产路径未接线（E11）**，不是「无法自动化验证」；E11 已修复（1fea3d1）并由本报告 §3.12 **补测通过**（到达→tips 27 ms、15 s 消失、连发堆叠/合并、暂停语义） | 已闭环 |
| TC-12 在服务模式下的服务存活 | 降级模式验证 | 服务模式 SCM 托管下的存活未验证 |

**放行建议**：`v0.1.0-rc.2/rc.3` 已验证 E10/E11 闭环；正式 `v0.1.0` **须待 E12 修复并由真实安装路径复验通过**（rc.4），此外仍有双机 + 管理员延后项（TC-11、TC-13 管理员路径、E7 服务模式、TC-01 真实双机发现、TC-02 30 台压测）。release note 须列明：E10（100 MB 上限）、E11（tips 生产接线）**已修复**；E12（学员机安装路径 GUI 管道）**修复后放行**；其余为延后项。

## 6 复现方式

```powershell
# 1) 构建（GUI 必须走正式脚本，见 O-1）
cargo build --release -p classroom-svc
.\scripts\build-gui.ps1

# 2) 降级安装（无管理员）
.\scripts\install.ps1 -Mode teacher -Room XK-204 -SvcExe <svc.exe> -GuiExe <gui.exe>

# 3) 起服务（或由 HKCU Run 拉起）
<svc.exe> --console

# 4) 取证
docs\acceptance\scripts\collect-m1-evidence.ps1 -Role teacher -DataRoot $env:LOCALAPPDATA\ClassShare\data -OutFile ev.txt
qa-ipc-cli.exe <data_root> status|list|sendfiles <path>|sendtext <title> <body>
# 5) 卸载
.\scripts\uninstall.ps1
```

**QA 探针源码**（本地工具，未入上游）：`qa-tools/ipc-cli/src/main.rs`、`qa-tools/agent-probe/src/main.rs`（探针日志 sink 复刻 `apps/classroom-svc/src/logging.rs`，使 agent 侧锚点与 svc 格式一致可直接汇总）。

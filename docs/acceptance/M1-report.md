# M1（P0 最小闭环）验收报告 —— 单机部分验收

| 项目 | 内容 |
| --- | --- |
| 报告 | M1 验收报告（部分验收：单机可做项 + 多 agent 代理） |
| 执行人 | qa-acceptance |
| 日期 | 2026-11-17 |
| 基线 | `main @ aee4e43`（含 **E9 e134b76**、E4/E6 锚点、E7 令牌 ACL、T-13 服务） |
| 环境 | 单机（用户确认仅 1 台可用）；Windows 11 专业版 26100；**非管理员**；PowerShell 5.1；WebView2 157.0.4341.60 |
| 依据 | [PRD](../../../final_draft.md) 第 11 章 / 2.4 指标；[验收方案 v1.3.6](../M1-acceptance-plan.md)；[冻结报告](../../reports/M1-freeze.md) |
| 结论 | **5 项通过 / 1 项部分通过（管理员路径延后）/ 1 项延后（需双机）**，未发现阻断级产品缺陷 |
| 证据 | `docs/acceptance/evidence/M1/single-machine/`（命令输出、日志、截图） |

> 本报告只声明**单机可验范围**内的结论。凡需 2 台以上真机或管理员权限的判据，一律标注「延后」，不以单机结果替代。基线 aee4e43 上的实测项与旧基线（6e778c0）上的探索项分别标注。

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

## 2 执行环境与构建口径（重要）

| 项 | 实测值 |
| --- | --- |
| 机器 | 单机；Win11 Pro 26100；非管理员（`IsInRole(Administrator)=False`） |
| 用户 SID | `S-1-5-21-2068859581-1204002046-625995256-1001` |
| 数据根 | 降级模式 `%LOCALAPPDATA%\ClassShare\data`（服务模式 `%PROGRAMDATA%` 需管理员） |
| 服务二进制 | `cargo build --release -p classroom-svc` → `classroom-svc.exe` |
| GUI 二进制 | `pnpm --dir apps/classroom-gui/ui build` + `cargo build --release -p classroom-gui **--features tauri/custom-protocol**` |
| QA 探针 | `qa-tools/ipc-cli`（真实命名管道 + 令牌握手客户端）、`qa-tools/agent-probe`（真实 agent 进程），**QA 本地工具，不入上游** |

> ★**构建口径发现（非产品缺陷，但会误导验收）**：GUI 若用裸 `cargo build --release -p classroom-gui`（**不加** `tauri/custom-protocol`），release 产物走 dev URL 而前端不加载 → `icon_ready` 不触发 → **图标窗口始终不可见**（实测 `vis=False`，WebView2 子进程已创建）。加特性后图标正常显示。建议：冻结报告/README 写明 GUI 构建命令，或加 CI 守卫。

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

## 4 缺陷与观察项

| 编号 | 级别 | 内容 | 处置建议 |
| --- | --- | --- | --- |
| O-1 | 观察（构建口径） | GUI 需 `--features tauri/custom-protocol`，否则图标不显示 | 写入构建文档/CI 守卫；**非产品缺陷** |
| O-2 | 观察（可观测性） | `broadcast` 锚点在推送完成后记录，`Δ=applied−broadcast` 为下界，可出现负值，无法直接测 PRD 2.4 分发时延 | M2/M3 补 `send_received` 锚点 |
| O-3 | 轻微 | `uninstall.ps1` 在右键项不存在时向 stderr 打印 reg.exe 原生错误 | 改为静默/过滤 |
| O-4 | 观察（文档） | `install.ps1` 非脚本同目录场景需显式 `-SvcExe` | 文档注明或放宽查找路径 |
| O-5 | 观察（代理局限） | 同机第二个自动发现 agent 无法绑定 UDP 47820 | 属一机一学员设计，非缺陷 |

**未发现阻断级产品缺陷。**

## 5 延后项与风险（交 Lead 决定放行口径）

| 延后项 | 原因 | 风险 |
| --- | --- | --- |
| TC-11 不登录落盘 | 需 2 台 + 管理员 + 重启 | 服务模式数据根与 Session 0 落盘未验证；**M1 P0 指标之一** |
| TC-13 管理员路径 | 非管理员 | 服务注册/SCM 重启/防火墙/`%PROGRAMDATA%` 未验证 |
| E7 服务模式双授予 | 非管理员 | 服务模式令牌仅 LocalSystem 或双授予需真机核验 |
| TC-01 真实双机发现 | 单机 | 跨机 UDP 广播链路（交换机/防火墙）未验证 |
| TC-02 30 台 p95 | 单机 | 已批准口径：留 M3 |
| TC-03/TC-02 的 tips UI 时延 | 无学员 GUI 自动化 | FR-G-02「tips ≤1 s」未验证 |
| TC-12 在服务模式下的服务存活 | 降级模式验证 | 服务模式 SCM 托管下的存活未验证 |

**放行建议**：按已批准 §5.2 口径，可出 **`v0.1.0-rc.1`（预发布）**，但须在 release note 明确：TC-11、TC-13 管理员路径、TC-01 双机发现、TC-02 30 台压测、tips UI 时延均为**延后项**，正式 `v0.1.0` 待双机 + 管理员真机验收。

## 6 复现方式

```powershell
# 1) 构建（注意 GUI 特性）
cargo build --release -p classroom-svc
pnpm --dir apps/classroom-gui/ui build
cargo build --release -p classroom-gui --features tauri/custom-protocol

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

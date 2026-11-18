# 双机测试方案（TC-01 真实双机 / TC-11 / TC-13 管理员 / E7 服务模式 / 跨机链路）

> 关联 issue：#91（双机/管理员环境补验）、#15（M1 验收）、#21（M2 验收）
> 目的：**只补单机无法验证的延后项**，单机已通过的 40+ 项不重做。
> 执行方式：人工跑（推荐首轮）或 mesh 自动化（见 §6）。

## 1 范围

| 用例 | 内容 | 单机状态 |
| --- | --- | --- |
| TC-01 真实双机发现 | 学生机自动发现教师机并入班；重名拒绝；PIN；EX-11 端口冲突 | 单机代理已验，真实广播延后 |
| TC-02 真实多端分发 | 10 MB 文件跨机分发、sha256、时延 | 单机 7 agent 已验 |
| TC-03 真实文本共享 | 跨机文本逐字节一致 + tips | 单机已验 |
| TC-06 同名覆盖 | 跨机覆盖（单条/version=2/1 份 .bak） | 单机已验 |
| TC-11 不登录收文件 | 学生机重启到登录界面不登录 → 服务接收 → 登录后可见 | 延后 |
| TC-13 管理员路径 | 服务注册/PROGRAMDATA/防火墙规则 + 卸载全清理 | 降级路径已验，管理员路径延后 |
| E7 服务模式 ACL | 令牌文件 ACL（SYSTEM/Administrators/当前用户 R/W、无继承、无 Users） | 降级模式已验 |
| TC-08 删除同步 | 跨机删除即时消失；断网端上线 ≤10s 补删 | 单机 8 agent 已验 |
| TC-09 离线补发 | 学生断网提交 → 恢复 ≤10s、无重复 | 单机已验 |
| TC-04/05 回传 | 学生跨机回传 → 教师分组；其他学生不可见 | 单机已验 |
| TC-07 占用重试 | 学生端锁定文件 → 教师覆盖 → 自动完成 | 单机双侧已验 |

## 2 前置条件

| 项 | 要求 |
| --- | --- |
| 第二台机器 | Windows 10/11 x64，8 GB+，WebView2 Runtime（Win11 自带；Win10 装 Evergreen） |
| 网络 | 同一网段、互 ping、**关闭 AP/客户端隔离**；UDP 47820 广播 + TCP 47821 可达 |
| 权限 | **教师机建议管理员**（服务模式 + PROGRAMDATA + 防火墙规则）；学生机可管理员或降级 |
| 版本 | 两台同一 tag（建议 `v0.2.0-rc.1` 或更新），同一构建脚本 |
| 时钟 | 两台开 NTP（`w32tm /resync`），跨机时延判据依赖 |
| 账号 | 两台各自本地账号；GitHub 凭证（若需证据回传） |

## 3 构建与安装（两台各一次）

```powershell
git clone git@github.com:zbian/classroom-fileshare.git; cd classroom-fileshare
git checkout v0.2.0-rc.1
.\scripts\build-gui.ps1                        # 生产构建（禁止裸 cargo build --release）
cargo build --release -p classroom-svc
# 教师机
.\scripts\install.ps1 -Mode teacher
# 学生机（单教室：不传 -Room → 自动发现）
.\scripts\install.ps1 -Mode student
```

安装后立即验证：

```powershell
Get-Service ClassShareSvc
Get-NetUDPEndpoint -LocalPort 47820
Get-NetTCPConnection -State Listen | Where-Object LocalPort -eq 47821
```

## 4 用例矩阵

### A 组：真实发现与入班

| # | 步骤 | 判据 | 证据 |
| --- | --- | --- | --- |
| A1 | 教师机开房（XK-204，无 PIN）→ 学生机启动 | 学生自动发现并入班；教师端 `online=1` | 两侧 svc.log + 学生端截图 |
| A2 | 学生用教师机已占用昵称加入 | 明确拒绝「昵称已被占用」，无连接 | 学生提示 + 教师日志 |
| A3 | 教师设 PIN=2468 → 学生输错/输对 | 错 PIN 拒绝、对 PIN 放行 | 两侧日志 |
| A4 | 教师机先占用 47821 再启动（EX-11） | 自动换端口，beacon 带实际端口，学生仍入班 | 教师日志 + 学生入班日志 |

### B 组：真实跨机内容分发

| # | 步骤 | 判据 | 证据 |
| --- | --- | --- | --- |
| B1 | 教师广播 10 MB 文件 | 学生落盘 sha256 一致；记录请求→落盘时延 | 两侧日志 + `Get-FileHash` |
| B2 | 教师粘贴 128 字文本 | 学生逐字节一致；tips ≤1 s 可见 | 截图 + UIA 文本 |
| B3 | 连续两次同名文件 | 学生端单条 / version=2 / 恰 1 份 .bak | 学生列表 + 仓库文件 |
| B4 | 学生端右键退出 GUI | GUI 进程消失、服务 Running、重启数据完整 | 进程表 + 截图 |

### C 组：双机专项（#91 核心）

| # | 用例 | 步骤 | 判据 |
| --- | --- | --- | --- |
| C1 | TC-11 不登录收文件 | 教师发文件 → 学生机重启到登录界面不登录 → 登录查看 | 服务（Session 0）接收落盘；登录后 GUI 可见 |
| C2 | TC-13 管理员路径 | 管理员安装（服务+PROGRAMDATA+防火墙）→ 卸载 | 服务/规则/目录/自启项全清理 |
| C3 | E7 服务模式 ACL | 服务模式安装后 `icacls` 令牌文件 | SYSTEM/Administrators/当前用户 R/W；无继承、无 Users |
| C4 | TC-08 删除同步 | 教师删除；学生禁用网卡→教师删除→恢复 | 在线 ≤1 s 消失；恢复后 ≤10 s 副本+索引清除 |
| C5 | TC-09 离线补发 | 学生断网提交 2 条 → 恢复 | ≤10 s 全部送达、无重复、队列清空 |
| C6 | TC-04/05 回传 | 学生回传文本/文件；第二台再起一个 agent 作「其他学员」 | 教师分组 +1/+2；其他学员 0 变化 |
| C7 | TC-07 占用重试 | 学生阅读器锁定文件 → 教师覆盖同名 | 学生显示「被占用重试」→ 关闭后自动完成，不断连 |

> C6 的「其他学员」可在学生机再起一个 agent 进程（`manual_addr` 指向教师、独立数据根/昵称）代理，不必第三台。

## 5 网络问题排查顺序

1. **防火墙**：放行 UDP 47820 / TCP 47821（若安装脚本未自动建规则）
   ```powershell
   New-NetFirewallRule -DisplayName 'ClassShare' -Direction Inbound -Protocol UDP -LocalPort 47820 -Action Allow
   New-NetFirewallRule -DisplayName 'ClassShare' -Direction Inbound -Protocol TCP -LocalPort 47821 -Action Allow
   ```
2. **AP 隔离**：学生机抓包确认能收到 47820 广播；收不到 → AP/客户端隔离或 VLAN
3. **兜底路径**：自动发现不可用时，学生机 `svc.json` 配 `manual_addr=<教师IP>:47821`（RK-04 备用）
4. **多网卡**：教师机多网卡时确认广播走与学生机同网段的那块

## 6 mesh 自动化（可选，需第二台装 DSH + dsh-plugin-mesh）

### 6.1 能力边界

| 需求 | mesh 是否满足 | 说明 |
| --- | --- | --- |
| 发现/派发/回收文本 | ✅ | `mesh_run` 派任务 → 对端 agent 执行 → 文本结果回传 |
| 二进制证据（截图/产物） | ❌ | 无附件通道；走 **Syncthing 证据同步**（§6.6）或 GitHub 分支 |
| 管理员操作（TC-13/E7） | ❌ | 对端 agent 默认 `workspace-write`，不提权 |
| 毫秒级时序 | ❌ | 跨机调度延迟 10 s 级；时序必须在 B 机脚本内测 |
| 证据可信度 | ⚠️ | 对端是 LLM agent；只信固定脚本的原始输出 + 哈希 |

### 6.2 架构

```
A 机（教师）                        B 机（学生）
Lead 会话                           DSH + mesh
  │ mesh_run(B, "执行 scripts/dual/student-case.ps1 -Case C4")
  ├──────────────────────────────▶ B agent 跑固定脚本
  │                                  ├─ 脚本内时序测量 + 断言 + 哈希
  │                                  ├─ 输出 JSON 证据信封
  │                                  └─ 日志/截图写本地 + 推送 evidence/dual 分支
  ◀────────────────────────────── 任务结果（JSON 摘要 + 证据路径）
  │ 汇总判定 → 报告
```

### 6.3 脚本契约（`scripts/dual/`）

| 脚本 | 运行位置 | 职责 |
| --- | --- | --- |
| `teacher-case.ps1` | A 机 | 教师侧动作 + 断言 + JSON 证据信封 |
| `student-case.ps1` | B 机 | 学生侧动作 + 断言 + JSON 证据信封（时序在脚本内测） |
| `run-dual.ps1` | A 机 | 校验前置条件、生成本地执行计划与 mesh 派发任务模板 |
| `syncthing-setup.ps1` | A/B 各一次 | 安装/配置/启动 Syncthing，共享证据目录（点对点，无 SMB/SFTP） |
| `syncthing-status.ps1` | A/B 均可 | 查看设备 ID/对端连接/文件夹状态；`-WaitConnectedSec` 等对端上线 |
| `syncthing-stop.ps1` | A/B 均可 | 停止本套件启动的 Syncthing 进程 |

证据信封格式（JSON）：`{ case, startedAt, finishedAt, steps[], rawOutput, hashes{}, verdict }`；
**`verdict` 只允许 `pass/fail/blocked/not_implemented`，不允许 agent 主观改写。**

### 6.4 第二台机器 DSH 配置（详见 §6.5）

- 安装 DSH → `dsh plugin add dsh-plugin-mesh` → **重启 DSH** → 起机器名
- 端口放行：mesh `TCP 45917 / UDP 45892`；课堂应用 `UDP 47820 / TCP 47821`
- 同 LAN 默认可直接派活；不可信网络开配对码（`requirePairing`）
- 确认 `mesh_peers` 能看到对方，且 capabilities 含 `rust`/`node`（B 机需构建）
- B 机需有登录桌面会话（GUI 用例）；管理员用例需 B 机以管理员运行 DSH 或人工执行

### 6.5 第二台机器 DSH 配置步骤（B 机）

1. **安装 DSH Desktop**（与 A 机同版本，避免 mesh 协议差异）→ 启动并登录
2. **安装 mesh 插件**（在 DSH Desktop 自带终端里）：
   ```bash
   dsh plugin add dsh-plugin-mesh
   ```
   安装后**重启 DSH**（插件启动时加载）
3. **起机器名**：打开侧栏「Mesh」→ 第一次会要求命名（默认主机名）；记下名字（A 机用它派发）
4. **网络**：
   - 同 LAN：等几秒自动发现；看不到点「局域网探测」
   - 跨网：Mesh → 连接 → 互联网组网 → 填 Tailscale 授权码（插件自动下载 tsnet 用户态节点）
   - 防火墙放行 mesh `TCP 45917`、`UDP 45892`；课堂应用 `UDP 47820`、`TCP 47821`
5. **信任配置**：
   - 同 LAN 测试：默认即可（发现即可派活，无确认弹窗）
   - 不可信网络：打开配对码，两端同码
   - 权限：对端任务默认 `workspace-write`；不要为测试放开 `danger-full-access`
6. **能力确认**：A 机 `mesh_peers` 查看 B 机 capabilities 是否含 `node`/`rust`（需在 B 机构建）
7. **桌面会话**：GUI 用例要求 B 机已登录桌面（锁屏可以，未登录不行——TC-11 相反：验证未登录时服务仍收文件）
8. **仓库与凭证**（B 机）：`git clone` + `gh auth login`（可选）；**证据回传用 Syncthing（§6.6）**，不需要 SMB/SFTP
9. **自检**：A 机 `mesh_peers` 能看到 B；A 机派发 `echo ok` 类任务能在 B 执行并回传结果

### 6.6 证据自动回流（Syncthing，推荐）

mesh 只有文本通道；截图/日志/JSON 用 **Syncthing 点对点同步**，不需要 SMB/SFTP/任何文件服务器。

**架构**：A（教师）与 B（学生）各跑一个 Syncthing 进程，共享同一个文件夹 ID（`classshare-dual-evidence`）：
- B → A：证据目录 `Send Only` → `Receive Only`（测试证据自动回流）
- 可选反向：A 的构建产物 `Send Only` → B `Receive Only`（mesh 无法传二进制）

**首次配置（A/B 各跑一次，顺序可任意）**：

```powershell
# A 机（教师）
.\scripts\dual\syncthing-setup.ps1 -Role teacher
#   记下输出的 deviceId，然后带对端 ID 再跑：
.\scripts\dual\syncthing-setup.ps1 -Role teacher -PeerDeviceId <B 的 deviceId> -PeerName 'B-学生机' -WaitPeerSec 60

# B 机（学生）
.\scripts\dual\syncthing-setup.ps1 -Role student -PeerDeviceId <A 的 deviceId> -PeerName 'A-教师机' -WaitPeerSec 60
```

**网络**：同步 `TCP/UDP 22000`、本地发现 `UDP 21027`（防火墙放行或同网段直连）；纯内网加 `-LocalOnly` 关闭全局发现/中继/NAT。

**状态/停止**：

```powershell
.\scripts\dual\syncthing-status.ps1 -PeerDeviceId <对端DeviceId> -WaitConnectedSec 60
.\scripts\dual\syncthing-stop.ps1 -All
```

**已验证（本机双实例预演，2026-11-18）**：两实例互连 `connected=true`（TLS1.3、isLocal）、B 写证据 → A 收到且 **SHA-256 一致**、延迟 <1s。

**已知坑（脚本已处理）**：PS 5.1 `Invoke-RestMethod -Body` 字符串按 ANSI 发送，含中文的仓库路径会变成 `??`（Syncthing 报 `filename syntax incorrect`）→ 脚本改为 UTF-8 字节 + `charset=utf-8`。

**注意**：只共享 `docs/acceptance/evidence/dual`，不要共享整个仓库；`.stfolder/`、`.stversions/`、`sync-conflict-*` 已 gitignore。

## 7 证据规范

- 日志：服务模式 `%PROGRAMDATA%\ClassShare\data\logs\svc.log`；降级 `%LOCALAPPDATA%\ClassShare\data\logs\svc.log`
- 时延：两侧日志时间戳 Δ（NTP 已同步）
- 归档：`docs/acceptance/evidence/M1/dual/`、`M2/dual/`，每用例 ≥2 类证据
- 双机证据回流：B 机写入 `docs/acceptance/evidence/dual/`（Send Only），由 Syncthing 自动同步到 A 机（Receive Only）；用 `scripts/dual/syncthing-status.ps1` 确认同步状态
- 结论回写：M1/M2 报告「延后」行改为实测结论；#91 逐条勾选关闭

## 8 执行顺序与时间（约 3 小时）

| 阶段 | 内容 | 时长 |
| --- | --- | --- |
| 0 | 环境确认 + 构建安装 + 基础连通 | 30 min |
| 1 | A 组（发现/入班/PIN/端口冲突） | 30 min |
| 2 | B 组（内容分发/覆盖/退出） | 40 min |
| 3 | C 组（TC-11/TC-13/E7/TC-08/TC-09/TC-04-05/TC-07） | 70 min |
| 4 | 证据归档 + 报告回写 + #91 收口 | 20 min |

## 9 #91 收口条件

- A1–A4、B1–B4、C1–C7 全部有结论（通过 / 失败开缺陷 / 环境不满足标注）
- 证据归档到 `evidence/*/dual/`
- M1/M2 报告「延后」项更新
- 失败项按 E 编号开 issue 并回归

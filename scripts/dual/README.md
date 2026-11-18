# scripts/dual —— 双机测试脚本（骨架 v1）

配套文档：[docs/acceptance/dual-machine-test-plan.md](../../docs/acceptance/dual-machine-test-plan.md)

## 文件

| 文件 | 运行位置 | 说明 |
| --- | --- | --- |
| `teacher-case.ps1` | 教师机（A） | 起隔离教师 svc、等待就绪/学生加入、输出 JSON 证据 |
| `student-case.ps1` | 学生机（B） | 起隔离学生 svc、等待入班、输出 JSON 证据（时序在脚本内测） |
| `run-dual.ps1` | 教师机（A） | 前置检查 + 生成执行计划与 mesh 派发文本 + 本机预演 |
| `syncthing-setup.ps1` | A/B 各一次 | 安装/配置/启动 Syncthing，共享证据目录（点对点，无 SMB/SFTP） |
| `syncthing-status.ps1` | A/B 均可 | 查看设备 ID/对端连接/文件夹状态；`-WaitConnectedSec` 等对端上线 |
| `syncthing-stop.ps1` | A/B 均可 | 停止本套件启动的 Syncthing 进程 |
| `export-artifacts.ps1` | A | 导出「被测环境包」：bin + scripts + cases.json + 哈希 manifest |
| `bootstrap-student.ps1` | B（首次） | 无人值守引导：Syncthing(student)、证据目录、环境自检 |
| `student-runner.ps1` | B | 按 `cases.json` 批量执行用例（子进程+超时），输出 `dual-report-<RunId>.json` |
| `remote-driver.ps1` | A | 导出 → 生成 mesh 派发文本 → 等待证据回流 → 校验汇总 |
| `cases.json` | 共享 | 用例清单（id/title/mode/script/args/timeout/evidenceGlob） |

## 证据信封（JSON）

```json
{
  "case": "A1", "machine": "B-PC", "role": "student",
  "startedAt": "...", "finishedAt": "...",
  "verdict": "pass|fail|blocked|not_implemented",
  "reason": "...", "steps": [{"name":"...","status":"...","detail":"...","at":"..."}],
  "hashes": {}, "dataRoot": "...", "svcLogTail": ["..."]
}
```

**verdict 只允许脚本按判据写死；agent 不得改写。**

## 使用

```powershell
# 教师机（终端 1）
.\scripts\dual\teacher-case.ps1 -Case A1 -Room XK-204 -WaitStudentSec 60

# 学生机（B）
.\scripts\dual\student-case.ps1 -Case A1 -TeacherAddr 192.168.1.7:47821

# 编排（教师机）
.\scripts\dual\run-dual.ps1 -Cases A1,B1,C4,C5 -StudentMachine <学生机mesh名>
```

## 证据自动回流（Syncthing，推荐）

mesh 派发只有文本通道，截图/日志/JSON 用 Syncthing 点对点同步（不需要 SMB/SFTP）。

### 快速开始（首次两台各跑一次，顺序可任意）

```powershell
# A 机（教师）
.\scripts\dual\syncthing-setup.ps1 -Role teacher
#   → 记下输出的 deviceId（形如 4YXJBRJ-...），然后：
.\scripts\dual\syncthing-setup.ps1 -Role teacher -PeerDeviceId <B 的 deviceId> -PeerName 'B-学生机' -WaitPeerSec 60

# B 机（学生）
.\scripts\dual\syncthing-setup.ps1 -Role student -PeerDeviceId <A 的 deviceId> -PeerName 'A-教师机' -WaitPeerSec 60
```

- 证据目录：`<repo>\docs\acceptance\evidence\dual`（A=Receive Only，B=Send Only）
- 脚本幂等：重复跑只更新配置；`-Stop` 停；`-NoStart` 只配置不起进程
- 端口：同步 `TCP/UDP 22000`、本地发现 `UDP 21027`（需防火墙放行或同一网段直连）
- 纯内网：加 `-LocalOnly`（关闭全局发现/中继/NAT）
- 本机多实例预演：`-StHome <另一目录> -GuiPort 8385 -SyncPort 22001 -LocalDiscoveryPort 21028`

### 状态与等待

```powershell
.\scripts\dual\syncthing-status.ps1 -PeerDeviceId <对端DeviceId> -WaitConnectedSec 60
```

### 已验证（本机双实例预演，2026-11-18）

- 两个实例（A 8384 / B 8385）互连成功：`connected=true`、TLS1.3、`isLocal=true`
- B（Send Only）写入证据 → A（Receive Only）收到，**SHA-256 一致**，延迟 < 1s
- 发现并规避一个坑：**PS 5.1 的 `Invoke-RestMethod -Body` 字符串按 ANSI 发送，含中文的仓库路径会变成 `??`**（Syncthing 报 `filename syntax incorrect`）→ 脚本已改为 UTF-8 字节 + `charset=utf-8`

### 注意

- 证据目录内 `.stfolder/`、`.stversions/` 等为 Syncthing 标记，已在 `.gitignore` 忽略
- 不要把整个仓库共享给 Syncthing（只共享 `docs/acceptance/evidence/dual`）
- 冲突文件 `sync-conflict-*` 已 gitignore；出现冲突说明两端同时写了同一文件（Send Only/Receive Only 配置下不应发生）
- 停止：`.\scripts\dual\syncthing-stop.ps1 -All`（只停本套件的进程）

## 无人值守远程执行（B 机只有 DSH，无人工）

设计见 [docs/acceptance/dual-machine-remote-framework.md](../../docs/acceptance/dual-machine-remote-framework.md)。

```powershell
# A 机：导出环境包 + 生成派发文本（不等待）
.\scripts\dual\remote-driver.ps1 -Cases SELFCHECK,A1 -PeerDeviceId <B deviceId> -DispatchOnly

# A 机（Lead）：用上面打印的 peer/thread/task 调用 mesh_run 派发给 B
#   B 机 agent 收到后执行：
#     bootstrap-student.ps1 -TeacherDeviceId <A deviceId>        # 首次
#     student-runner.ps1 -Cases SELFCHECK,A1 -RunId <RunId>      # 本轮

# A 机：等待证据回流并汇总（B 机跑完后）
.\scripts\dual\remote-driver.ps1 -Cases SELFCHECK,A1 -PeerDeviceId <B deviceId> -SkipExport
```

- 证据经 Syncthing 自动回流；mesh 只负责「派发任务」，**结论以证据文件为准**
- 环境包与证据各用一个 Syncthing 文件夹（`syncthing-setup.ps1 -ArtifactsDir` 自动创建：A=SendOnly / B=ReceiveOnly）
- 本地自测（无需第二台机器）：`remote-driver.ps1 -Cases SELFCHECK -PeerDeviceId <B sim id> -LocalStudent -LocalArtifactsDir <模拟B的artifacts目录>`
- 已验证（2026-11-18 本机双实例）：导出环境包 → A/B Syncthing 互连 → **artifacts 同步到 B** → runner 产出报告 → 证据回流 A → merged 报告生成，SELFCHECK = pass

## mesh 派发（由 Lead agent 执行）

```
mesh_run(peer: "<学生机mesh名>", thread: "dual-test-<日期>",
  task: "在 B 机执行 <repo>\scripts\dual\student-case.ps1 -Case <id> -TeacherAddr <A:47821>；
         证据写入 docs/acceptance/evidence/dual/（Syncthing 自动回流）；
         只回传脚本 stdout 的 JSON，不要改写 verdict。")
```

## 当前实现状态（v1 骨架）

- 已实现：`teacher-case.ps1` A1/A4；`student-case.ps1` A1、B4（部分）
- 未实现：其余用例返回 `not_implemented`（按方案人工执行或补实现）
- 时序类用例（C4/C5/C7）必须补：脚本内 `Stopwatch` 测量 + 判据阈值断言

## 注意

- 所有数据根隔离（`LOCALAPPDATA/APPDATA/PROGRAMDATA`），不污染本机真实数据
- `svc.json` 必须 **UTF-8 无 BOM**（否则服务读取失败）
- 脚本以 `--console` 运行 svc（前台，无服务注册），管理员类用例（TC-13/E7）仍需人工

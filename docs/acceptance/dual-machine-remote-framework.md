# 双机测试远程执行框架（B 机无人值守）

> 关联：#91（双机/管理员补验）；配套：`docs/acceptance/dual-machine-test-plan.md`、`scripts/dual/`

## 1 目标与前提

**目标**：B 机只有 DSH、没有人工。A 机的 Lead agent 通过 **mesh** 派发固定脚本任务，B 机 agent 执行用例；证据经 **Syncthing** 自动回流 A 机，A 机自动汇总报告。

**前提（B 机）**：

| 项 | 要求 |
| --- | --- |
| DSH | 已安装并可被 mesh 发现（`mesh_peers` 能看到） |
| mesh 插件 | 已安装并重启 DSH；LAN 可达或 Tailscale 连通 |
| 权限 | mesh 创建的会话**能执行 PowerShell 并写 artifacts/evidence 目录**；专用测试机建议将 mesh `permissionPreset` 设为 `danger-full-access`（或把目录放进 B 的工作区） |
| 审批 | B 机会话不能因审批弹窗阻塞（无人值守）；需关闭交互审批或自动放行固定脚本 |
| 网络 | 与 A 机同 LAN（或 Tailscale）；Syncthing 端口 22000/21027 放行 |
| 桌面会话 | GUI 用例需要 B 机已登录桌面；TC-11 需要「重启到登录界面不登录」 |

> A 机（教师）由 Lead agent 直接驱动；B 机不要求装仓库/工具链，二进制与脚本由 A 机导出的「环境包」经 Syncthing 同步过去。

## 2 架构

```
A 机（教师，有 Lead agent）                      B 机（无人值守，仅 DSH+mesh）
┌───────────────────────────┐                 ┌──────────────────────────────┐
│ Lead agent                │  mesh_run 任务  │ DSH agent（自动执行固定脚本） │
│  remote-driver.ps1        │ ──────────────▶ │  bootstrap-student.ps1       │
│   ├ export-artifacts.ps1  │                 │  student-runner.ps1          │
│   └ 等待证据 + 汇总        │                 │   ├ student-case.ps1（逐用例）│
└──────────┬────────────────┘                 └──────────┬───────────────────┘
           │                                              │
           │        Syncthing（点对点，无 SMB/SFTP）        │
           │   artifacts: A SendOnly  → B ReceiveOnly      │
           │   evidence : B SendOnly  → A ReceiveOnly      │
           ▼                                              ▼
 docs/acceptance/evidence/dual/  ◀──────────────  B 证据目录（JSON/截图/日志）
```

## 3 流程

| 步 | 执行方 | 动作 | 产物 |
| --- | --- | --- | --- |
| 1 | A（Lead） | `export-artifacts.ps1` 导出环境包（bin/scripts/cases.json/manifest 哈希） | `%LOCALAPPDATA%\ClassShareDual\artifacts` |
| 2 | A | `remote-driver.ps1 -DispatchOnly` 生成派发文本与 `run-plan` | 任务文本 |
| 3 | A（Lead） | `mesh_run(peer=B, thread=dual-<RunId>, task=...)` 派发 | 任务受理回执 |
| 4 | B agent | 首次：`bootstrap-student.ps1 -TeacherDeviceId <A>` 配 Syncthing + 自检 | `student-env.json` |
| 5 | B agent | `student-runner.ps1 -Cases ... -RunId ...` 逐用例执行（内部超时） | `dual-case-<RunId>-<id>.json` |
| 6 | B agent | 写汇总报告到证据目录 | `dual-report-<RunId>.json` |
| 7 | Syncthing | 证据自动回流 A | A 证据目录出现报告 |
| 8 | A（Lead） | `remote-driver.ps1` 等待 + 校验 + 汇总 | `dual-report-merged-<RunId>.json` |
| 9 | A（Lead） | 结论写回 M1/M2 报告与 #91 | 报告更新 |

## 4 脚本契约

| 脚本 | 位置 | 职责 |
| --- | --- | --- |
| `export-artifacts.ps1` | A | 导出 bin + scripts + cases.json + `artifacts-manifest.json`（含 SHA-256） |
| `bootstrap-student.ps1` | B（首次） | 配置 Syncthing（student）、准备证据目录、写 `student-env.json`（含 deviceId/权限/磁盘） |
| `student-runner.ps1` | B | 按 `cases.json` 执行用例（子进程 + 超时）、逐用例写 JSON、汇总 `dual-report-<RunId>.json`、stdout 输出报告 |
| `remote-driver.ps1` | A | 导出环境包 → 启动/确认 A Syncthing → 打印 mesh 派发文本 → 等待证据回流 → 校验汇总 |
| `cases.json` | 共享 | 用例清单（id/标题/mode/script/args/timeoutSec/evidenceGlob） |

**执行结果可信度**：每个用例由固定脚本产出 JSON，`verdict ∈ {pass, fail, blocked, not_implemented, timeout}`；**agent 不得改写 verdict**；A 机按 `evidenceGlob` 与报告结构校验，缺失即判失败。

## 5 派发协议（mesh）

```text
mesh_run(
  peer:   "<B 机 mesh 名>",
  thread: "dual-<RunId>",          # 同一轮复用，便于对端保留上下文
  task:   "在 B 机执行（无人值守）：
           1) 首次：%LOCALAPPDATA%\ClassShareDual\artifacts\scripts\bootstrap-student.ps1 -TeacherDeviceId <A deviceId>
           2) 本轮：%LOCALAPPDATA%\ClassShareDual\artifacts\scripts\student-runner.ps1 -Cases <ids> -RunId <RunId>
           只回传 student-runner stdout 的 JSON 与 dual-report-<RunId>.json 路径，不要改写 verdict。"
)
```

- mesh 只保证「任务送达」；**结果以证据文件为准**（mesh 回传文本仅作参考）
- 任务里只允许调用 `scripts/dual/` 下的固定脚本（不接受对端自由发挥）
- 同一 `RunId` 的证据文件名固定，避免多轮混淆

## 6 失败处理

| 现象 | 判定 | 处理 |
| --- | --- | --- |
| mesh 里看不到 B | 环境未就绪 | 检查 B 的 DSH/mesh/防火墙；LAN 探测 |
| 任务发出但无证据 | fail（超时） | 看 B 机 agent 会话记录；确认 artifacts 已同步、PowerShell 执行策略 |
| B 报告 `blocked` | 环境缺项 | 看 `student-env.json`（admin/二进制/证据目录写权限） |
| 权限被拒（workspace-write） | blocked | 调整 B 的 mesh `permissionPreset`/目录位置（见 §1） |
| 用例超时 | fail(timeout) | `cases.json` 的 timeoutSec 或 B 机负载；单用例失败不阻塞后续 |
| 证据回流缺失 | fail | `syncthing-status.ps1` 查连接/文件夹状态 |

## 7 安全

- B 机为**专用测试机**：允许 mesh 会话执行固定脚本；不接受任意任务文本
- 环境包带 SHA-256 manifest，A 机可校验 B 机实际运行的文件版本
- 证据目录只含测试数据；不共享仓库、不上传外部
- mesh 默认 LAN 无认证：测试网段使用；不可信网络开配对码

## 8 本地自测（不需要第二台机器）

```powershell
# 1) A 机 Syncthing（teacher）+ B 模拟实例（student）——用第二套端口/目录
.\scripts\dual\syncthing-setup.ps1 -Role student -PeerDeviceId <A> -StHome $env:LOCALAPPDATA\ClassShareDual\syncthing-home-b `
    -GuiPort 8385 -SyncPort 22001 -LocalDiscoveryPort 21028 -EvidenceDir $env:LOCALAPPDATA\ClassShareDual\evidence-b -LocalOnly
# 2) 框架自测：driver 在本机模拟 B 执行并等待证据回流
.\scripts\dual\remote-driver.ps1 -Cases SELFCHECK -PeerDeviceId <B sim id> -LocalStudent
```

自测验证点：artifacts 导出 → A/B Syncthing 互连 → student-runner 产出报告 → 证据回流 A → merged 报告生成。

**已验证（2026-11-18 12:39，本机双实例模拟）**：

```
[1/4] 导出环境包（svc 5.3MB + gui 12.4MB + 10 脚本 + cases.json + manifest 哈希）
[2/4] A Syncthing(8384/22000) 就绪，peer B 已添加
[3/4] 生成 mesh 派发文本（peer/thread/task）
[4/4] 本机模拟 B：student-runner SELFCHECK => pass
      报告经 Syncthing 回流 A → dual-report-merged-<RunId>.json overall=pass
```

即：**除 mesh 派发这一段（需要真实 B 机）外，其余链路已端到端验证**。

## 9 待办（依赖 B 机）

- [ ] B 机 mesh 可见（当前 `mesh_peers` 尚未发现对端）
- [ ] B 机首次 `bootstrap-student.ps1`（需 A deviceId）
- [ ] 真实用例：A1/A4/B1/B4/C4/C5/C7 的 `student-case.ps1` 实现（当前仅 A1/B4 部分实现，其余 `not_implemented`）
- [ ] GUI 类用例需要 B 机桌面会话 + 截图回传（Syncthing 证据目录）

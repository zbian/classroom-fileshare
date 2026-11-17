# scripts/dual —— 双机测试脚本（骨架 v1）

配套文档：[docs/acceptance/dual-machine-test-plan.md](../../docs/acceptance/dual-machine-test-plan.md)

## 文件

| 文件 | 运行位置 | 说明 |
| --- | --- | --- |
| `teacher-case.ps1` | 教师机（A） | 起隔离教师 svc、等待就绪/学生加入、输出 JSON 证据 |
| `student-case.ps1` | 学生机（B） | 起隔离学生 svc、等待入班、输出 JSON 证据（时序在脚本内测） |
| `run-dual.ps1` | 教师机（A） | 前置检查 + 生成执行计划与 mesh 派发文本 + 本机预演 |

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

## mesh 派发（由 Lead agent 执行）

```
mesh_run(peer: "<学生机mesh名>", thread: "dual-test-<日期>",
  task: "在 B 机执行 <repo>\scripts\dual\student-case.ps1 -Case <id> -TeacherAddr <A:47821>；
         只回传脚本 stdout 的 JSON 与 evidence 文件路径，不要改写 verdict。")
```

## 当前实现状态（v1 骨架）

- 已实现：`teacher-case.ps1` A1/A4；`student-case.ps1` A1、B4（部分）
- 未实现：其余用例返回 `not_implemented`（按方案人工执行或补实现）
- 时序类用例（C4/C5/C7）必须补：脚本内 `Stopwatch` 测量 + 判据阈值断言

## 注意

- 所有数据根隔离（`LOCALAPPDATA/APPDATA/PROGRAMDATA`），不污染本机真实数据
- `svc.json` 必须 **UTF-8 无 BOM**（否则服务读取失败）
- 脚本以 `--console` 运行 svc（前台，无服务注册），管理员类用例（TC-13/E7）仍需人工
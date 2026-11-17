# E14 内容补齐回归（#131 学员本地落库后）

## 背景
- E14（#119）修复后，学员「我共享的」按 `owner_role === 'Student'` 客户端过滤；但当时 #104 上行链路**不落本地库** → 该视图内容为空（已知缺口，#121 跟踪）。
- #131（学员本地落库 + E14 过滤）已合并 → 本次做 GUI 回归确认内容恢复且数据范围仍正确。

## 真实链路证据（release GUI + 真实 teacher/student svc + CDP 19234；非 mock）
环境：隔离 `LOCALAPPDATA/PROGRAMDATA`；teacher svc（房间 E14-131/47861）+ student svc（alice，`manual_addr` 本机）；
教师先经真实 IPC 广播 `教师广播样例.pdf`（1 条）；随后学员 GUI 通过「+ 粘贴文本」发送一条文本。

| 断言 | DOM 实测 | 结论 |
| --- | --- | --- |
| 学员发送反馈 | toast「已发送给教师」 | ✅ |
| 学员「我共享的」 | `['E14回归-学员本人提交']`，`共 1 条` | ✅ **内容补齐生效**（此前为 0） |
| 学员「我共享的」是否混入教师广播 | 否（仅本人提交） | ✅ E14 数据范围仍正确 |
| 学员「教师共享」 | `['教师广播样例.pdf']`，`共 1 条` | ✅ |

服务端日志佐证（student svc）：
```
本地落库（PendingSync，等待教师确认） item_id=… version=1
FileAck：本地提交已确认（Synced） item_id=…
```
（先本地落库→上行→教师确认后转 Synced，与 #131 设计一致）

截图：`01-student-mine-own-submission.png`、`02-student-teacher-share.png`

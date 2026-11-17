# E14 修复证据（#119）：数据范围错误（师生双向串数据）

## 根因
`apps/classroom-gui/ui/src/main/store.ts::refresh()`：
- 教师分支 `{ include_deleted: false }` —— **无 owner 过滤** → 教师「我共享的」拉到全部条目（含学生提交），
  且工具栏可对其执行删除/改名（**违反 PRD 5.5：学生提交只读**）
- 学员「我共享的」`owner: null` —— 无过滤 → 显示全部条目（含教师广播）

## 修复（GUI-only，零 IPC 契约变更）
| 视图 | List 过滤（服务端） | 客户端处理 |
| --- | --- | --- |
| 教师「我共享的」 | `{ owner: 'teacher', include_deleted:false }` | — |
| 学员「教师共享」 | `{ owner: 'teacher', include_deleted:false }` | — |
| 学员「我共享的」 | `{ include_deleted:false }`（拉全量） | **按 `owner_role === 'Student'` 过滤**（#94 已保证学员本地库中 Student 条目只可能是本人提交，故无需昵称来源） |
| 教师「学生提交」 | 走 `GuiRequests::Submissions` | 不变 |

mock 客户端（`MockGuiClient.list`）同步镜像新语义，避免 dev 视图与真实链路行为漂移。

## 已知缺口（如实记录，不在本 PR 范围）
**`#120`：学员提交不落本地库**（#104 上行链路 agent 只发帧、不写自身 store）
→ 修复后学员「我共享的」**为空**（语义正确：不再显示教师/他人数据；内容可见性依赖 #120）。
本 PR 未造数据、未用 mock 冒充真实。

## 真实链路证据（release GUI + 隔离真实 svc + CDP 19230）
环境：隔离 `LOCALAPPDATA`/`PROGRAMDATA`；teacher svc + student svc（昵称 alice，`manual_addr` 本机）。
构造数据：教师真实 IPC 广播 `教师广播样例.txt` 1 条 + 学员真实上行提交 `alice 提交1/2` 2 条
（教师 svc 日志：`broadcast … file=教师广播样例.txt`、两条 `学员提交入库 owner=alice …`）。

| 断言 | DOM 实测 | 结论 |
| --- | --- | --- |
| 教师「我共享的」行 | `['教师广播样例.txt']`，计数 **1** | ✅ 不含学生提交 |
| 教师列表中是否有学生条目 | `HAS_STUDENT_ITEM: false` | ✅ 无法代改学生提交 |
| 学员「我共享的」行 | `[]`，计数 **0** | ✅ 不再显示教师广播（内容缺失见 #120） |
| 学员「我共享的」是否含教师广播 | `false` | ✅ 数据范围已修正 |
| 学员「教师共享」行 | `['教师广播样例.txt']`，计数 **1** | ✅ 正常可见 |

截图：`01-teacher-mine-scope.png`、`02-student-mine-scope.png`、`03-student-teacher-share-scope.png`

## 门禁
worktree 内 `.\scripts\local-ci.ps1` 六项 ALL PASS（workspace 175 passed）；`crates/**` 零改动。

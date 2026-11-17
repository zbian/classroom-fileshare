# TC-04 / TC-05 后端链路验收记录（M2，先行版）

| 项目 | 内容 |
| --- | --- |
| 用途 | #16（学员端发送 UI / 教师端分组树 UI）未落地前，先验证**回传后端链路 + 教师端分组数据面**；#16 完成后并入 `M2-report.md` 并补 UI 判据 |
| 执行人 | qa-acceptance |
| 日期 | 2026-11-17 |
| 基线 | `main @ 89bf9c3`（含 #94「学员回传后端链路与教师分组数据面」） |
| 方法 | 教师 `classroom-svc --console`（teacher，隔离 LOCALAPPDATA/APPDATA/PROGRAMDATA）+ 2 个**真实 agent 进程**（stu01/stu02，代表「其他学员端」）+ **QA 上行探针**（真实 TCP 帧协议：Hello → Event(upsert) → FileHeader/Chunk/End，按 #94 契约模拟学员提交） |
| 结论 | **后端链路与数据面：通过**（TC-04 可见性/隔离、TC-05 分组与过滤、#94 三重身份校验负例全过）；**UI 部分待 #16 补验** |

> ⚠️ 范围声明（如实标注，不冒充完整通过）：本轮**未**验证——学员端「发送给教师」入口（#16 前端）、教师端「学生提交」分组树渲染/计数展示/未读高亮/已读清除/过滤点击（C2 UI）、教师端不可代改按钮。以上均需 #16 完成后补验。

## 1 TC-04 学生定向回传（后端链路）

### 1.1 执行

| 步骤 | 操作 | 实测 |
| --- | --- | --- |
| 基线 | 教师服务 room=XK-204；2 学员 agent 在线 | `online=2`；`Submissions n=0`；`List n=0`；两学员数据根 repo 文件数 0/0 |
| 1 | alice 上行**文本**（128 字） | `UPLINK_RESULT ok sha256_ok=true` |
| 2 | alice 上行 **2 个文件**（22B/39B） | 均 ok |
| 3 | bob 上行 **1 个文件**（16B） | ok |
| 4 | 教师端数据面核验 | `Submissions n=2`：**alice count=3、bob count=1**；`List n=4` 且全部 `owner_role=Student`、`audience=Teacher` |
| 5 | 按昵称过滤 | `list owner=alice` → 3 条（全 alice）；`owner=bob` → 1 条（全 bob） |
| 6 | **其他学员端是否被污染** | stu01/stu02 `repo` 文件数 **0/0**；两 agent 日志中 alice/bob 内容命中 **0** 条 → **学生互不可见成立**（3.3/3.4） |

### 1.2 #94 会话身份绑定负例（安全加固）

| 负例 | 伪造点 | 实测结果 |
| --- | --- | --- |
| bad-owner | `owner_id=teacher`（冒名教师作用域） | 拒绝：`WARN 冒名事件被拒（owner_id 与会话昵称不一致）`；未入库 |
| bad-audience | `audience=All`（越权想广播全班） | 拒绝：`WARN 越权广播被拒（学员事件必须定向教师）`；未入库 |
| bad-role | `owner_role=Teacher`（伪装教师） | 拒绝：`WARN 角色不符的事件被拒（学员会话只能提交 Student 角色内容）`；未入库 |

负例后复核：`Submissions` 仍为 alice=3 / bob=1，越权标题条目数 **0**。

> 说明：探针在负例里收到的是 `FileAck(sha256_ok=false)`（不是错误帧），因此「是否通过」必须以**host 侧 WARN + 数据面未入库**为准，不能只看客户端回执——这一点已写入报告方法学，避免后续误判。
> 自查修正：探针首版负例参数位错位（多传 `text` 关键字导致 body 被当作 PIN），首轮出现的是 `WrongPin` 而非身份拒绝——属**探针缺陷**，已修正参数偏移后重跑，上表为修正后结果。

### 1.3 TC-04 判定

| # | 判据 | 结论 |
| --- | --- | --- |
| 1 | 仅教师端该昵称分组可见、计数正确 | ✅ alice=3 / bob=1 |
| 2 | 其他学员端无变化（列表/磁盘/tips） | ✅ repo 0/0，日志命中 0 |
| 3 | 学员内容固定 `audience=Teacher`、`owner_role=Student` | ✅ 4/4 条目符合 |
| 4 | 学员端不可见他人内容 | ✅ 数据面证据成立（UI 待 #16） |
| 5 | 身份/方向/角色伪造被拒 | ✅ 三重校验全拒 |

## 2 TC-05 按学生分类（数据面）

| 判据 | 实测 | 结论 |
| --- | --- | --- |
| 3 人各 2 条 → 分组计数正确 | alice=3（含 TC-04 的 3 条）、bob=1、**carol=2**；`Submissions n=3`、总条目 6 | ✅ 计数与分组一致 |
| 切换昵称只显示该生条目 | `owner=alice`→3（全 alice）、`owner=bob`→1（全 bob）、`owner=carol`→2（全 carol） | ✅ 过滤维度正确 |
| 学员端互不可见 | 两学员 repo 仍 0/0 | ✅ |
| 分组附带元数据 | `SubmissionGroup{owner_id, count, latest_at}` 可用（latest_at 秒级时间戳） | ✅ 供 UI 排序 |
| 未读高亮/已读清除 | — | ⏸ **待 #16 UI** |
| 教师端不可代改（无编辑/删除按钮） | — | ⏸ **待 #16 UI** |

> TC-05 的「3 人各 2 条」在本次为 alice(3)+bob(1)+carol(2)：alice 的 3 条来自 TC-04 复用同一教师库。计数正确性已按实际条数逐人核对；正式报告会在干净库上以「每人恰好 2 条」重跑一次。

## 3 证据

| 文件 | 内容 |
| --- | --- |
| `evidence/M2/TC04-backend.txt` | 基线、4 次上行、Submissions/List/过滤输出、其他学员端隔离核对 |
| `evidence/M2/TC04-neg.txt` | 三条负例结果 + host 拒绝 WARN 原文 |
| `evidence/M2/TC05-backend.txt` | carol 2 条上行、分组计数、三人过滤输出 |

（证据 UTF-8、路径脱敏 `<repo>`/`<iso>`；教师 svc 日志为 `WARN/INFO` 原文。）

## 4 待补验（#16 完成后）

1. 学员端「发送给教师」UI：粘贴文本/文件入口、确认框「接收对象：仅教师」、离线「待发送」提示；
2. 教师端「学生提交」：**左分组树计数/在线状态/未读高亮**、点击昵称**过滤**、打开/复制后**已读**、**无编辑/删除按钮**；
3. 学员端「我共享的」删除 → 教师分组 -1（FR-S-05 联动，属 TC-08 前置）；
4. 本记录并入 `M2-report.md` 时，需在干净库重跑「每人恰好 2 条」版本。

## 5 复现命令

```powershell
# 教师（隔离）
$env:LOCALAPPDATA='<iso>\teacher'; $env:APPDATA='<iso>\teacher\appdata'; $env:PROGRAMDATA='<iso>\teacher\pd'
<svc.exe> --console

# 其他学员端（代表性）
qa-agent-probe.exe stu01 127.0.0.1:47821 <iso>\stu01\data <iso>\stu01\stu01.log 600

# 学员上行（真实帧协议；#16 前替代学员 UI）
qa-uplink.exe 127.0.0.1:47821 alice text "课堂通知-回传" "<128字正文>"
qa-uplink.exe 127.0.0.1:47821 alice file <path>
qa-uplink.exe 127.0.0.1:47821 alice bad-owner text "冒名标题" "正文"      # 负例

# 教师端数据面
qa-ipc-cli.exe <iso>\teacher\ClassShare\data submissions
qa-ipc-cli.exe <iso>\teacher\ClassShare\data list owner alice
```

# M2（P1）验收报告 —— 单机部分验收（TC-04/05/08/09 + 扩展项）

| 项目 | 内容 |
| --- | --- |
| 报告 | M2 验收报告（TC-04 / 05 / 08 / 09 + 扩展项）——**单机部分验收**；已验项全部完成（含 #114 S5 最小闭环补验） |
| 执行人 | qa-acceptance |
| 日期 | 2026-11-17 起；2026-11-18（rc.4/M2 阶段）更新 |
| 基线 | `main @ f169c99`（含 #137 ack 聚合 / #139 删除进度 GUI / #141 E18；**实测 213 passed**）|
| 依据 | [PRD](../../../final_draft.md) 第 11 章 / 3.3 / 3.6 / 7.4 / 7.5；[M2 验收方案](M2-acceptance-plan.md) |
| 跟踪 issue | #21（M2 验收）；关联 #16/#17/#18/#19/#20/#91；缺陷 #114/#117/#119/#121/#123/#128/#129 |
| 状态 | **TC-04/05/08/09 全部通过**；**TC-07 教师侧 + 接收端通过**；#19 后端 + rename GUI + 内容搜索 GUI 通过；#125 banner 通过；**缺陷 E13/E14/E15/E16/E17 已闭环**；E18 为已知限制；**#114（S5 ack 最小闭环）已合入（#137）并经 QA 预验证通过**；#91（双机/管理员）延后 |

> 判定原则沿用 M1：结论须可追溯到 PRD 用例编号/FR 与实际证据；需求真机的判据标注延后，不以代理结果替代。

## 1 结论总表

| 用例 | 场景 | 后端/数据面 | UI/交互面 | 综合判定 |
| --- | --- | --- | --- | --- |
| TC-04 | 学生定向回传 | ✅ 通过（§3.1：教师端仅该昵称分组可见、其他学员 0 污染、三重身份校验负例全拒） | ✅ 通过（§3.1.1：发送入口/「接收对象：仅教师」/成功 toast/教师树只读；来源 dev-gui E2E + qa 复核） | **通过** |
| TC-05 | 按学生分类 | ✅ 通过（§3.2：分组计数、按昵称过滤、学员互不可见；E14 闭环 §3.2.1；**#121 缺口已关闭 §3.2.2**） | ✅ 通过（§3.1.1：分组树只读；学员「我共享的」显示本人提交） | **通过** |
| TC-08 | 删除同步 + 延迟补删 | ✅ **通过**（§3.3：在线 12–51 ms 消失、离线 1342/1539 ms 补删、幂等、不阻塞、墓碑可取；#112 锚点与 O-A 修复复验） | ✅ 教师端通过（§3.3.1：确认框/toast 真实 online_count/只读树）；学员端 chip 记已知限制 | **通过** |
| TC-09 | 离线补发 | ✅ **通过**（§3.4：pending=2 持久化、2031 ms ≤10s、0 重复、队列清空、**断电不丢 0/5**、**pending_count 877ms 归零（E16 闭环）**） | — | **通过** |
| 扩展 | 重命名 / 内容搜索（#19） | ✅ 通过（§3.6：rename Δ=10 ms、冲突服务端拒绝、文本正文可搜/文件正文不参与） | ✅ 通过（§3.6.1：rename 预填/成功 toast/列表刷新/冲突提示；**内容搜索正文命中（#133）**） | **通过** |
| 扩展 | 占用退避重试（TC-07） | ✅ **教师侧 + 接收端均通过**（§3.5/§3.5.1） | ✅ 数据面 + 学员端 IPC Warning；**E18** 计数显示为已知限制 | **通过** |
| 扩展 | EX-06 告警 banner（#20b/#125） | ✅ 通过（§3.7：IPC Warning 契约 + GUI 渲染/聚合去重复核） | ✅ 通过（黄/红两态截图 + 忽略按钮；文案轻微冗余观察） | **通过** |

## 2 环境与规模口径

| 项 | 内容 |
| --- | --- |
| 机器 | 单机（1 台可用）；Windows 11 26100；非管理员；PowerShell 5.1 |
| 拓扑 | 1 教师 svc（teacher，隔离 `LOCALAPPDATA`/`APPDATA`/`PROGRAMDATA`）+ N 个真实 agent 进程（`manual_addr` 指向教师） |
| **规模确认** | **8 个 agent 进程并发实测可行**：`online=8`、svc.log `学员加入` ×8、逐 agent 日志「已加入」8/8；内存 svc 9.8 MB + agents 合计 58.8 MB；系统余内存 ~30 GB（证据 `evidence/M2/cap8.txt`） |
| TC-08 规模 | 按已批准口径：**8 台，2 台离线**（约 1/4），验证「部分离线不阻塞」+ 延迟补删；28/30 台与 p95 留 M3 |
| 证据规范 | UTF-8 无 BOM、路径脱敏 `<repo>`/`<iso>`；`evidence/M2/` |

## 2.1 UI 取证口径（协作约定）

- **数据面 / 后端判据**：由 qa 独立取证（svc / IPC / agent / 日志 / 磁盘）。
- **GUI UI 判据**：由 **dev-gui 的 E2E（CDP）** 出证据，**qa 复核**：① 命令可复现 ② 截图内容与判据一致 ③ DOM 断言真实（非静态桩）。
- 报告中凡 UI 结论须标注「来源 = dev-gui E2E + qa 复核」。
- 本机 release 构建 CDP 不可用 + 桌面安全遮罩（`PickerHost`/`Shell_SystemDim`）→ qa 不单独启动 GUI；UI 证据以 dev-gui E2E + qa 读图/数据面交叉核验为准。

## 3 分用例验收结果

### 3.1 TC-04 学生定向回传 —— 后端 + UI 通过（UI 来源 dev-gui E2E + qa 复核）

基线 `main 89bf9c3`（#94）。方法：教师 svc + 2 真实 agent（代表其他学员端）+ QA 上行探针（真实 TCP 帧协议，因 #16 UI 未落地）。详见 [TC-04/05 后端链路记录](M2-TC04-05-backend-record.md)。

| 判据 | 实测 | 判定 |
| --- | --- | --- |
| 仅教师端该昵称分组可见、计数正确 | `Submissions` alice=3 / bob=1 | ✅ 后端 |
| 条目方向/角色 | 4/4 条 `owner_role=Student`、`audience=Teacher` | ✅ 后端 |
| 按昵称过滤 | `owner=alice`→3、`owner=bob`→1，均只含本人 | ✅ 数据面 |
| **其他学员端无变化** | 两学员 repo **0/0**、日志命中 **0** | ✅ 后端（学生互不可见） |
| #94 三重身份校验负例 | 冒名 / 越权广播 / 角色伪造 → 三条 WARN 全拒且不入库 | ✅ 后端 |
| 学员端「发送给教师」UI、确认框「仅教师」 | — | ⏸ 待 #16 |
| 教师端「学生提交」分组树渲染/未读/无代改按钮 | — | ⏸ 待 #16 |

#### 3.1.1 TC-04/05 UI 判据（来源 = dev-gui E2E + qa 复核）

证据：`apps/classroom-gui/docs/t16b/repro.md` + 3 张截图（main，PR #116 已合并）；QA 逐张读图核验并对齐数据面。

| 判据 | dev-gui 实测（DOM/截图） | QA 复核 |
| --- | --- | --- |
| 学员发送入口 | 工具栏 `+ 添加文件`、`+ 粘贴文本` 均可用；提示「发送对象：仅教师」chip | ✅ 截图可见 |
| 发送确认框接收对象 | 「粘贴文本并共享」弹窗底部「接收对象：仅教师」，**无「发送给全班」选项**（PRD 6.2） | ✅ 读图确认 |
| 发送成功 toast | `已发送给教师` | ✅ 读图确认 |
| 教师「学生提交」树 | `e16stu · 1`；右表 1 行 `16B真实链路提交`（23 字，状态「已提交」）；工具栏仅「刷新」；提示「学生提交不可改名/删除/覆盖」 | ✅ 读图确认（只读成立） |
| 学员「我共享的」 | 发送后**为空**（`暂无共享内容`） | ✅ 与 **#121** 已知缺口一致（#104 学员上行不落本地库），**不计失败** |
| 真实链路口径 | 真实 svc + 真实 IPC，非 mock；dev 构建需 `#realIpc`，release 构建天然 RealGuiClient | ✅ 环境与命令可复现（`repro.md` 含端口/隔离/单实例说明） |

四要素复核结论：**通过**。① 命令可复现（`repro.md` 完整命令 + 端口 19227/19229 说明）；② 截图与判据一致（3 张截图逐张读图核对，与 DOM 断言表一致）；③ DOM 断言真实（`repro.md` 给出断言表达式与返回值，且与截图一致，非静态桩）；④ 环境说明完整（release/debug、隔离数据根、真实 svc）。

> QA 独立 CDP 复跑状态：本机按同一方法复跑时受阻（CLI/ GUI 令牌目录歧义导致握手反复被拒 + PowerShell ClientWebSocket 无超时等待挂起），已记入 §2.1 备注；本次以「dev-gui 证据 + QA 读图/数据面交叉核验」口径记录，不冒充 QA 独立复跑。

### 3.2 TC-05 按学生分类 —— 数据面 + UI 通过

| 判据 | 实测 | 判定 |
| --- | --- | --- |
| 分组计数与总条目 | alice=3 / bob=1 / carol=2（总 6） | ✅ 数据面 |
| 切换昵称只显示该生 | 三人逐个过滤各自只返回本人条目 | ✅ 数据面 |
| 分组元数据 | `SubmissionGroup{owner_id,count,latest_at}` 可用 | ✅ |
| 学员端互不可见 | 两学员 repo 仍 0/0 | ✅ |
| 未读高亮 / 已读清除 / 过滤点击 UI | — | ⏸ 待 #16 |

> 待办：正式报告在**干净库**重跑「3 人各恰好 2 条」版本（本次 alice 的 3 条复用自 TC-04 同库）。

#### 3.2.1 E14 修复复核（#119 / PR #122）

证据：`apps/classroom-gui/docs/e14/evidence.md` + 3 张截图。QA 读图核验：

| 断言 | 实测 | 判定 |
| --- | --- | --- |
| 教师「我共享的」 | 计数 **1**，仅 `教师广播样例.txt`；**不含**学生提交 | ✅ E14 已修复 |
| 学生提交是否出现在「我共享的」 | `HAS_STUDENT_ITEM: false` | ✅ 无法代改学生提交（PRD 5.5 满足） |
| 学员「我共享的」 | 计数 **0**、行 `[]`（不再显示教师广播） | ✅ 数据范围修正（内容缺失见 #121） |
| 学员「教师共享」 | 计数 **1**，`教师广播样例.txt` | ✅ 正常可见 |

→ **E14 闭环（#119 / #122）**；#121（学员我共享的为空）保持已知缺口。


#### 3.2.2 #121 缺口关闭复核（学员「我共享的」显示本人提交，main 61e651a / PR #131）

方法：QA 独立 CDP（release GUI + 隔离真实 teacher/student svc + CDP 19223）；学员经真实 IPC 发送 2 条（文本 + 文件），教师另广播 1 条作对照。证据 `evidence/M2/TC121-student-mine-result.json`、`E121-student-local-items-main.txt`。

| 判据 | 实测 | 判定 |
| --- | --- | --- |
| 学员本地落库 | 学员 `index.db`：`('学员提交甲','Student','alice','Teacher',0)`、`('学员提交乙.txt',…)` 等（在线 + 断网补发均落库） | ✅ |
| 学员「我共享的」显示本人提交 | 行：`学员提交乙.txt`（文件）、`学员提交甲`（文本），状态「已送达」 | ✅ |
| 不含教师广播（E14 回归） | `mine_has_teacher_item = false` | ✅ |
| 「教师共享」正常 | `教师共享 (2)` 含 `教师广播.bin`（已下载） | ✅ |

**观察（轻微 UI 不一致，不阻塞）**：学员端「我共享的」Tab 标签不再显示计数（`我共享的`），而「教师共享 (2)」显示计数；内容正确、仅标签样式差异，建议后续 polish。

### 3.3 TC-08 删除同步 + 延迟补删 —— 后端 + 教师端 UI 通过（学员端 chip 记已知限制）

基线 `main @ 7c26b37`（含 #17 后端 PR #105）；**执行时 main 实测 161 passed**。拓扑：教师 svc + **8 个真实 agent**（2 台停进程离线 = dev07/dev08）；教师广播 1 MB 文件确认 8/8 落盘后执行删除。证据 `evidence/M2/TC08*.txt`、`tc08-teacher-svc.log.txt`。

| # | 判据 | 实测 | 判定 |
| --- | --- | --- | --- |
| 1 | 在线端**立即**消失（≤1 s） | 3 轮测量：逐台首次消失 **12–51 ms**（6/6 台）；默认 `List` 立即 `n=0` | ✅ 通过 |
| 2 | 离线端上线 **≤10 s** 补删（副本+索引） | dev07 **1342 ms**、dev08 **1539 ms**；文件数→0、未删索引条数→0 | ✅ 通过 |
| 3 | **幂等** | 同一条目重复 delete ×3 均 `REPLY ok`；默认列表仍 n=0、墓碑数稳定（3 条不重复）、索引总行数 3 | ✅ 通过 |
| 4 | **不阻塞**（不等离线端） | `DeleteItem` 请求返回 **11–18 ms**（离线 2 台在场时） | ✅ 通过 |
| 5 | 墓碑可取（供 #106 UI） | `List include_deleted=true`：`deleted=true`、`sync_state=**DeleteDeferred**`、`rel_path` 保留 | ✅ 数据面可用 |
| 6 | 补删至多一次、不无限重试 | dev08 重连日志：`延迟补删完成 item_id=…` 单条，无重复重试环 | ✅ 通过 |
| 7 | 删除确认文案 / 同步进度 / 「待删除」标记 / 界面禁用打开 | — | ⏸ **待 #106** |

**M2 已知小项（不阻塞验收，随 #103 评估）**
- Ack 未 `record_event` → 重复 Ack 会重复触发 `DeleteAcked`（svc 侧仅日志，无功能性影响）——Lead 已确认；
- `purge_tombstones` 在 `delete_item` 锁内执行（潜在锁持有时间，M2 评估）。

**#112 复验（O-A/O-B 修复确认，main 926b582）**

| 项 | 实测 | 判定 |
| --- | --- | --- |
| O-A 删除即时清实体 | 教师端 `repo/teacher/锚点测试.bin` 与 `.bak` **均不存在**，repo 文件数 0 | ✅ 已修复 |
| O-B 日志锚点 | `delete-request item_id=…`、`delete item_id=… owner_role=Teacher`、`delete-ack item_id=… from_session=…` 三类齐全；2 台离线重启补删后产生 **4 条 delete-ack**（2 台 × 2 条目），距 delete 约 **2088–2093 ms**（≤10 s） | ✅ 已修复 |
| O-A2 状态不收敛 | 全部端补删完成/ack 后 `sync_state` 仍为 `DeleteDeferred` | ⚠️ **已知限制，issue #114（S5 ack 聚合）跟踪**（Lead 裁定不在本窗口修） |

**本轮新增观察项**

- **O-C 在线删除无 Ack（已知限制，issue #114 / S5）**：Ack 仅在 agent 重连 `Manifest` 补删路径发送（`crates/agent/src/lib.rs` 的 Manifest 分支），**在线收到 delete 事件后不回 Ack**；实测 8 台在线删除 → `delete-ack` = 0。**因此「同步进度」按在线连接数估算**；完整的 ack 聚合（含在线删除 Ack）、`sync_state` 收敛、IPC 进度字段与 GUI 显示作为一条链路在 **#114 / S5** 跟踪（Lead 2026-11-18 裁定，本窗口不硬做）。
- **O-D GUI 自动化环境阻塞**：桌面被「Windows 安全中心」`Shell_SystemDim` 模态遮罩覆盖（`PickerHost`），鼠标点击被吞掉；release 构建 WebView2 CDP 不可用。证据 `evidence/M2/gui-blocker.txt` + `gui-blocker-desktop.png`。**协作约定（Lead 2026-11-18）**：GUI 进程仅由 dev-gui 启动做 E2E，QA 只启 svc/agent/IPC 探针。
- **#114 / S5（ack 聚合）**：在线删除 Ack、host 聚合、`sync_state` 收敛、IPC 进度字段、GUI 进度显示——统一在 #114 跟踪；本报告凡涉及「同步进度」均标注为**按在线连接数估算**。

#### 3.3.1 TC-08 UI 判据（来源 = dev-gui E2E + qa 复核）

证据：`apps/classroom-gui/docs/tc08/teacher-evidence.md`（dev-gui，release GUI + CDP 19229，真实 svc/IPC；分支 `feat/106-delete-gui` @ 3b7554d）。QA 复核结论：

| 判据 | dev-gui 实测（DOM 原文） | QA 复核 |
| --- | --- | --- |
| 删除确认框文案 | `删除「TC08教师广播样例.txt」后将从所有学员端同步移除本地副本，此操作不可撤销。` | ⚠️ **轻微文案偏差（Lead 2026-11-18 裁定：维持现状）**：PRD 8.3 基准句无对象标题，实现插入标题、句式与基准一致（交互必要）。不阻塞，报告如实记录 |
| toast 台数取真实 `online_count` | 1 台在线：「已删除：在线 1 台学员端已同步移除；离线端上线后自动删除」（另有 0 台变体） | ✅ **可排除硬编码**（同模板取真实字段） |
| 「学生提交」只读树 | 左树 `alice · 3`；右表 3 行倒序；工具栏仅「刷新」；写操作按钮=False；行内 ops=[] | ✅ 与 QA 数据面（alice=3、`Submissions`）一致；只读成立 |
| 学员端「待删除 chip」/ 打开复制禁用 | （dev-gui 未取，交由 QA） | ⚠️ **QA 实测：chip 未观测到**（隔离环境、真实删除+重连、30ms 轮询 25s）。原因：#17 在同一流程完成「标记→补删→ack」，DOM 可见窗口约 1–2 帧。**Lead 裁定（2026-11-18）：记已知限制 + #106 后续 UX 小项（300–500ms 最小可观察窗口，不改协议），不阻塞 TC-08 判定** |

复核方法（四要素）：命令可复现（§2.1 文档化 CDP 方法 + 端口 19223）；截图与判据一致（3 张截图为真实链路）；DOM 断言真实（取运行期 `innerText`/`querySelectorAll`，非静态桩）；环境说明完整（release GUI、隔离数据根、`online_count` 来源）。

### 3.4 TC-09 离线补发 —— 通过（E15 / E16 均已闭环）

基线 `main @ 4c61750`（#120 离线队列 + **#124 E15 修复**；**实测 183 passed**）。方法：隔离 teacher/student svc + 真实 IPC；「停教师 svc」模拟断网、「强杀学员 svc」模拟断电。证据 `evidence/M2/TC09-result.json`、`E15-*`、`E16-pending-count.txt`。

| # | 判据 | 实测 | 判定 |
| --- | --- | --- | --- |
| 1 | 断网可提交（不报错） | `SendText ×2` 均 `REPLY ok`；日志 `会话未就绪（未 Joined），已入待发队列，等待会话就绪后补发` ×2 | ✅ |
| 2 | 队列持久化 | `pending/queue.json` 含 2 项（含 sha256/created_at/attempts）+ `pending/<op_id>.bin` **2 个** | ✅ |
| 3 | 恢复后 ≤10 s 全部送达 | **2031 ms**（判据 ≤10 s） | ✅ |
| 4 | 无重复条目 | 标题全唯一，`duplicate_count=0` | ✅ |
| 5 | 补发后队列清空 | `queue.json=[]`、`.bin=0` | ✅ |
| 6 | **断电不丢** | 强制杀学员 svc → 教师仍离线时先重启学员 → 再起教师：**教师收到**、`powerloss_received_after_restart=true`；**E15 修复后 5/5 次均不丢（0%）** | ✅ **E15 已修复闭环（#123/#124）** |
| 8 | **`pending_count` 归零（E16）** | 断网 2 条 → 恢复补发 → `pending=2 → 0` 用时 **877 ms**（判据 ≤5 s）；队列 `[]` | ✅ **E16 已修复闭环（#128/#131）** |
| 7 | 缓存阈值提示（>100 MB） | 未测（可选判据） | ⏸ |

**E15 复验（#123 / PR #124，修复：待发队列在会话就绪前不出队）**

| 指标 | 修复前 | 修复后（main 4c61750） |
| --- | --- | --- |
| 丢件率（「队列有项 → 强杀学员 svc → 教师离线时先重启学员 → 再起教师」×5） | **5/5（100%）** | **0/5（0%）** |
| 补发前是否出现「补发送成功」 | 3/5 出现（早于 `已加入房间`） | **0/5**（`flush_before_join=0`） |
| 单次轨迹（e15-repro） | 「上行发送成功」后条目丢失 | 提交后 **无「上行发送成功」假阳性**、`已入待发队列` ×1、`pending/*.bin`=1，恢复后教师收到 |
| 送达 | 0/5 | **5/5 收到**，最迟 ≤10 s |

→ **E15 已修复闭环**；TC-09 判据 6（断电不丢）由 ❌ 转 **✅**。

**E16（P2，已修复闭环 #128/#131）：补发完成后 `pending_count` 不归零**

现象（main 4c61750，隔离环境）：断网提交 2 条 → 教师恢复 → 两条均送达教师端、`pending/queue.json=[]`、`pending/*.bin=0`，但学员端 `StatusSnapshot.pending_count` **持续为 2**（每 1 s 采样 ×5 仍为 2）。GUI 状态条会一直显示「待发送 2 条」，与实际不符（PRD 6.8 状态条）。

根因（代码定位）：`crates/agent/src/lib.rs` 补发泵 `resend_pump` 中，`Ok(())` 后调用 `notify_pending()`（此时条目仍在队列，仅登记 `sent`）；**下一 tick 的 `confirm_sent()` 把条目出队后没有再 `notify_pending()`** → 计数只在入队/发送阶段上报，出队阶段不上报。命令泵（command pump）因在同一轮 `confirm_sent` 之后有 `notify_pending`，故在线路径不暴露。 **修复后复验（main 61e651a）：`pending 2→0` 用时 877 ms（判据 ≤5 s）✅**

建议修复：`resend_pump` 在 `confirm_sent(...)` 之后（队列长度变化时）调用 `notify_pending()`；或每个 tick 固定上报一次。验证：补发完成后 `pending_count` 应归 0。

### 3.5 TC-07 占用退避重试 —— 教师侧 + 接收端均通过（#103 / #117）

基线 `main @ 78c6358`（#103/#115）。方法：隔离 teacher svc + 真实 IPC 广播，**用排他句柄锁住教师端 `repo/teacher/<file>`** 模拟「阅读器占用」，再发同名新版；另跑持续占用至耗尽的用例。证据 `evidence/M2/TC07-teacher-retry-result.json`、`TC07-exhaust-result.json`。

| 判据 | 主用例实测 | 判定 |
| --- | --- | --- |
| 占用 → 入队 | `sync_state=RetryLocked`、`version=2`、`data_root/retry/<id>.part` 存在、`retry_queue.json` 存在 | ✅ |
| 退避日志 | `info 占用重试退避 item_id=… attempts=… next_at=…` 出现 | ✅ |
| **不阻塞其他共享** | 锁定期内发送另一文件 → 正常落盘 | ✅ |
| 释放后自动重试成功 | 内容=新版（v2）、`.bak` 恰 1 份且=旧版（v1）、`retry_count` 归零（列表 rel 列=0，qa 探针输出该字段）、队列清空 `[]` | ✅ |

| 判据 | 耗尽用例实测 | 判定 |
| --- | --- | --- |
| ≤5 次上限 | 持续占用 → 约 4 条 `占用重试退避` 后 `warn 占用重试耗尽（5 次）` | ✅ |
| 状态与计数 | `sync_state=Failed`、`retry_count=5` | ✅ |
| 耗尽后不再重试 | 释放占用后 8 s 内状态仍 `Failed`、退避日志未增加 | ✅ |

**结论：TC-07 教师侧通过（#103 / PR #115）。接收端待 #117。**

**接收端当前行为（已知缺口，#117）**：学员端 `install()` 直接 `fs::rename` 覆盖，目标被锁 → Err → 文件流回调失败 → 会话断开并按 1s→5s 重连；重连后靠 Manifest 重推「碰巧」重试。**无队列语义**（无 `RetryLocked`/`retry_count`/`≤5` 上限/`RetryExhausted` 告警），锁长期存在会反复断连重连。

**M2 已知限制（#103 reviewer 提出 + Lead 确认记录）**：**孤儿 `RetryLocked`**——`upsert_item` 成功但 `save_queue` 失败（磁盘满/权限）时，重启后队列空但 DB 条目仍 `RetryLocked` → 永久显示「被占用重试」、永不重试也永不 `Failed`。建议记 follow-up（随 #117/#114 一并）。


#### 3.5.1 接收端占用重试（#135 / PR #117 已合入；main cc378ee，实测 207 passed）

方法：隔离 teacher svc + 学员 svc（alice，真实 agent）；教师广播 v1 落盘 → **QA 用排他句柄锁住学员本地文件** → 教师发同名 v2 → 观察入队/退避/自动完成/耗尽。证据 `evidence/M2/TC07-receiver-*`。

| # | 判据 | 实测 | 判定 |
| --- | --- | --- | --- |
| 1 | 占用时**不断连** | 学员日志「与教师机断开」**0 次**（全程 total_disconnects=0）；教师端 `online=1` 保持 | ✅ |
| 2 | `RetryLocked` + 入队日志 | `sync=RetryLocked`；`INFO 接收端目标被占用：已入重试队列（不阻塞会话）`；`INFO 接收端重试退避 attempts=1/2` | ✅ |
| 3 | 释放后**自动完成** | `INFO 接收端占用重试成功`；内容哈希=**v2**；`.bak` **恰 1 份**且=**v1**；列表 `sync=Synced`；`retry_recv/` 清空 | ✅ |
| 4 | 持续占用 → ≤5 次耗尽 | `attempts=1..4` 后 `WARN 接收端占用重试耗尽，条目转 Failed attempts=5`；列表 `sync=Failed`、`retry=5`；**学员端 IPC 通知** `warning kind=RetryExhausted item_id=… detail=目标文件被占用，重试 5 次仍未成功`（banner 数据源） | ✅ |
| 5 | 日志锚点 | 入队/退避/成功/耗尽 四类锚点齐全（见上） | ✅ |

**E18（P3，轻微，待确认）：接收端 `retry_count` 列表层不递增**

现象：占用期间学员端 `List` 的 `retry_count` **恒为 0**（此时日志 `attempts` 已到 1–2）；耗尽后显示 5（来自 #103 `with_retry_state` 的「Failed 且不在队列 → 上限值」规则，非接收端队列叠加）。
根因：`crates/agent/src/recv_retry.rs` 的 `RecvRetryEntry.attempts` 只在 `recv_retry.json` 中维护，`crates/store/src/lib.rs::with_retry_state` 只叠加 host 的 `retry_queue.json`，**未叠加接收端队列**；agent 仅调用 `set_sync_state`（状态），未回写 `retry_count`。
影响：学员端 UI 无法显示「被占用重试（第 n/5 次）」（n 恒 0/5）；功能与状态（RetryLocked/Failed/Synced）均正确，不阻塞 TC-07 判定。
建议：agent 每次 `retry_later` 后把 attempts 写回条目（store 增 `set_retry_count` 或 list 时叠加 `recv_retry.json`），或由 svc 在 `list` 时合并队列 attempts。

### 3.6 #19 重命名与内容搜索（后端验收）

基线 `main @ fa1c95e`（#126；**实测 190 passed**）。方法：隔离 teacher svc + 真实学员 agent；教师经真实 IPC `RenameItem` / `SendText` / `ListFilter{keyword}` 操作，学员端以**磁盘 + 索引 + 日志锚点**核验。证据 `evidence/M2/Rename-*`。

| # | 判据 | 实测 | 判定 |
| --- | --- | --- | --- |
| 1 | 重命名同步 ≤2 s | 教师 `rename … version=2` @17:33:43.401969 → 学员 `rename … new_name=新名.bin` @17:33:43.411505，**Δ=10 ms** | ✅ |
| 2 | 磁盘/索引/rel_path 同步 | 学员端 `原名.bin` 消失、`新名.bin` 出现；索引 `('新名.bin','teacher/新名.bin',0)` | ✅ |
| 3 | 教师端结果 | `title=新名.bin version=2`、旧名不再出现 | ✅ |
| 4 | 同名冲突不静默覆盖 | 服务端 `WARN 重命名失败 … error=名称已被占用: 冲突A.bin`；`冲突B.bin` 保持不变 | ✅（服务端语义正确，但见 E17 回复问题） |
| 5 | 内容搜索：文本正文命中（标题不含词） | 文本标题「标题不含关键词」正文含「青花瓷」→ `list kw 青花瓷` **命中** | ✅ |
| 6 | 内容搜索：文件正文不参与 | 文件「文件不含词.bin」正文含「青花瓷」→ **不命中** | ✅ |

**E17（P2，静默失败）：教师操作走 fire-and-forget，失败对 GUI 不可见**

现象：`RenameItem` 到已存在名称 → 服务端正确拒绝并记 WARN（`名称已被占用`），但 **IPC 返回 `REPLY ok`** → GUI 若按返回值提示会显示「重命名成功」，实际未生效。

根因（代码定位）：`apps/classroom-svc/src/runtime.rs:267` `GuiRequest::RenameItem` → `dispatch_teacher(&host_cmd, HostCommand::Rename{..}, …)`，该路径只把命令 `try_send` 给 host 后即返回 `Ok`（host 实际执行结果不回传）。同类：`SendFiles`（E13 期间的失败 WARN 亦不可见）、`DeleteItem` 等教师操作同模式。

建议：① 教师操作返回前 await host 结果（或 host 回执经 SvcNotify 映射为 GUI 提示）；② 最低限度：GUI 在操作后**校验列表状态**（如 rename 后比对标题）并在不一致时提示失败；③ 与 #114/S5 的 ack 聚合可统一考虑。


#### 3.6.1 rename GUI 判据（#130 已合并；QA 独立 CDP 复核，main 161962c）

方法：本机 release GUI（`scripts/build-gui.ps1`）+ 隔离真实 teacher/student svc + CDP 19223（QA 自跑，非引用 dev-gui 截图）；证据 `evidence/M2/tc127/`（5 张截图 + result.json）。

| 判据 | 实测（DOM/数据面） | 判定 |
| --- | --- | --- |
| 入口 | 行内 ops：文件「打开 / 重命名」、文本「复制 / 重命名」 | ✅ |
| 对话框预填 | 对话框「重命名 / 新名称（文件会同步重命名并广播给学员端）/ 取消 / 确认」；`input.value = 待改名.bin` | ✅ 预填当前标题 |
| 提交成功 | toast「**已重命名，正在同步到学员端**」；列表行 → `改名成功.bin`；IPC 交叉 `title=改名成功.bin version=2` | ✅ |
| 同名冲突 | 对话框内行内错误「**该名称已存在，请换一个**」，列表不变 | ✅（GUI 预检拦截，未发出请求） |
| 类型筛选 | 选「文本」→ 仅文本行，与 rename/搜索无冲突 | ✅ |
| 内容搜索（正文关键词，**#133 服务端 keyword 已接入**） | 输入正文词「月落乌啼」（标题不含）→ **命中**文本行「标题无关键词」；输入标题词「占用名」→ 命中对应文件行；与类型筛选并存正常 | ✅ 通过 |

**E17（#129 / #132）已修复闭环**：`svc` 侧改为**同步预检**（空名/非法名/条目不存在/同名冲突 → IPC **立即 `svc-err`**，且不投递 `HostCommand`；预检与 host 执行使用**同一净化结果**，`../x` → `x` 后比对）。QA 实测：冲突 → `svc-err 名称「保留名.bin」已存在，请换一个`；空名 → `名称不能为空`；不存在 id → `条目不存在或已删除`；`../保留名.bin` → **同样返回已存在错误**（评审修正生效）；含 `/` 的名称按净化语义处理（净化后合法即接受）。**已知边界**：host 执行期失败（如占用改名失败）仍只写日志，完整回执随 S5/#114。

> 过程说明：QA 脚本首版直接设 `input.value` 未触发 React 受控更新，导致「改名无效」的假结果；改为原生 setter + `input` 事件后复测通过（脚本缺陷已修正并记录，避免误判产品）。

### 3.7 EX-06 同步告警（IPC 契约，为 #125 banner 提供数据面证据）

基线 `main @ fa1c95e`。方法：QA 探针以真实 IPC 客户端订阅 `SvcNotify`（`listen` 模式，不经 GUI），分别触发两类告警并抓取通知原文。证据 `evidence/M2/Warning-*`。

| 告警类型 | 触发方式 | IPC 通知实测（原文） | 判定 |
| --- | --- | --- | --- |
| `RetryExhausted` | 用排他句柄锁住教师端目标文件 → 同名覆盖 → 退避 5 次耗尽 | `NOTIFY warning kind=RetryExhausted item_id=c3c82d58-… detail=目标文件被占用，重试 5 次仍未成功，条目已置失败` | ✅ 契约正确（含 item_id 与中文 detail） |
| `BackupFailed` | 锁定目标 → 覆盖时 `.bak` 备份失败（不回滚新版本） | `NOTIFY warning kind=BackupFailed item_id=1769cfc8-… detail=旧版本备份失败（目标被占用）` | ✅ 契约正确 |

**说明**：#125（EX-06 banner GUI）的界面聚合展示证据仍由 dev-gui E2E 提供、QA 按四要素复核；本节为**数据面/IPC 侧独立取证**（可交叉验证 banner 展示内容非硬编码）。同时观察到：备份失败时同一条目会先后产生 `BackupFailed` 与后续 `RetryExhausted`（两条告警），#125 的聚合去重逻辑应能合并/分列展示，复核时需核对。

**#125（EX-06 banner GUI）四要素复核 —— 通过**

证据：`apps/classroom-gui/docs/t20b/evidence.md` + 2 张截图（main，PR #125 已合并）；QA 读图 + 代码核对 + 与 §3.7 的 IPC 契约交叉。

| 要素 | 核对结果 |
| --- | --- |
| 命令可复现 | evidence.md 给出完整手法（学员 v1 入库 → 独占句柄锁教师本地副本 → 重传 v2 触发 #103 重试队列）、环境（release GUI + CDP 19231、隔离 data root）；QA 侧有等价脚本（`qa-tools/tc07-*.ps1`）可复现同一触发 | ✅ |
| 截图与判据一致 | `01` 黄 banner「1 条目标文件正被程序占用，已加入重试队列…」+ 行内 chip「被占用重试」；`02` 红 banner「1 条连续重试失败 重试已用尽：…（EX-06）。」+「忽略」按钮 + 行内 chip「失败」 | ✅ 逐张读图确认 |
| DOM 断言真实（非硬编码） | 代码核对：`SyncIssuesBanner` 由 `items`（`sync_state=RetryLocked/Failed`、`retry_count`）与 `warnings`（真实 `SvcNotify::Warning`）派生；`store.ts` 按 `kind+item_id+detail` 去重、上限 50；`dismissWarnings` 仅清展示 | ✅ 且与 QA 的 IPC 契约（RetryExhausted/BackupFailed 通知原文）一致 |
| 环境说明 | 隔离 `LOCALAPPDATA`/`PROGRAMDATA`、真实 teacher/student svc、release GUI | ✅ |

聚合/去重（reviewer 口径 + qa 核对代码）：**BackupFailed 进逐条列表**；**RetryExhausted 不进逐条**，转为红色「需人工处理」汇总 + 「忽略」按钮；`otherWarnings` 单列。与 reviewer 结论一致。

**观察（轻微文案冗余，不阻塞）**：黄 banner 聚合行已含「已加入重试队列」，其 detail 行又以「已加入重试队列，稍后自动覆盖；不影响其它共享。」开头 → 同屏出现两次「已加入重试队列」。建议后续 polish（属于 #20b/#106 文案整理，不改逻辑）。

### 3.8 未完成 / 待补验项小结（明细见 §5）

| 项 | 当前状态 | 依据/跟踪 |
| --- | --- | --- |
| TC-07 接收端占用重试 | ✅ **已实现（#135）并通过**；`retry_count` 列表叠加轻微问题见 **E18** | 已验（§3.5.1） |
| 学员「我共享的」为空 | 预期（#104 学员上行不落本地库） | **#121** |
| 学员端「待删除 chip」 | 实现存在、DOM 可见窗口 1–2 帧不可稳定取证 | 已知限制；#106 后续 UX 小项（300–500ms） |

| 教师操作静默失败 | `RenameItem` 冲突等失败 IPC 仍回 `ok`（fire-and-forget） | **E17**（P2，待修） |
| 缓存阈值提示（>100MB） | 未测（可选判据） | TC-09 判据 7 |

## 4 缺陷与观察项（M2 期间）

| 编号 | 级别 | 内容 | 状态 |
| --- | --- | --- | --- |
| E13 | 一般 | GUI 启动自共享自身 exe（M1 期间发现） | ✅ 已闭环（PR #99 / v0.1.1，QA 双向复验） |
| **E14** | **P1（已闭环）** | UI 数据范围错误（双向）：`store.ts` 学员 `studentFilter("mine")` 返回 `owner:null`；教师分支无 owner 过滤 → 教师「我共享的」混入学生提交、学员「我共享的」混入教师广播，且提供编辑/删除入口（违反 PRD 3.4/5.5） | **#119 → PR #122 已合入**；QA 复核（读图 + 数据面）：教师我共享的=1 仅教师条目（`HAS_STUDENT_ITEM:false`）、学员我共享的=0、教师共享=1 ✅ |
| **E15** | **P1（已修复闭环）** | 离线补发在会话未就绪时被判成功 → 静默丢件（间歇→该窗口实测 5/5 丢）：先报「补发送成功」后 4s 才 `已加入房间`；断连瞬间提交被记「上行发送成功」未入队 | **#123 → PR #124 已合入**；QA 复验：丢件 **5/5 → 0/5**、`flush_before_join=0`、送达 5/5 ≤10s；TC-09 判据 6 通过 |
| **E16** | **P2（已修复闭环）** | 补发完成后 `pending_count` 不归零（队列已空、教师已收到，状态条仍显示「待发送 N 条」）；根因 `resend_pump` 在 `confirm_sent` 出队后未 `notify_pending` | **#128 → PR #131 已合入**；QA 复验：`pending 2→0` 用时 **877 ms** ✅ |
| **E17** | **P2（已修复闭环）** | 教师操作 fire-and-forget：`RenameItem` 冲突等失败 IPC 仍 `ok`（静默失败） | **#129 → PR #132 已合入**；QA 复验：冲突/空名/不存在/净化后冲突均返回明确 `svc-err` 且不投递命令 ✅（host 执行期失败仍仅日志，随 S5/#114） |
| **E18** | **P3（已修复 #141，待 QA 复验）** | 接收端占用重试期间 `retry_count` 列表层恒 0（日志 `attempts` 正常递增）；耗尽后因 #103 规则显示 5。根因：`with_retry_state` 只叠加 host `retry_queue.json`，未叠加接收端 `recv_retry.json` | **#141 已合入**（接收端占用重试期间 `retry_count` 回写）；QA 未及在本轮复验，**标注待补验**（功能/状态此前已正确） |


## 5 已知限制与延后项

| 项 | 原因 |
| --- | --- |
| 真实双机 UDP 发现 / 跨机断网（拔网线） | 单机环境；#91 |
| 管理员路径（服务模式/`%PROGRAMDATA%`/E7 双授予） | 非管理员；#91 |
| 28/30 台压测与 p95 | 已批准留 M3 |
| 静默断链（无 FIN/RST）离线场景 | 需心跳读超时（#16 会话活性治理） |
| TC-07 接收端占用重试 | 属 PRD 5.7 完整语义但须 agent 落盘路径队列化 + 告警 + GUI 接入 → **#117**（Lead 裁定 M2 缩范围） |
| 学员端「待删除」chip 可见窗口 | #17 在同一流程完成标记+补删+ack，DOM 窗口 1–2 帧；如需最小可观察窗口（300–500ms）为 GUI UX 增强（#106 后续，不改协议） |
| 学员「我共享的」空内容 | E14 修复后语义正确但列表为空（#104 学员上行不落本地库）→ **#121**（dev-backend 后续） |
| 教师操作失败不可见（E17） | 教师操作 fire-and-forget，失败仅服务端 WARN，GUI 收到 ok | **#129** |

## 6 `v0.2.0` 放行条件（对照方案 §5.3）

- [x] TC-04 / TC-05：后端 + UI 通过（§3.1/§3.1.1/§3.2/§3.2.2；#121 已关闭）
- [x] TC-08：后端（#105）+ 教师端 UI（#106/#111）通过（§3.3/§3.3.1）；学员端 chip 记已知限制
- [x] TC-09（#18）通过（§3.4；断电不丢 0/5、pending_count 归零）
- [x] 扩展项：rename（后端 + GUI）、内容搜索（后端 + GUI #133）、占用重试（教师侧 + 接收端）均通过
- [x] 本报告定稿（M2 单机部分验收；#114 已实现待合并，见下）
- [ ] 延后项（§5）在 release note 列明（含 **#91 双机/管理员**、M3 设备注册表与 p95）

## 6.1 `#114`（S5 ack 最小闭环）状态

- **已合入**：PR **#137**（`main` 含 #137/#139）；QA 在 #137 分支（`21120db`，8 个真实 agent、2 台离线）**预验证通过**：
  - **在线删除 Ack**：在线期 `delete-ack` = **6/6**（6 台在线各一条）——原 O-C 缺口关闭；
  - **进度通知**：`DeleteProgress{done:1→6, total:6}` 连续上报；
  - **状态收敛**：删除后 `deleted=true / sync=Deleted`（不再是 `DeleteDeferred`）；
  - **离线补删**：2 台重连后 `delete-ack` 累计 = **8**（离线端补删亦 Ack）。
- **口径说明**：S5 最小闭环为**在线端聚合**，进度分母 `total` = **删除时在线台数**（本场景 6），离线端重连补删后不再上报 7/8、8/8；「全设备 6/8→8/8」需**设备注册表**（M3）——此边界已由 #137 记录。
- 该结论已并入 §3.3（TC-08 后端）与结论总表；M2 定稿后如 M3 实现设备注册表，再补「全设备进度」判据。

## 7 证据索引

目录：`docs/acceptance/evidence/M2/`（UTF-8 无 BOM、路径脱敏 `<repo>`）；M1 证据见 `evidence/M1/`。

| 分组 | 证据文件 |
| --- | --- |
| TC-04 后端 | `TC04-backend.txt`、`TC04-neg.txt`、`TC04-svcpath.txt`、`TC04-gate.txt`、`TC04-gui.txt`、`TC04-submissions-ui.png` |
| TC-05 后端（分组数据面） | `TC05-backend.txt` |
| TC-08 后端 | `TC08.txt`、`TC08-timing.txt`、`TC08-timing2.txt`、`TC08-idem.txt`、`TC08-anchors.txt`、`TC08-ack.txt`、`tc08-teacher-svc.log.txt`、`tc08b-teacher-svc.log.txt` |
| TC-08 UI（dev-gui 证据 + QA 复核记录） | `TC08-chip-result.json`、`TC08-chip.png` |
| TC-09 / E15 | `TC09-result.json`、`TC09-result-after-E15fix.json`、`E15-fixed-rate-summary.json`、`E15-trace1-stu2.log.txt`、`E15-trace1-tea3.log.txt`、`E15-trace2-stu1.log.txt`、`E15-trace2-tea2.log.txt`、`E15-trace3-recovered-stu2.log.txt` |
| TC-07 / EX-06 | `TC07-teacher-retry-result.json`、`TC07-exhaust-result.json`、`Warning-RetryExhausted-notify.txt`、`Warning-RetryExhausted-svc.log.txt`、`Warning-BackupFailed-notify.txt`、`Warning-BackupFailed-svc.log.txt` |
| #19 重命名 / 搜索 | `Rename-result.json`、`Rename-teacher.log.txt`、`Rename-student.log.txt`；GUI 复核 `tc127/`（5 截图 + result.json） |
| E13（M1 遗留缺陷闭环） | `../M1/e13/E13-result.txt`、`../M1/e13/E13-verify.txt` |
| E14 / E16 / #121 | `E14-teacher-scope-result.json`、`E16-stale-pending-stu.log.txt`、`E16-fix-result-main.json`、`E16-prefix-result.json`、`TC121-student-mine-result.json`、`E121-student-local-items-main.txt` |
| 环境 / 取证能力 | `cap8.txt`、`test-count-7c26b37.txt`、`gui-blocker.txt`、`gui-blocker-desktop.png` |

> 说明：`TC08`/`Rename-`/`Warning-` 等前缀在正文中按组引用；上表列出精确文件名以便复核。

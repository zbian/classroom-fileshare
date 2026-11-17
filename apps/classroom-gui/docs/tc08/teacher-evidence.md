# TC-08 教师端 UI 证据（真实链路，供 qa 复核）

> 环境：隔离 `LOCALAPPDATA`/`PROGRAMDATA`；`teacher svc` + `student svc`（昵称 alice，`manual_addr=127.0.0.1:47821`，**保持在线**）；
> GUI 为 **release 构建**（`cargo build --release -p classroom-gui --features custom-protocol`，内嵌前端，恒走 `RealGuiClient`），
> CDP `--remote-debugging-port=19229`。data root：`%TEMP%\e16e`。
> 代码版本：`feat/106-delete-gui` @ `3b7554d`（#111，本 PR 合并后 main 行为一致）。

## 1) 删除确认框（DOM 原文）

前置：以**教师身份**经真实 IPC 广播 `TC08教师广播样例.txt`（`SendFiles`），教师 GUI 列表出现该行；选中 → 点「删除」。

`document.querySelector('.modal .info.err').innerText` 原文：

```
删除「TC08教师广播样例.txt」后将从所有学员端同步移除本地副本，此操作不可撤销。
```

说明：PRD 8.3 基准句为「删除后将从所有学员端同步移除本地副本，此操作不可撤销」；
实现按 UI 惯例插入了被删对象标题与外层「删除「」」包裹（句式与基准一致）。
第二行（`.modal .info`）：「在线学员端将立即删除；离线学员端将在其上线后自动删除，删除完成后列表同步刷新。」
截图：`01-teacher-delete-confirm-real.png`

## 2) 删除后 toast —— 台数取真实 `online_count`

环境中有 **1 台在线学员（alice）**，`StatusSnapshot.online_count=1`。

确认删除后 toast 原文（真实 DOM）：

```
已删除：在线 1 台学员端已同步移除；离线端上线后自动删除
```

- 1 台在线的变体排除了「硬编码 0」的可能；0 台在线时同一模板显示「在线 0 台…」（此前 e16d 环境实测）。
- 教师 svc 日志同步可见 `broadcast …` 与删除请求处理。
  截图：`02-teacher-delete-toast-real.png`

## 3) 「学生提交」分组树（只读）

用 alice 的真实上行提交（student pipe `SendText` ×3，间隔发送；教师 svc 日志逐条 `学员提交入库 owner=alice …`）。

| 断言 | DOM 实测 |
| --- | --- |
| 左树 | `alice · 3`（计数 = 真实条目数） |
| 右表行 | `['alice 提交v2', 'alice 提交v1', 'alice 提交2']`（3 行，latest 倒序） |
| 工具栏按钮 | `['刷新']` |
| 是否存在写操作按钮（删除/改名/重命名/编辑） | **False** |
| 行内 ops（`.grow .ops span`） | `[]`（只读，无行内操作） |

截图：`03-teacher-submissions-readonly-real.png`

## 4) 已知事项 / 分工

- **时间显示**：本分支左树最新时间会显示 `01-22 02:35`（把 `latest_at` 的 Unix **秒**当毫秒渲染）——该 bug 已在 **#116（#16B）** 修复（`fmtGroupTime = fmtLatest(sec*1000)`）；合并后显示为 `HH:MM`。
- **待删除 chip 的可见窗口**（学员端）：后端 #17 在收到墓碑的同一流程内完成「标记 DeleteDeferred → 补删 → ack」（qa 实测 ack 2088–2093ms），因此 chip 在 DOM 上的可见窗口可能只有 1–2 帧。**由 qa 自行取证/裁定**；若需要最小可观察窗口（如 300–500ms），建议作为 #106 后续 UX 项单独评估（不改协议）。
- 本文件所有结论来自**真实链路**（真实 svc + 真实 IPC + release GUI），非 mock。

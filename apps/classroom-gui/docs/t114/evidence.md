# #114 删除同步进度 GUI 消费（WIP → 已实现）

契约（#137 / head `21120db`，待合入）：`SvcNotify::DeleteProgress { item_id, done, total }`
→ notify retag 统一外壳 `{t:"DeleteProgress", payload:{item_id,done,total}}`（**读 `payload`**）。
语义：删除时在线端数 = total；每收一个 ack 推一次；`done` 已由 svc `min(total)`；全部 ack → host 置终态 `Deleted`；`total=0` 不显示进度。

## 变更（仅 `apps/classroom-gui/ui/src/**`；`crates/**` 零改动）
| 位置 | 内容 |
| --- | --- |
| `shared/ipc/types.ts` | 新增 `SvcNotify 'DeleteProgress'`（payload 形态）；`SyncState` 补 **`'Deleted'`**（#114 终态） |
| `main/store.ts` | `deleteProgress` 状态 + `case 'DeleteProgress'`：`done=min(done,total)`；`total<=0` 不显示；完成态保留 **2.5s** 后自动隐藏（`progressTimer`） |
| `main/components/DeleteProgressBar.tsx` | 进度条：进行中「正在同步删除：done/total 台」；完成「已同步删除：total/total 台」 |
| `main/App.tsx` | Toolbar 下渲染进度条（两个 Tab 通用） |
| `views/MySharesTab` / `TeacherSharesTab` | 状态映射补 `Deleted → 「已删除」`（reviewer 建议项） |
| `shared/ipc/client.ts`（mock） | 教师删除后模拟 0/3→2/3→3/3 进度事件（dev 保真；真实事件由 #137 后端推送） |

## 证据（dev mock 保真，`docs/t114/`）
CDP 断言（教师删除 mock 条目后轮询 `[role=status]`）：
```
SEQ: ['正在同步删除：0/3 台', '正在同步删除：2/3 台', '已同步删除：3/3 台']
完成态保留 2.8s 后 → None（自动隐藏）
```
截图：`01-progress-running.png`（2/3 进行中）、`02-progress-completed.png`（3/3 完成）、`03-progress-hidden.png`（隐藏后）。

## 说明（重要）
- **真实链路证据待 #137 合入**：当前 main 尚无 `SvcNotify::DeleteProgress`，无法做「教师删除 → 学员 ack → 进度」真机验证；
  本 PR 已按契约定稿实现，mock 证据仅证明 UI 消费/收敛/完成隐藏逻辑；#137 合入后我补一轮真实链路（教师 svc + 多学员 svc ack）并贴 CDP 断言。
- 门禁：`pnpm build` 三入口 PASS（`tsc --noEmit` + vite build）；Rust 侧零改动，local-ci 的 Rust 门禁不受影响（因磁盘事故后冷 target 时间原因未全量跑，CI 会跑）。

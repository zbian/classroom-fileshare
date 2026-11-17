# EX-06 同步异常 banner 接入真实数据（#20b / #103）

## 变更
- **`SvcNotify::Warning { kind, item_id, detail }` 订阅与展示**（#103 契约）
  - `WarningKind`：`RetryExhausted`（占用重试耗尽）/ `BackupFailed`（备份写入失败）/ `DiskFull`
  - store 新增 `warnings`（按 kind+item_id+detail 去重，最多保留 50 条，可「忽略」清除展示）
  - `SyncIssuesBanner` 逐条展示真实 `detail`，并保留 PRD 8.3 关键词
- **`ShareItem.retry_count` 展示**：占用重试行内 chip 显示「被占用重试（第 n/5 次）」
- 前端 Warning 通知形态修正为 `{"t":"Warning","payload":{kind,item_id,detail}}`
  （notify retag 统一 payload 外壳）；补 Rust 单测 `warning_notify_retagged_for_frontend`
- mock 客户端 `retry_count` 字段补齐（dev 视图与真实契约一致）

## 真实链路证据（非 mock / 非静态注入）
环境：隔离 `LOCALAPPDATA`/`PROGRAMDATA`；teacher svc + student svc（alice）+ **release GUI（CDP 19231）**。
手法：学员上行文件 v1 入库教师侧 → **用独占句柄锁住教师侧本地副本**（`CreateFileW share=0`）
→ 学员重传同名 v2，触发教师 host 的占用退避重试队列（#103）。

| 环节 | 实测 |
| --- | --- |
| 首次重试 | 行内 chip = **`被占用重试`**；banner = 「1 条目标文件正被程序占用，已加入重试队列　已加入重试队列，稍后自动覆盖；不影响其它共享。」（截图 `01-retry-count-chip.png`） |
| 教师 svc 日志 | `占用重试退避 attempts=2/3/4/5`（退避 1→2→4→8→16s）、`重试时备份失败（不回滚）…os error 32`、`占用重试耗尽，条目转 Failed attempts=5` |
| 耗尽后 | 行内 chip = **`失败`**；banner = 「1 条连续重试失败　重试已用尽：请关闭占用该文件的程序后重新发送，或联系管理员处理（EX-06）。」+ **「忽略」按钮**（仅收到真实 `Warning` 事件时渲染） |
| 截图 | `02-retry-exhausted-banner.png` |

## 已知限制（#103 上游）
- 「孤儿 RetryLocked」（重试队列进程重启后条目状态残留）属 #103 已知限制，GUI 不做特殊处理。
- 单条 Warning 的 `detail` 由服务端文本直出；本环境 banner 首行是**聚合语义**，逐条 detail 在下方
  `· <kind 文案>：<detail>` 行展示（个别通知到达时序晚于首次 banner 渲染，属正常刷新行为）。

## 门禁
worktree 内 `.\scripts\local-ci.ps1` 六项 ALL PASS；`cargo test --workspace` 见 PR 描述。
`crates/**` 零改动（只消费 #103 已合入契约）。

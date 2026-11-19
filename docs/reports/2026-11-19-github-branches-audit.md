# GitHub 远端分支合并状态审计（2026-11-19）

> 对象：GitHub 镜像（zbian/classroom-fileshare）55 个 heads（含 main）→ 审计 54 个非 main 分支
> 判定：① GitHub merged PR（headRefName 命中）② `git merge-base --is-ancestor` ③ `git cherry` 补丁等价（无未吸收 `+` 提交）

## 汇总

| 状态 | 数量 |
| --- | --- |
| 已合并到 main | **53** |
| — 未合并（PR 已关闭） | 1 |
| — 已合并（PR） | 52 |
| — 已合并（内容等价） | 1 |
| **未合并** | **1** |
| 开放 PR | 0 |

## 未合并分支

| 分支 | PR | ahead | 未吸收提交 | 最后提交 |
| --- | --- | --- | --- | --- |
| `docs/m1-acceptance-plan` | #35 | 14 | 14 | 2026-11-16 |

## 已合并分支清单

| 分支 | PR | ahead | 未吸收 | 最后提交 |
| --- | --- | --- | --- | --- |
| `docs/dual-machine-plan` | #145 | 1 | 0 | 2026-11-18 |
| `docs/e14-131-regression` | #134 | 1 | 0 | 2026-11-18 |
| `docs/m2-plan` | #107 | 2 | 2 | 2026-11-17 |
| `docs/night-report` | #140 | 2 | 2 | 2026-11-18 |
| `docs/night-report-fix` | #143 | 1 | 0 | 2026-11-18 |
| `docs/reviews-batch-m2` | #109 | 1 | 0 | 2026-11-17 |
| `docs/reviews-batch-m2-2` | #113 | 1 | 0 | 2026-11-18 |
| `docs/reviews-pr85` | #95 | 1 | 0 | 2026-11-17 |
| `docs/syncthing-setup` | #146 | 1 | 0 | 2026-11-18 |
| `docs/tc08-evidence` | #118 | 1 | 0 | 2026-11-18 |
| `feat/103-retry-queue` | #115 | 2 | 2 | 2026-11-18 |
| `feat/104-student-uplink` | #110 | 2 | 2 | 2026-11-17 |
| `feat/106-delete-gui` | #111 | 2 | 0 | 2026-11-18 |
| `feat/114-ack-aggregation` | #137 | 3 | 3 | 2026-11-18 |
| `feat/114-delete-progress-ui` | #139 | 2 | 1 | 2026-11-18 |
| `feat/117-recv-retry-queue` | #135 | 2 | 2 | 2026-11-18 |
| `feat/121-student-local-store` | #131 | 3 | 3 | 2026-11-18 |
| `feat/127-content-search` | #133 | 1 | 0 | 2026-11-18 |
| `feat/127-rename-gui` | #130 | 1 | 0 | 2026-11-18 |
| `feat/129-rename-receipt` | #132 | 2 | 2 | 2026-11-18 |
| `feat/136-recv-retry-count` | #141 | 1 | 0 | 2026-11-18 |
| `feat/16b-student-send-ui` | #116 | 2 | 2 | 2026-11-18 |
| `feat/16-student-submissions-ui` | #108 | 1 | 0 | 2026-11-17 |
| `feat/17-delete-sync` | #105 | 1 | 0 | 2026-11-17 |
| `feat/18-offline-queue` | #120 | 2 | 2 | 2026-11-18 |
| `feat/19-rename` | #126 | 1 | 0 | 2026-11-18 |
| `feat/20b-warning-banner` | #125 | 2 | 2 | 2026-11-18 |
| `feat/20-drop-retry` | #101 | 1 | 0 | 2026-11-17 |
| `feat/89-m2-s3-session-identity` | #92 | 1 | 0 | 2026-11-17 |
| `feat/90-list-search-filter` | #96 | 1 | 0 | 2026-11-17 |
| `feat/94-student-upload-backend` | #98 | 2 | 2 | 2026-11-17 |
| `feat/dual-artifacts-sync` | #148 | 1 | 0 | 2026-11-18 |
| `feat/dual-remote-framework` | #147 | 1 | 0 | 2026-11-18 |
| `feat/dual-test-run` | #149 | 1 | 0 | 2026-11-18 |
| `feat/student-installer` | #156 | 1 | 0 | 2026-11-19 |
| `feat/t6-agent` | #34 | 6 | 5 | 2026-11-16 |
| `fix/119-e14-owner-filter` | #122 | 2 | 2 | 2026-11-18 |
| `fix/17-observations` | #112 | 2 | 2 | 2026-11-18 |
| `fix/93-e13-argv0` | #99 | 2 | 2 | 2026-11-17 |
| `fix/e15-pending-dequeue` | #124 | 2 | 2 | 2026-11-18 |
| `fix/ipc-console-acl-degrade` | #150 | 1 | 0 | 2026-11-18 |
| `qa/acceptance-e12` | #97 | 2 | 2 | 2026-11-17 |
| `qa/e13-close` | #102 | 2 | 2 | 2026-11-17 |
| `qa/local-ci` | #85 | 1 | 0 | 2026-11-17 |
| `qa/m2-114-update` | #142 | 1 | 0 | 2026-11-18 |
| `qa/m2-acceptance-plan` | #100 | 1 | 0 | 2026-11-17 |
| `qa/m2-tc0405-record` | #138 | 22 | 22 | 2026-11-18 |
| `release/v0.1.1` | — | 1 | 0 | 2026-11-17 |
| `test/dual-a2` | #151 | 1 | 0 | 2026-11-18 |
| `test/dual-a3` | #152 | 1 | 0 | 2026-11-18 |
| `test/dual-b1` | #153 | 1 | 0 | 2026-11-18 |
| `test/dual-b2c4` | #154 | 1 | 0 | 2026-11-18 |
| `test/dual-c5` | #155 | 1 | 0 | 2026-11-18 |

## 说明

- `ahead` 为分支相对 `github/main` 的提交数；squash-merge 分支 ahead>0 属正常（提交被压成 1 个新提交进 main）。
- `未吸收提交` 用 `git cherry` 统计（`+` 行）：>0 表示补丁未进入 main；已合并分支出现 >0 是 squash 导致，不代表丢失。
- `release/v0.1.1` 无 PR，但补丁已全部在 main（内容等价）；`v0.1.1` 标签仍保留。
- `docs/m1-acceptance-plan`（#35 CLOSED）为唯一未合并分支：14 个提交未进入 main，内容已被后续 `M1-acceptance-plan v1.3.9` 取代。

## 清理执行（2026-11-19）

按审计结论删除 GitHub 远端 53 个已合并分支（`git push github --delete`，两批执行），保留：

- `main`
- `docs/m1-acceptance-plan`（唯一未合并分支）

执行后校验：GitHub heads = **2**；Gitee heads = **1**（main，从未推其它分支）。
本地 remote-tracking 已 prune；删除分支的完整 refs 备份仍保留在
`%LOCALAPPDATA%\ClassShareDual\backup\classroom-fileshare-allrefs-20261119-081600.bundle`。
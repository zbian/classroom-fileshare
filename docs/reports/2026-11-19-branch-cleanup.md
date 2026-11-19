# 本地历史分支清理（2026-11-19）

## 判定依据

1. **GitHub 已合并 PR**：`gh pr list --state merged` → 109 个合并 PR，105 个唯一 `headRefName`；
   本地分支名命中即视为「已合并」。
2. **祖先合并**：`git merge-base --is-ancestor <branch> github/main` → 3 个分支 tip 直接在 main 历史里。
3. **内容等价**（`git cherry`）：未命中 PR 但补丁已被 main 吸收（`-` 行、无 `+` 行）→ 视为已合并
   （仅 `qa-e6-verify`、`release/v0.1.1` 两个）。

## 结果

| 项 | 数量 |
| --- | --- |
| 清理前本地分支（含 main） | 111 |
| 删除 | **105**（103 依据 PR/祖先 + 2 依据内容等价） |
| 清理后本地分支 | **6**（main + 5 个未合并历史分支） |

保留的 5 个（未合并、且不在 GitHub 远端；M1 E7 历史迭代，最终实现已被 #150 取代）：

- `qa-e7-v2`（ahead 2）
- `qa-e7-v3`（ahead 6）
- `qa-e7-verify`（ahead 1）
- `qa/acceptance-v131`（ahead 3）
- `qa/verify-t6`（ahead 5）

## 备份与恢复

- 清理前已创建全 refs bundle（12 MB）：
  `%LOCALAPPDATA%\ClassShareDual\backup\classroom-fileshare-allrefs-20261119-081600.bundle`
- 恢复单个分支：`git fetch <bundle> 'refs/heads/<branch>:refs/heads/<branch>'`
- 清理掉的 103 个中，55 个仍存在于 GitHub 远端（`git fetch github` 可取回）

## 附注

- 删除是本地操作，未改动 Gitee/GitHub 远端分支
- 已确认删除分支均为 squash-merge 或祖先合并，内容已在 main 中，不影响构建
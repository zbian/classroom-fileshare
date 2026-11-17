# 开发协作流程（GitHub Flow）

本文件是团队的强制工作规范。所有成员（含 Lead）开发类变更一律走 issue → 分支 → PR → Review → Merge 流程。

## 角色

| 成员 | 职责 | 写入范围（advisory scope） |
| --- | --- | --- |
| `lead` | 计划、建 issue、派工、合并 PR、打 tag、协调 | 全仓库（仅协调性写入） |
| `dev-backend` | Rust 核心开发：`crates/*`、`apps/classroom-svc`、CI 配置 | `crates/`、`apps/classroom-svc/`、`.github/` |
| `dev-gui` | Tauri GUI 与 React 前端：`apps/classroom-gui/` | `apps/classroom-gui/` |
| `code-reviewer` | 审查所有 PR，对照 PRD 与安全清单，产出审查记录 | 只读代码；写 `docs/reviews/` |
| `qa-acceptance` | 按 TC 用例验收，产出验收报告，驱动缺陷闭环 | 只读代码；写 `docs/acceptance/` |

## 环境备忘

- 仓库：`git@github.com:zbian/classroom-fileshare.git`（SSH 推拉已验证）
- Rust：用户级安装，命令在 `$env:USERPROFILE\.cargo\bin\`；新进程若找不到 cargo，先执行
  `$env:Path += ";$env:USERPROFILE\.cargo\bin"`
- rustfmt / clippy 组件缺失（报 `'cargo-fmt.exe' is not installed` 之类）时执行
  `rustup component add rustfmt clippy`（stable 工具链默认不含，本地首次 fmt/clippy 前需安装）
- 前端：Node 24 + pnpm 11；WebView2 Runtime 已就绪
- git 身份：仓库级 `.git/config` 已配置（zbian / 685101+zbian@users.noreply.github.com），无需全局配置
- gh CLI：可用；若遇到 `Resource not accessible by personal access token` 说明令牌权限未就绪，报告 Lead

## 工作树隔离（并行开发必需）

主检出目录只有 Lead 用于协调，**成员一律在独立 worktree 中工作**：

```powershell
git fetch origin
git worktree add .wt/<branch-name> -b feat/<issue号>-<短名> origin/main
cd .wt/<branch-name>
# ... 开发、提交 ...
git push -u origin feat/<issue号>-<短名>
# 完成后清理：git worktree remove .wt/<branch-name>
```

`.wt/` 已加入 `.gitignore`。

## 分支与提交规范

- 分支命名：`feat/<issue号>-<短名>`、`fix/<issue号>-<短名>`、`docs/<短名>`、`chore/<短名>`
- **禁止直接推送 `main`**（唯一例外：项目启动初始化提交，已发生完毕）
- 提交信息：Conventional Commits（`feat:` `fix:` `docs:` `test:` `chore:` `refactor:`），正文含 `Refs #<issue>`（完成时 `Closes #<issue>`）
- 提交前：**本机 CI 门禁** `.\scripts\local-ci.ps1` 必须全绿（见下节「本机 CI（门禁）」）

## Issue 规范

- **每个任务一个 GitHub issue**，由 Lead 按实施计划（`docs/plans/`）创建
- Issue 模板字段：背景与目标 / PRD 引用（章节+FR 编号）/ 验收标准（可勾选）/ 依赖 issue
- 标签体系：`P0` `P1` `P2`；`area/protocol` `area/net` `area/store` `area/host` `area/agent` `area/ipc` `area/gui` `area/service` `area/docs`；`type/feature` `type/bug` `type/chore` `type/test`；`role/dev` `role/review` `role/qa`
- 里程碑：`M1-P0`（能上一节课的最小闭环）→ `M2-P1`（回传/删除同步/离线）→ `M3-release`

## PR 流程

1. 开发者推送分支后创建 PR：标题 `[#<issue>] <描述>`，正文含变更说明、自测清单、GUI 变更附截图、`Closes #<issue>`
2. `code-reviewer` 审查：approve / request changes；审查记录写入 `docs/reviews/PR-<编号>.md`
3. **本机 CI 门禁**：`.\scripts\local-ci.ps1` 必须 **ALL PASS**，并把汇总输出（每步 PASS/FAIL + 总耗时）贴进 PR 描述或评论
4. Lead 执行 **squash merge**，保留 `Closes #<issue>` 于合并提交信息
5. 涉及用户可见行为的 PR 合并后由 `qa-acceptance` 按 issue 验收标准复核，通过后在 issue 留言关闭验收

## 本机 CI（门禁，2026-11-17 用户指令：停用 GitHub Actions）

GitHub Actions 已停用（`.github/workflows/ci.yml` 移除，git 历史可追溯），改由
**`scripts/local-ci.ps1`** 作为门禁权威实现（与 qa 的 `docs/acceptance/local-ci.md` 同口径）。
该脚本串联 6 道门禁，任一步失败即非 0 退出：

| # | 门禁 | 说明 |
| --- | --- | --- |
| 1 | PS BOM 守卫 | 全仓受版本控制 `.ps1` 必须 UTF-8 BOM；`.json` 数据不得带 BOM（双面规则；PS 5.1 对无 BOM 脚本按 ANSI/GBK 误读中文，已三次踩坑） |
| 2 | `cargo fmt --all --check` | 格式 |
| 3 | `cargo clippy --workspace --all-targets -- -D warnings` | debug 静态检查 |
| 4 | `cargo test --workspace` | 全量测试 |
| 5 | `pnpm install --frozen-lockfile` + `pnpm build` | 前端三入口（icon/main/tips） |
| 6 | `cargo clippy --release -p classroom-gui --features custom-protocol -- -D warnings` | GUI release profile 守卫（debug clippy 看不到 `cfg(not(debug_assertions))` 分支，见 #77/#79） |

常用参数：`-Quick`（BOM+fmt，秒级）、`-SkipWeb`（后端全量）、`-SkipTest`（只 lint）、
`-SkipReleaseClippy`。门禁依赖真实 `ui/dist`；缺失时脚本创建临时占位并在结束时清理。

> 例外机制（M2 评估，记 #16）：门禁 1 对 `.json` 采用「一律不得带 BOM」的硬规则；
> 若将来确有需带 BOM 的 JSON 数据（如外部工具产出），再加路径白名单并在脚本注释与
> 本节同步说明——当前仓库 3 个受版本控制 JSON 均无 BOM。

### Review 必查清单（code-reviewer）

- [ ] 与 PRD 对应 FR 的交互/异常/验收标准逐条符合
- [ ] 同名覆盖语义：作用域键 = 发送者 + 接收范围 + 文件名；`.part → SHA-256 → 原子替换 → 1 份 .bak → version+1`
- [ ] 路径安全：文件名净化、拒绝 `..` 与绝对路径、只写应用数据目录
- [ ] 幂等：event_id 去重、version 判旧、重复事件不产生第二条记录
- [ ] 大小与类型限制：单文件 ≤100MB、文本 ≤10000 字、风险类型（exe/bat/lnk…）拦截
- [ ] 日志用 tracing，不含文件正文与学生隐私
- [ ] 测试：核心逻辑有单测/集成测试且真实覆盖（非形式化）
- [ ] GUI：状态文字+色块（不单靠颜色）、只读区无增删改按钮、文案与 PRD 8.3 一致

## Tag 策略（仅 Lead 打 tag）

| 节点 | Tag | 触发条件 |
| --- | --- | --- |
| PRD/设计基线 | `docs-v1.0` | ✅ 已打（2026-11-13） |
| P0 最小闭环 | `v0.1.0` | M1 全部 issue 关闭，TC-01/02/03/06/11/12 验收通过 |
| P1 完成 | `v0.2.0` | M2 全部关闭，TC-04/05/08/09 通过 |
| 正式发布 | `v1.0.0` | M3 验收 + 全部 13 个 TC 通过 |

Tag 一律 annotated tag 并推送 origin。

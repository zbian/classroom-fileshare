# 本机 CI 口径与执行结果（验收侧证据）

| 项目 | 内容 |
| --- | --- |
| 用途 | GitHub Actions 停用后，替代「GHA 绿」作为 PR 门禁与验收前置证据的**本机 CI** 口径与结果记录 |
| 权威实现 | [`scripts/local-ci.ps1`](../../../scripts/local-ci.ps1)（dev-backend，#87）；本文件为**验收侧独立复核**口径 |
| 执行人 | qa-acceptance |
| 执行日期 | 2026-11-17 |
| **实测基线** | **`main @ e822696`**（干净 detached worktree `.wt/ci-fresh`；执行时 HEAD。此后 main 前进至 `c6792b0`，仅为前端 #96 搜索/筛选/虚拟滚动，未改变 Rust/PS 门禁面；下次 rc 复验将在此基线上重跑） |
| 环境 | Windows 11 26100、PowerShell 5.1、cargo 1.100.0 (MSVC)、Node 24 / pnpm 11.21.0 |
| 结果 | **6/6 门禁 ALL PASS**（总耗时 **129.8 s**）；`cargo test --workspace` = **146 passed / 0 failed** |
| 证据 | [`evidence/M1/local-ci/`](evidence/M1/local-ci/)：`local-ci-output.txt`（脚本原始输出）、`test-by-crate.txt`（按 crate 分布）、`test-total.txt` |

> 口径变更背景：用户指令「停用 GitHub Actions，改用本机 CI」。PR 门禁由「GHA 绿」改为「**本机 `scripts/local-ci.ps1` 全绿 + 输出贴 PR 描述**」；release note 不再写「CI 全绿」，改写「本机 CI（`scripts/local-ci.ps1`）全绿」。

## 1 六项门禁（按依赖顺序；缺一不可）

执行顺序**必须**为 1→6：门禁 6（GUI release clippy）依赖门禁 5 产出的 `ui/dist`（`--features custom-protocol` 会在编译期内嵌前端资源，dist 缺失则失败）。

| # | 门禁 | 命令 | 判定标准 |
| --- | --- | --- | --- |
| 1 | **PS BOM 守卫（双面规则）** | 见 §2 | 全仓**受版本控制**的 `*.ps1` **必须**有 UTF-8 BOM，且**受版本控制的 `*.json` 不得有 BOM**；违规数 **0** |
| 2 | fmt | `cargo fmt --all --check` | exit 0（无 diff） |
| 3 | clippy（debug） | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（任何 warning 视为失败） |
| 4 | test | `cargo test --workspace` | exit 0 且 `0 failed` |
| 5 | **前端构建（三入口）** | `pnpm --dir apps/classroom-gui/ui install --frozen-lockfile` + `pnpm --dir apps/classroom-gui/ui build` | exit 0；产出 `ui/dist/{icon,main,tips}.html` 与 assets |
| 6 | **GUI release clippy** | `cargo clippy --release -p classroom-gui --features custom-protocol -- -D warnings` | exit 0（**debug 盲区守卫**：`#[cfg(not(debug_assertions))]` 分支与 `push_tip` 类未用告警只在 release+feature 组合暴露，见 #79） |

## 2 门禁 1：PS BOM / JSON 双面规则

```powershell
$root  = (Get-Location).Path
$ps1   = @(git ls-files -- '*.ps1')   # 本轮跟踪 7 个
$json  = @(git ls-files -- '*.json')  # 仓库内受版本控制 JSON
$badPs = @($ps1 | Where-Object {
  $p = Join-Path $root $_; if (-not (Test-Path -LiteralPath $p)) { return $true }
  $b = [IO.File]::ReadAllBytes($p)
  -not ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
})
$badJs = @($json | Where-Object {
  $p = Join-Path $root $_; if (-not (Test-Path -LiteralPath $p)) { return $true }
  $b = [IO.File]::ReadAllBytes($p)
  ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
})
if ($badPs.Count + $badJs.Count -gt 0) { $badPs; $badJs; exit 1 }
```

- **PS 脚本必须有 BOM**：Windows PowerShell 5.1 对无 BOM 文件按 ANSI/GBK 解析，中文字符串会乱码甚至语法崩溃（`install.ps1` / `uninstall.ps1` / `register-context-menu.ps1` 三次踩坑）。
- **JSON 不得有 BOM**：`serde_json` 不接受前导 BOM（`gui.json` 曾因 BOM 静默丢失图标位置；`svc.json` 同源风险）。
- 取文件清单用 `git ls-files`：天然排除 `.wt/`（worktree）、`node_modules`、`target` 等未跟踪产物；**不要**改用全路径 `Get-ChildItem` 扫描（在 worktree 内会误杀整棵树）。

## 3 本次执行结果（`main @ e822696`）

| # | 门禁 | 结果 | 耗时 |
| --- | --- | --- | --- |
| 1 | PS BOM / JSON 守卫 | **PASS** | 0.2 s |
| 2 | `cargo fmt --all --check` | **PASS** | 0.5 s |
| 3 | `cargo clippy --workspace --all-targets -D warnings` | **PASS** | 36.6 s |
| 4 | `cargo test --workspace` | **PASS**（**146 passed / 0 failed**） | 51.0 s |
| 5 | `pnpm install --frozen-lockfile` + `pnpm build`（icon/main/tips） | **PASS** | 3.5 s |
| 6 | GUI release clippy（`--release --features custom-protocol`） | **PASS** | 38.0 s |
| — | **合计** | **ALL PASS** | **129.8 s** |

**测试分布（146）**：见 [`test-by-crate.txt`](evidence/M1/local-ci/test-by-crate.txt)（如 store 17、ipc 12、protocol 25+、gui 12、svc 10、host 9、agent 8、net 14/6 等；以文件内逐 crate 记录为准）。

## 4 PR 门禁用法（替代 GHA）

1. 在**干净 worktree**（无本地探针/未提交改动）执行 `.\scripts\local-ci.ps1` 全量；
2. 将**六门禁 PASS 汇总**与 `146 passed / 0 failed` 粘贴到 PR 描述（或引用本目录证据）；
3. 任一失败即阻断合并；失败项按缺陷流程报 Lead 开 issue；
4. 本机 CI 只证明**代码门禁**，不替代 TC 用例验收——rc 仍需 QA 验收复验。

## 5 与 `scripts/local-ci.ps1` 的对齐

- 脚本为权威实现；本文件 §1 的命令与判定标准与脚本逐条一致。
- 脚本支持 `-Quick`（BOM+fmt）、`-SkipWeb`、`-SkipTest` 等开关用于迭代；**PR 前必须全量**。
- 若脚本增删门禁，以 Lead 公布的六项为准并同步修订本文件；QA 每次 rc 会**独立重跑**并留证，不直接引用开发者自跑结果。

## 6 注意

- **GUI 构建**：生产/验收构建一律 `.\scripts\build-gui.ps1`；门禁 6 的 `--features custom-protocol` 与 O-1 是同一根因的两面，不可省略。
- **在含 QA 探针的 worktree 跑测试会带上探针 crate**，例如 `cargo test --workspace` 数目偏高；门禁口径以**干净 worktree** 结果为准。
- 证据统一 **UTF-8 无 BOM**、路径已脱敏为 `<repo>`。

# M1（P0 最小闭环）代码冻结报告

| 项目 | 内容 |
| --- | --- |
| 里程碑 | M1-P0（能上一节课的最小闭环） |
| 冻结基线 | `e134b76`（含 E9 自动发现连接 #69） |
| 冻结日期 | 2026-11-17 |
| 状态 | **代码侧冻结完成**（验收进行中：qa 单机部分验收） |
| 依据 | PRD 第 12 章 P0 范围、[M1 实施计划](plans/2026-11-13-m1-p0.md) |

## 1 交付范围

### 1.1 workspace 成员（8）

`crates/{protocol,store,net,host,agent,ipc}` + `apps/classroom-svc` + `apps/classroom-gui/src-tauri`

### 1.2 任务与 PR 对照

| 任务 | issue | PR | 内容 |
| --- | --- | --- | --- |
| T-1a | #1 | #24 | Cargo workspace 骨架 + CI |
| T-1b | #2 | #27 | Tauri 2 + React 三入口脚手架 |
| T-2 | #3 | #28 | protocol：模型/事件/幂等合并（含尾随点绕过修复） |
| T-3 | #4 | #30 | store：SQLite 索引 + 原子覆盖文件仓库 |
| T-4 | #5 | #31 | net：UDP beacon + TCP 帧协议 |
| T-5 | #6 | #32 | host：房间/准入/昵称查重/广播 |
| T-6 | #7 | #34 | agent：发现/接收/重连对齐（含协议修订 #5①③④⑤） |
| T-7 | #8 | #38 | ipc：命名管道/令牌/双工 |
| T-8 | #9 | #29 | GUI 置顶浮动图标/右键菜单/退出 |
| T-9 | #10 | #53 | GUI 教师端主界面 + RealGuiClient 真实 ipc |
| T-10 | #11 | #59 | GUI 学员视图与首次昵称 |
| T-11 | #12 | #33 | GUI 15 秒 tips 气泡 |
| T-12 | #13 | #66 | GUI 桌面入口三件（右键粘贴/文件拖放/资源管理器右键） |
| T-13 | #14 | #44 | Windows 服务包装 + 安装/卸载脚本 |
| **E9** | #15 跟踪 | **#69** | **agent 自动发现并连接（PRD 2.4/3.1 P0 要求，冻结巡检补齐）** |
| 安全缺陷 | #49 | #50 | E7 令牌 ACL（icacls→windows-rs API 直写） |
| 存储加固 | — | #46/#63 | busy_timeout + WAL 并发收口 |
| 取证锚点 | #37/#49 | #39/#48 | E4 broadcast/received/applied、E6 size 语义 |

### 1.3 GUI 侧全部入库

T-8 置顶图标(#29) · T-11 tips(#33) · T-9 教师界面+RealGuiClient(#53) · T-10 学员视图(#59) · T-12 桌面入口三件(#66)

## 2 质量门（终态实跑）

| 门 | 命令 | 结果 |
| --- | --- | --- |
| fmt | `cargo fmt --all --check` | exit 0 |
| clippy（debug） | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（含 GUI src-tauri，无交叉破坏） |
| clippy（release） | `cargo clippy --release -p classroom-gui --features custom-protocol -- -D warnings` | exit 0（debug 看不到 `cfg(not(debug_assertions))` 分支；CI 独立作业 `gui-release-clippy` 守卫） |
| test | `cargo test --workspace` | exit 0：25 binaries / **127 passed / 0 failed / 0 panicked** |
| GUI 生产构建 | `scripts/build-gui.ps1`（pnpm build → **release clippy** → `cargo build --release --features custom-protocol`） | exit 0（O-1 构建口径防呆；生产构建入口自带 release clippy 守卫） |
| PS BOM（全仓 5 个 ps1） | 手动核验 + CI 守卫（全仓受版本控制 `.ps1`，`git ls-files`） | 5/5 BOM=True、PS 5.1 语法 0 错误 |

114 用例分布：protocol 25（幂等合并）· store 15（覆盖/bak/穿越/保留名）**+2（E10 大小上限）**+1（并发+WAL）· net 14（帧/会话/停机）· host 9（房间广播/存活探测/manifest）**+2（E10 端到端拒绝）**· agent 9（接收/重连/墓碑/E3 探测回路/E9 自动发现）· ipc 12（双工/令牌 ACL EqualSid 语义断言）· svc 11（配置/清理/运行时/服务/room_filter 四态）· gui(src-tauri) 27（窗口/ipc client/O-1 构建模式判定 2 项/**tips 真实数据通路 13 项**）

> 数字口径：冻结基线 108 → 114（+4 E10 +2 O-1）→ **127 = +13（P0 #79 tips 真实数据通路接线）**。

CI 作业：rust（fmt/clippy/test + PS BOM 守卫）+ web（pnpm build）+ **gui-release-clippy**（release profile 盲区守卫）

## 3 安全与可靠性收口（M1 期间发现并修复）

| 缺陷 | 发现方 | 处置 |
| --- | --- | --- |
| 尾随点/空格绕过类型拦截（学生可投毒 `.exe.`） | reviewer（PR #28） | sanitize/is_risky_ext 归一 + 保留设备名上收 protocol |
| 同版本 delete 被幂等去重吞掉（删除无法落地） | dev-backend（T-6 设计） | merge 语义修正：Equal + deleted 且本地未删 → Overwrite |
| Manifest 重推未按 owner/audience 收敛（可污染扩散） | reviewer-2（PR #34） | 过滤 + on_event 非教师事件忽略 + 回归测试 |
| agent 不回 Pong（重名探测误驱逐活跃学员） | qa（E3） | agent 补 Ping→Pong + 真实回环用例 |
| 令牌 icacls 限权从未生效（本地越权全权限） | qa（E7，真实二进制实证） | icacls 退役 → windows-rs API 直写 + EqualSid 断言 + fail closed + 存量自愈 + user_sid 双授予 |
| 并发打开 store 撞 BUSY（生产双连接场景） | CI（run 82） | WAL + busy_timeout 5000 + open 重试 |
| 注册表 `%*` 外层引号致多选塌缩 | reviewer-2（PR #66） | 裸 `%*` + CommandLineToArgvW 三形态证据 |
| PS 脚本 BOM 回归 | reviewer-2（PR #66 终审） | 补 BOM + **CI 守卫根治** |
| **beacon 自动连接缺失（学员须手填 IP）** | **reviewer-2（全仓巡检）** | **E9 补齐：discovery 泵 + room_filter 房间隔离 + 锁定首个目标；学员不传 -Room 也能入班** |
| **100MB 单文件上限从未实现（超限文件全量读内存扩散）** | **reviewer（验收报告评审，E10/#76）** | **store `fs::copy` 前 O(1) 拒绝 + host 发送前快速失败 + 端到端用例（含边界 100MB 通过 / 拒绝后链路正常）；流式推送记 M2** |
| **tips 气泡在 release 从不触发**（`push_tip` 仅被 debug 的 `demo_tips` 调用，`svc://notify` 与 `svc://share-received` 未接） | reviewer（PR #79 / P0） | `ipc_client::forward_loop` 接线：`ShareReceived` + 覆盖态 `ItemUpdated` → `notify_to_tip` → (id,version) 去重 → `tips::push_tip`；映射在数据源侧，暂停语义保持（emit 先于 show 判定）+ 13 例单测 |

## 4 已知 Deferral（17 项，均已有归属）

| # | 项 | 归属 |
| --- | --- | --- |
| 1 | ack 跟踪（FileAck/事件 ack 收集与状态回写） | #16 / #17 协同 |
| 2 | 占用重试完整退避（≤5 次；当前入队语义 + SCM 层策略） | #20 |
| 3 | Unicode 文本拖放（OLE IDropTarget） | **#20**（拖拽投放完整交互） |
| 4 | 学员回传（FR-S-02/03，host 现明确拒绝） | #16 |
| 5 | 删除同步全链（在线已完成；延迟补删端到端） | #17 |
| 6 | 离线待发队列（PendingOp 持久化 + 补发） | #18 |
| 7 | 会话身份绑定 + net client runner 抽取（含 WTS 取 SID） | #16 |
| 8 | 心跳 Pong 读超时（会话活性治理） | #16 |
| 9 | LocalSystem ACE 冗余累积（无害，评估收敛） | #16 |
| 10 | grant_for_session 无自动化单测（WTS 路径） | #16 / #21 |
| 11 | ensure_client 惰性重连竞态（单飞，进一步收敛 tips 双弹；现已有 (id,version) 去重兜底，P0 #79） | #16 |
| 12 | 「开机自启 已开启」硬编码改注入真实状态 | #20 |
| 13 | ServerHandle sessions 累积 / 退避不重置 | #22（备查级） |
| 14 | bak→final 崩溃回滚自愈（自愈路径已确认） | #18 / #22 |
| 15 | CI BOM 守卫改全仓扫描（当前盲区 docs/acceptance/scripts） | **#16（M2 首个任务）** |
| 16 | GHA runner ACL 写入静默无效机制调查 | research |
| 17 | 2 项测试补强 nit（pragma_query format! 拼接、gui.json BOM 剥离单测） | #21 |

## 5 交接 T-14（qa 验收）

**已执行（单机部分）**：TC-06 同名覆盖、TC-12 右键退出、TC-13 前半卸载清理、TC-01 教师侧、TC-02/03 多 agent 时延趋势、E7 user_sid 取证

**需双机/管理员环境（延后）**：
- TC-01 学员侧真实发现与重名拒绝（**学员安装不传 `-Room` 即可自动入班**，教师先建好房间再让学员加入）
- TC-02/03 真实多端分发
- TC-11 不登录收文件
- E3 真机复验（静默死亡/强杀两情形）
- E9 真实 beacon 发现（受限广播环境依赖）

**M3 范围**：安装包（NSIS）与静默部署、30 台 p95 压测、日志按天切分

## 6 验收材料索引

| 类别 | 位置 |
| --- | --- |
| 验收执行方案 v1.3.5 | [docs/acceptance/M1-acceptance-plan.md](../acceptance/M1-acceptance-plan.md) |
| 取证脚本（单机采集 + 跨节点 Δ 汇总） | [docs/acceptance/scripts/](../acceptance/scripts/) |
| GUI 手动/自动清单与截图 | `apps/classroom-gui/docs/t8..t12/` |
| 评审记录（全部 PR） | [docs/reviews/](../reviews/) |
| 缺陷台账（E1–E9） | 验收方案 §3.8 |

## 7 冻结结论

M1 代码侧**已冻结**：P0 全部功能实现并入库（含 E9 自动发现补齐）、**108 用例全绿**、fmt/clippy 零告警、CI 三作业守卫、安全缺陷链（E1–E9）全部闭环、17 项 Deferral 均有归属。

- 预发布：`v0.1.0-rc.1`
- **正式 `v0.1.0` 条件**：T-14 验收通过（单机部分已执行；双机项待资源到位后补验，release note 须注明 TC-02 缩减口径：≥3 台真机全绿 + 多 agent 压测代理，正式 30 台 p95 留 M3）

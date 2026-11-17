# M1（P0 最小闭环）代码冻结报告

| 项目 | 内容 |
| --- | --- |
| 里程碑 | M1-P0（能上一节课的最小闭环） |
| 冻结基线 | `bd34ed2`（含 T-12 / PR #66 `6b91583` + 终审记录 #67） |
| 冻结日期 | 2026-11-17 |
| 状态 | 代码侧冻结（验收待 T-14） |
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
| 安全缺陷 | #49 | #50 | E7 令牌 ACL（icacls→windows-rs API 直写） |
| 存储加固 | — | #46/#63 | busy_timeout + WAL 并发收口 |
| 取证锚点 | #37/#49 | #39/#48 | E4 broadcast/received/applied、E6 size 语义 |

### 1.3 GUI 侧全部入库

T-8 置顶图标(#29) · T-11 tips(#33) · T-9 教师界面+RealGuiClient(#53) · T-10 学员视图(#59) · T-12 桌面入口三件(#66)

## 2 质量门（实跑证据）

| 门 | 命令 | 结果 |
| --- | --- | --- |
| fmt | `cargo fmt --all --check` | exit 0 |
| clippy | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0（含 GUI src-tauri，无交叉破坏） |
| test | `cargo test --workspace` | exit 0：25 binaries / **106 passed / 0 failed / 0 panicked** |
| PS BOM | CI 守卫（scripts + apps 自有 ps1） | 3/3 有 BOM、PS 5.1 语法 0 错误 |

106 用例分布：protocol 25（幂等合并）· store 15（覆盖/bak/穿越/保留名）+1（并发+WAL）· net 14（帧/会话/停机）· host 9（房间广播/存活探测/manifest）· agent 8（接收/重连/墓碑/E3 探测回路）· ipc 12（双工/令牌 ACL EqualSid 语义断言）· svc 10（配置/清理/运行时/服务）· gui(src-tauri) 12（窗口/ipc client）

CI 作业：rust（fmt/clippy/test）+ web（pnpm build）+ PowerShell BOM 守卫

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

## 4 已知 Deferral（M2 集结）

| # | 项 | 归属 |
| --- | --- | --- |
| 1 | ack 跟踪（FileAck/事件 ack 收集与状态回写） | #16（M2） |
| 2 | 占用重试的完整退避（当前入队语义 + SCM 层策略） | #20（M2） |
| 3 | Unicode 文本拖放（OLE IDropTarget） | #16（M2） |
| 4 | 学员回传（FR-S-02/03，host 现明确拒绝） | #16（M2） |
| 5 | 删除同步全链（在线已完成；延迟补删端到端） | #17（M2） |
| 6 | 离线待发队列（PendingOp 持久化 + 补发） | #18（M2） |
| 7 | 会话身份绑定 + net client runner 抽取（含 WTS 取 SID） | #16（M2） |
| 8 | 心跳无 Pong 读超时（会话活性治理） | #16（M2） |
| 9 | LocalSystem ACE 冗余（无害，评估收敛） | #16（M2） |
| 10 | grant_for_session 无自动化单测（WTS 路径） | #16（M2） |
| 11 | GHA runner ACL 环境备案（机制调查） | research |
| 12 | CI BOM 守卫改全仓扫描（当前盲区 docs/acceptance/scripts） | #22（M3） |

## 5 交接 T-14（qa 真机验收）

**需管理员/多机环境**：
- TC-11：不登录收文件（服务开机自启 + 机器级数据根）
- TC-12：右键退出后服务存活 + 数据完整
- TC-13 前半：卸载清理（服务/自启/右键注册项）
- TC-01 双机：真实发现链路 + 重名拒绝（E3 真机复验：静默死亡/强杀两情形）
- user_sid 双授予取证：安装后 + 首次登录后各取一次 icacls

**M3 范围**：安装包（NSIS）与静默部署、30 台 p95 压测、日志按天切分

## 6 验收材料索引

| 类别 | 位置 |
| --- | --- |
| 验收执行方案 v1.3.5 | [docs/acceptance/M1-acceptance-plan.md](../acceptance/M1-acceptance-plan.md) |
| 取证脚本（单机采集 + 跨节点 Δ 汇总） | [docs/acceptance/scripts/](../acceptance/scripts/) |
| GUI 手动/自动清单与截图 | `apps/classroom-gui/docs/t8..t12/` |
| 评审记录（全部 PR） | [docs/reviews/](../reviews/) |
| 缺陷台账（E1–E8） | 验收方案 §3.8 |

## 7 冻结结论

M1 代码侧**具备验收条件**：P0 全部功能实现并入库、106 用例全绿、fmt/clippy 零告警、CI 三作业守卫、安全缺陷链（E1–E8）全部闭环、12 项 Deferral 均有归属。

**待 T-14 真机验收通过后打 `v0.1.0`**（release note 须注明 TC-02 缩减口径：≥3 台真机全绿 + 多 agent 压测代理，正式 30 台 p95 留 M3）。

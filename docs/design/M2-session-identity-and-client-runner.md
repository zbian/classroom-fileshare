# M2 设计：会话身份绑定与客户端会话运行器

| 项目 | 内容 |
| --- | --- |
| 状态 | 设计草案（#16 设计输入，待评审） |
| 基线 | `8b81bf0`（v0.1.0-rc.1，M1 已收口，108 用例） |
| 作者 | dev-backend |
| 关联 | M2 里程碑 #16–#21；Deferral 清单（M1 冻结报告 §4）；E3（#37 静默死亡，已由 PR #34 修复） |
| 目标读者 | lead、dev-gui、code-reviewer、qa-acceptance |

> 本文是 M2 会话身份绑定（Session Identity）与客户端会话运行器抽取（Client Runner）的设计输入。
> 分两步落地（S1/S2 纯重构、S3 行为变更），两 PR 分开评审。

## 1 背景与目标

M1 收口时，`crates/net` 与 `crates/agent` 各有一份 TCP 会话帧分发实现：

- 服务端：`net::session::run_session` + `SessionHandler` 回调（join/事件/分块流/断线/清单/Pong）
- 客户端：`agent::agent_session` 手写 `select!` 帧循环（心跳、幀分派、重连由 `connect_loop` 包裹）

两份实现已经发生过一次真实漂移：host 的「重名存活探测」（PR #34 B）依赖客户端应答 `Frame::Ping`，而 agent 手写循环漏了该分支（Ping 落入 `other =>` 忽略），导致**活跃学员被误判僵死驱逐**（E3，issue #37）。当时以最小修（agent 补 Ping→Pong）+ 真实回环用例收口（PR #34 修复包）。

同时，M2 开放学员回传（FR-S-02/03）后，服务端必须能回答「**这条事件来自哪个已认证会话**」，否则：

1. 学员 A 可伪造 `owner_id = B` 的 upsert，污染教师端 B 分组；
2. 学员可伪造 `audience = All`，把内容广播给其他学员（PRD 3.3 明令学生互不可见）；
3. ack 无法定向（Deferral #1 ack 跟踪）。

M2 设计目标：

- **G1** 会话身份绑定：服务端事件处理具备会话级身份与权限校验，杜绝冒名与越权广播。
- **G2** 客户端会话运行器抽取：帧语义单点维护，从结构上消除「服务端/客户端双实现漂移」这类缺陷类别（E3 是它的第一个受害者，不是最后一个）。
- **G3** 为 Deferral 项提供落点：ack 跟踪、Pong 读超时（会话活性治理）、静默死亡真机复验的自动化前置。

非目标（M2 不做）：跨网段、重连后事件重放协议变更、教师端 GUI 直连 socket（M1/M2 仍走 ipc；role 绑定仅预留）。

## 2 现状与缺口（代码事实，基线 8b81bf0）

| 位置 | 现状 | 缺口 |
| --- | --- | --- |
| `crates/net/src/session.rs:32` | `async fn on_event(&self, ev: Event, tx: &mut Sender<Frame>)` | 不携带 session_id，服务端无法知道事件来自哪条连接 |
| `crates/net/src/session.rs:48` | `on_manifest_req(&self, tx)` | 同上；且未区分「已 join / 未 join」 |
| `crates/net/src/session.rs:36` | `on_file_chunk_stream(&self, header, chunks, expected)` | 同上（M2 学员文件回传必需） |
| `crates/net/src/session.rs:153-169` | `joined_session: Option<String>` 仅用于 `on_pong`/`on_disconnect` 回调 | 分发 `Event`/`ManifestReq` 时不带该身份，**未 join 连接也能发事件** |
| `crates/host/src/hub.rs:373-380` | `on_event` 仅校验 `ev.item.owner_id != TEACHER_OWNER → 丢弃`（M1 最小加固） | M2 学员事件 owner_id 是昵称，该防线失效；无 audience 校验；无法区分会话 |
| `crates/host/src/hub.rs:42,355` | `Conn { nickname, tx, pong_at }`，session_id 作键 | 已具备身份存储，缺的是「事件 → 会话」的传递通道 |
| `crates/agent/src/lib.rs:167` | `agent_session` ~100 行手写帧循环 | 与 net 服务端实现重复；心跳/Ping 应答/hello 拒绝分类各自维护 |

## 3 会话身份绑定设计（S3）

### 3.1 接口修订 #9①

`SessionHandler` 三个回调增加会话身份参数（**仅新增参数**，agent/GUI 侧零影响——agent 是客户端，GUI 走 ipc）：

```rust
#[async_trait]
pub trait SessionHandler: Send + Sync {
    async fn on_hello(&self, hello: Hello, tx: &mut mpsc::Sender<Frame>) -> HelloReply;

    /// 修订 #9①：携带已认证会话 id（join 成功后的 session_id）
    async fn on_event(&self, session_id: &str, ev: Event, tx: &mut mpsc::Sender<Frame>);

    async fn on_file_chunk_stream(
        &self,
        session_id: &str,
        header: FileHeader,
        chunks: mpsc::Receiver<FileChunk>,
        expected_sha256: String,
    ) -> FileAck;

    async fn on_disconnect(&self, session_id: &str);
    async fn on_manifest_req(&self, session_id: &str, tx: &mut mpsc::Sender<Frame>);  // 修订 #9①
    async fn on_pong(&self, session_id: &str);
}
```

### 3.2 未 join 守卫（net 层）

`run_session` 已维护 `joined_session: Option<String>`。修订后分发规则：

- `Frame::Event` / `Frame::FileHeader` / `Frame::FileChunk` / `Frame::FileEnd` / `Frame::ManifestReq`：**仅当 `joined_session.is_some()` 时分发**；否则 `tracing::warn!` 丢弃（不回帧、不断连接，避免被探测）。
- `Hello` 重复发送：维持现行为（忽略/可视为重入攻击面，M2 记 warn）。

行为说明（写入 PR 描述）：未 join 连接此前可直接触发 `on_event`（服务端会尝试落库/广播），修订后一律不进入 handler——这是**收紧**，不改变已认证连接的任何路径。

### 3.3 host 三重校验（hub.on_event）

按序校验，任一失败 → `warn` + 丢弃（不落库、不广播、不 ack）：

1. **会话存在**：`conns.get(session_id)` 命中（join 注册过；断开后表项已删）
2. **身份一致**：`ev.item.owner_id == conn.nickname`（冒名核心防线）
3. **方向合法**：`ev.item.audience == Audience::Teacher`（学员只能定向教师；伪造 `All` 拒绝，PRD 3.3）

`owner_role` 强制 `Student`（服务端收到即改写/校验）；`kind/size/sha256` 等字段的合法性由 store 层二次校验（既有路径）。

delete/rename 事件同校验；`new_name` 的净化沿用 store/sanitize 路径。

### 3.4 教师连接与 role 绑定（预留）

M1/M2 教师操作走 ipc → host core（不经 socket），故 host 无教师 socket 会话。

预留方案（M3 或需要时）：`Conn` 增 `role: Role`；`on_event` 按 role 约束：
- Student：owner==nickname 且 audience==Teacher
- Teacher：owner==TEACHER_OWNER，audience 不限（可广播）

### 3.5 ack 路由（Deferral #1 落点）

- 事件级 ack：`Frame::Event(ev)` 中 `kind=Ack`，由客户端在成功 apply 后回发；host 按 `session_id` 定向确认（取代广播式 ack 收集）。
- 文件级 ack：`Frame::FileAck { item_id, sha256_ok }` 已在分块流路径，绑定 session_id 后可精确记账（哪个学员确认了哪个文件）。
- 状态映射：`ShareItem.sync_state` 的 `PendingSync → Synced` 由 ack 聚合驱动（M2 #17 起）。

## 4 客户端会话运行器抽取（S1/S2）

### 4.1 不变量

1. 帧编解码/心跳/重连分类**单点维护**（net crate）
2. 客户端与服务端共享同一帧语义（含 Ping→Pong 探测应答）
3. host 侧零改动（S1/S2 不触碰服务端行为）
4. agent 对外行为等价（S2 后现有 9 用例**断言零改动**全绿）

### 4.2 类型设计

```rust
/// 客户端会话回调：agent 实现，把帧映射到自身接收状态机。
#[async_trait]
pub trait ClientHandler: Send + Sync {
    /// 收到服务端下发的帧（Event / FileHeader / FileChunk / FileEnd / Manifest / Pong / Ping）。
    /// 返回 Err 视为致命（结束会话）；Ok 继续。
    async fn on_frame(&self, frame: Frame) -> anyhow::Result<()>;
}

pub struct ClientSessionConfig {
    pub addr: SocketAddr,
    pub hello: Hello,                 // 含 protocol_version/nickname/role/pin
    pub heartbeat: Duration,          // 5s
    pub request_manifest: bool,       // join 成功后自动发 ManifestReq（M1 行为）
    pub connect_timeout: Duration,    // 5s
}

pub enum SessionEnd {
    /// 对端关闭/IO 断线：可重连
    Closed,
    /// join 被拒（错 PIN/重名/版本）：致命，不重连
    Rejected(HelloErr),
}

pub async fn run_client_session<H: ClientHandler + 'static>(
    cfg: ClientSessionConfig,
    handler: H,
) -> anyhow::Result<SessionEnd>;
```

内部职责（与 `Server` 对称）：

| 职责 | Server（既有） | Client（新增） |
| --- | --- | --- |
| 建连 | `accept` | `connect` + 超时 |
| 握手 | `on_hello` → `HelloReply` | 发 Hello → 收 HelloReply（拒 → `Rejected`） |
| 心跳 | 无（被动） | 定时发 `Ping` |
| 探测应答 | 收 `Ping` 自动回 `Pong` | 收 `Ping` **自动回 `Pong`**（E3 根因，结构性修复） |
| 清单请求 | `on_manifest_req` | 可选自动 `ManifestReq` |
| 帧分派 | `SessionHandler` 回调 | `ClientHandler::on_frame` |
| 断线 | `on_disconnect` | 返回 `SessionEnd::Closed` |

### 4.3 agent 迁移形态（S2）

```rust
// 现状：agent_session 手写循环 ~100 行
// 迁移后：
struct AgentSession<'a> { incoming: &'a mut Incoming, notify: &'a Sender<AgentEvent> }
#[async_trait] impl ClientHandler for AgentSession<'_> {
    async fn on_frame(&self, frame: Frame) -> anyhow::Result<()> {
        match frame { /* Event/FileHeader/Chunk/End/Manifest/Pong → 既有状态机 */ }
        Ok(())
    }
}
// agent_session 变为：构造 ClientSessionConfig + AgentSession → run_client_session
// connect_loop 保留（退避重连），S2 后可选上移为 run_client_with_reconnect
```

## 5 执行顺序

| Step | 内容 | 行为影响 | PR |
| --- | --- | --- | --- |
| S1 | net：`ClientHandler` + `run_client_session` + 单测（host/agent 不动） | 无（纯新增） | PR-1 |
| S2 | agent：`agent_session` 改薄封装 → 9 用例断言零改动全绿 | 无（重构，行为等价） | PR-1（或紧随 PR-2） |
| S3 | net+host：三回调加 `session_id` + 未 join 守卫 + host 三重校验 + 3 新用例 | **有**（收紧：未 join 事件/冒名/越权被拒） | PR-2 |
| S4 | M2 回传链路（#17/#18）在身份绑定后落 `on_event` / 分块流 | 有 | M2 后续 |
| S5 | host ack 收集与按 session 定向（Deferral #1 闭项） | 有 | M2 后续 |

拆分理由：S1/S2 是纯重构（断言零改动是验收条件），S3 是行为收紧（需 reviewer 重点看未 join 守卫与三重校验）——混在一起会让「行为变更藏在重构里」，重演 E3 教训。

## 6 与 E3 / Pong 超时 / ack 的关系矩阵

| 能力 | E3（Ping→Pong 探测应答） | Pong 读超时（Deferral #8） | ack 跟踪（Deferral #1） | 会话身份绑定 | 客户端运行器 |
| --- | --- | --- | --- | --- | --- |
| S1（net 新增 client runner） | ✅ **结构性消除**：Ping→Pong 应答成为运行器内置职责（不再是 agent 手写分支） | — | — | — | ✅ 落地 |
| S1 单测 | ✅ mock server ↔ run_client_session 覆盖 Ping→Pong | — | — | — | ✅ ~5 用例 |
| S2（agent 薄封装） | ✅ 行为等价保留（9 用例含 E3 探测回路守护） | — | — | — | ✅ 完成迁移 |
| S3（身份绑定） | 依赖 S1/S2（客户端能应答探测） | —（读超时在 S3 之后做，需要会话活性通道） | ⏳ 提供 session_id 定向基础，ack 本身在 S5 | ✅ 落地（冒名/越权防线） | — |
| S4（回传链路） | — | — | 依赖 S3 身份 | ✅ 使用方 | — |
| S5（ack 收集） | — | — | ✅ 闭项 | 依赖 S3 | — |
| 后续（M2 后段） | 真机静默死亡复验（TC-01 双机） | ⏳ Pong 读超时实现（会话活性治理） | — | — | 可选：`run_client_with_reconnect` 上移 |

结论：E3 在 S1/S2 结构性收口（客户端运行器内置应答）；Pong 读超时在 S5 之后单独做（需要 S3 会话活性数据）；ack 依赖 S3 的 session_id 定向，S5 闭项。

## 7 对现有 108 用例的影响清单

| crate | 现有 | S1/S2 影响 | S3 影响 | 处理 |
| --- | --- | --- | --- | --- |
| protocol | 25（merge） | 无 | 无 | 0 改动 |
| store | 15 + 1（concurrent/WAL） | 无 | 无 | 0 改动 |
| net | 1（beacon）+ 6（frame）+ 7（session） | **新增** ~5 client runner 用例；`SessionHandler` mock 在 S3 需加参数 | session 测试里 `MockHandler` 的 3 个回调 impl 加 `session_id: &str` 参数（7 用例中 3 个 impl 相关） | S1 新增；S3 改 mock 签名，断言不变 |
| host | 9（hub） | 无 | hub 三回调 impl 加参数；**新增 3 用例**（冒名/越权/未 join）；现有 9 用例断言不变 | S3 改 impl + 新增 |
| agent | 9 | **S2 后断言零改动全绿**（重构验收条件） | 无（client 侧不受 SessionHandler 影响） | 0 断言改动 |
| ipc | 9 + 3 | 无 | 无 | 0 改动 |
| svc | 10 + 1 | 无 | 无 | 0 改动 |
| gui(src-tauri) | 12 | 无 | 无 | 0 改动 |

汇总：S1 预计 **+5 用例**（net）；S2 **0 断言改动**（9 回归）；S3 **+3 用例**（host）、`MockHandler` 签名适配（net 7 用例内部 impl，断言不变）。M2 结束后全量约 **116 用例**。

## 8 测试策略

1. **net 层（S1）**：mock TCP 对连（`TcpListener` 随机端口 + `run_client_session`）覆盖：hello 拒绝分类（`Rejected(WrongPin)`）、心跳发送、服务端 Ping→客户端 Pong、FileChunk 流回调、对端关闭 → `Closed`。
2. **agent 回归（S2）**：现有 9 用例（含 `auto_discovers_and_connects_without_manual_addr`、E3 探测回路）**断言零改动**全绿为重构验收标准。
3. **host 身份（S3）**：真实 host + 真 agent join 后，用「定制客户端」注入违规事件：
   - owner=bob（会话是 alice）→ 拒
   - audience=All（学员）→ 拒
   - 未 join 直发 Event → 拒（服务端无落库/无通知）
4. **跨 crate 回归**：`cargo test --workspace`（108 + 新增）全绿 + fmt/clippy。

## 9 风险与回滚

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| S3 改 `SessionHandler` 签名波及 host 三回调 | 编译期破坏面集中但可见 | host 适配 diff 写入 PR 描述（lead 要求）；net 14 + host 9 + agent 9 全绿后合入 |
| 未 join 守卫误伤合法路径 | 客户端在 join 应答到达前发事件（现有实现不会） | 守卫只 warn 丢弃、不断连接；agent 侧 S1 保证 join 完成后再发帧 |
| S2 重构引入行为差异 | 接收/重连语义变化 | 9 用例断言零改动 + 真实回环（真 host 真 agent） |
| ack 路由与既有 `sync_state` 状态机耦合 | 状态错乱 | S5 单独设计 `SyncState` 迁移表，先只记账不改状态，再切状态 |

回滚：S1/S2 是纯新增/等价重构，可单独 revert；S3 若线上受阻，`on_event` 三重校验可回退为 M1 的 owner 白名单（单 commit revert）。

## 10 验收标准

- [ ] S1：net 新增客户端运行器，单测覆盖 §8.1 五类；`cargo test -p classroom-net` 全绿
- [ ] S2：agent 9 用例断言零改动全绿；`agent_session` 行数显著下降（薄封装）
- [ ] S3：host 3 新用例全绿（冒名/越权/未 join 均被拒）；net `MockHandler` 适配后 7 用例断言不变全绿
- [ ] 全量：`cargo test --workspace` 全绿 + fmt/clippy -D warnings
- [ ] PR 描述：S3 附 host hub 三回调适配 diff 摘要 + 未 join 守卫行为说明（lead 要求）

## 附录 A Deferral 映射

| M1 Deferral（冻结报告 §4） | 本文落点 |
| --- | --- |
| ack 跟踪 | §3.5 + S5 |
| 会话身份绑定 / run_client_session | §3 + §4 + S1/S2/S3 |
| Pong 读超时（会话活性） | S3 后（§6 矩阵） |
| 静默死亡真机复验 | TC-01 双机（S2 后自动化前置已就位） |
| 学员回传 | S4（#17/#18） |

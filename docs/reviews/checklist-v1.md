# 评审检查单 v1

| 项目 | 内容 |
| --- | --- |
| 版本 | v1（已由 lead 认可为统一评审标准） |
| 日期 | 2026-11-13 |
| 作者 | code-reviewer |
| 依据 | [final_draft.md](../../final_draft.md)（PRD V1.0，精读第 3/5/6/7/9/10 章）；[docs/DEVELOPMENT.md](../DEVELOPMENT.md) Review 必查清单；[docs/plans/2026-11-13-m1-p0.md](../plans/2026-11-13-m1-p0.md) |

## 用途与判定规则

- 本清单是后续所有 PR 的统一评审标准；PR 审查时按其 area 勾选对应组 + 「通用」组。
- 判定：任一适用条目不符 → **request changes**（须注明 PRD 章节依据）；范围外问题走评论，不计入本次结论。
- M1 计划未排期但 PRD 已定义的行为（如占用重试、删除同步全链路），若 PR 未实现，须在对应 issue 的 **Deferral 字段**声明延后至 M2；无声明视为缺失。
- 审查结论与逐条勾选记录写入 `docs/reviews/PR-<编号>.md`。

## 通用（每个 PR 必查）

- [ ] 与 PRD 对应 FR 的交互规则/异常边界/验收标准逐条符合（DEVELOPMENT.md 必查第 1 条）
- [ ] 公共接口签名与 M1 计划锁定的 Interfaces 一致，未擅改（计划 T-02/T-03 等「签名不得擅改」）
- [ ] 核心逻辑有真实单测/集成测试：先红后绿、断言行为而非实现细节，非形式化覆盖（DEVELOPMENT.md 必查）
- [ ] 日志统一 tracing，不含文件正文、文本内容、学生昵称等隐私（PRD 9.6）
- [ ] 不引入 PRD 2.5 范围外功能（跨网段/云端中继/账号/版本历史/全局热键）
- [ ] 分支/提交/PR 格式合规：Conventional Commits、Refs/Closes #issue、GUI 变更附截图（DEVELOPMENT.md）

## protocol（T-02）

- [ ] ShareItem 字段与 PRD 3.5 一一对应：version 从 1 起、deleted、sync_state 六态（已同步/待发送/待同步/延迟删除/被占用重试/失败）
- [ ] scope_key = owner_id | audience | 文件名(文本标题)；同名判定仅在同作用域内生效，不同学生同名互不覆盖（PRD 5.7/6.7）
- [ ] 事件仅四类 upsert/delete/rename/ack，均带全局唯一 event_id 与 version（PRD 3.6）
- [ ] merge 幂等：本地无或 version 更高→Overwrite；重复 event_id→IgnoreDuplicate；version 更旧→IgnoreStale；同 version 重放不产生第二条（PRD 3.6）
- [ ] sanitize_file_name 剥路径成分、拒绝 `..`/绝对路径/Windows 非法字符，测试须含 `..\..\evil.exe`、`a/b.txt`、`报告<>.pdf`（PRD 9.4 路径安全、5.3）
- [ ] is_risky_ext 覆盖 exe/bat/cmd/com/scr/pif/vbs/js/jar/lnk（PRD 5.3、9.4、EX-05）
- [ ] 常量与限制一致：单文件 ≤100MB、文本 ≤10000 字、分块 1MB、PROTOCOL_VERSION 存在（PRD 9.4、3.1）
- [ ] validate_text：纯空白标题/内容拒绝、10001 字拒绝（PRD 5.2、9.4）
- [ ] crate 纯逻辑零 IO，时间由调用方注入（计划 T-02 Step3 可测性约定）

## store（T-03）

- [ ] 覆盖链路严格为 `.part → SHA-256 校验 → 原子替换 → 旧版转唯一 .bak → version+1`，顺序不可颠倒（PRD 5.7/7.3；DEVELOPMENT.md 必查）
- [ ] 仅保留 1 份 .bak，新覆盖替换旧备份；两版后目录仅 `作业.docx` 与 `作业.docx.bak` 各一（PRD 5.7、TC-06）
- [ ] 哈希不符→Err 且无正式文件落盘、.part 清理（PRD EX-07、9.2）
- [ ] risky 扩展入库前拒绝（PRD 9.4、EX-05）
- [ ] 仅写应用数据目录 `root/repo/<owner_id>/`，净化后路径不得逃逸 owner 目录（PRD 9.4 数据位置）
- [ ] schema：share_items 作用域键 UNIQUE 部分索引 WHERE deleted=0；event_log(event_id PK) 支持去重（PRD 3.6、5.7）
- [ ] apply_remote 重放同 event_id→IgnoreDuplicate，不改索引（PRD 3.6）
- [ ] 崩溃安全：中断不留被当作完整文件的半成品；重启依 SQLite 索引恢复（PRD 9.2）
- [ ] 原子替换失败(占用)→RetryLocked 入队、退避重试≤5 次、不阻塞其他共享（PRD 5.7/EX-06/7.6）**——M1 已确认延后至 M2（PRD 12 章 P1 范围），issue 须有 Deferral 声明**

## net（T-04）

- [ ] 帧格式 4B 大端长度+1B 类型+payload；超限帧(>8MB)拒绝防滥用（计划 T-04 安全约定）
- [ ] Hello 携带协议版本，不匹配→VersionMismatch 拒绝（PRD 3.6、4.5）
- [ ] HelloReply 错误码 WrongPin/DupNickname 语义正确（PRD 5.1、6.1 EX-02）
- [ ] 文件分块 ≤1MB 逐块传输、整体 SHA-256 校验、FileAck 携带 sha256_ok（PRD 3.1、EX-07）
- [ ] Beacon 仅含房间码/端口/pin_required/主机名，不泄露 PIN 明文与个人信息（PRD 3.1、9.4 准入）
- [ ] Manifest 支持版本清单比对（PRD 7.5）
- [ ] 端口约定 UDP 47820 / TCP 47821；端口冲突自动更换并提示（计划 RK-07 落地；PRD 5.1 EX-11）
- [ ] 测试用真实 TCP 回环对连，非 mock（测试真实性）

## host（T-05）

- [ ] 昵称查重：重复拒绝加入并要求改名（PRD 6.1 EX-02、5.5）
- [ ] PIN 校验：错误 PIN 无法加入（PRD 5.1、TC-01）
- [ ] 仅广播 audience=All 的教师内容；学生提交绝不转发给其他学生（PRD 3.3 第 4 条）
- [ ] 学员上线先发 Manifest 对齐再推增量（PRD 7.5）
- [ ] upsert 先入 store 再广播事件，文件随 FileHeader/Chunk 推送并跟踪 ack（PRD 7.1）
- [ ] 集成测试含 ≥2 模拟学员经真实 TCP（测试真实性）

## agent（T-06）

- [ ] 发现双通道：beacon 自动发现 + 手动 IP（PRD RK-04）
- [ ] 文件先收分块至 .part、SHA-256 校验通过才 apply_remote（PRD 7.1/7.3、EX-07）
- [ ] 断线重连：拉 Manifest 对齐补齐错过的 upsert，event_id 去重无重复条目（PRD 7.5、3.6、TC-09）
- [ ] 学员端不出现任何他人内容（PRD 3.3 第 3 条、6.4）
- [ ] 教师机离线→只读缓存模式，本地查看不崩溃（PRD 3.1、EX-08）

## ipc（T-07）

- [ ] 令牌鉴权：错 token 拒绝连接，测试覆盖（PRD 9.4 本地通信）
- [ ] token 文件 ACL 仅当前用户可读（计划 T-07；PRD 9.4）
- [ ] 服务与 GUI 版本不一致时以协议版本号拒绝并提示升级（PRD 4.5）
- [ ] 往返测试：请求+notify 双向推送（测试真实性）

## gui（T-08..T-12）

- [ ] 图标：置顶、不占任务栏、拖动+边缘吸附、位置按工作区百分比持久化、多显示器/DPI、单实例（PRD 4.1、EX-12）
- [ ] tips：15s 自动消失、倒计时进度条、≤3 条堆叠+「还有 N 条」、悬停暂停/移出继续、点击直达、退出清空不落盘、先落盘再提醒（PRD 4.2、7.2、TC-10）
- [ ] 右键菜单六项齐全；「暂停接收提醒」只抑制 tips 不影响接收落盘；退出仅结束 GUI 不停服务不删数据（PRD 4.3、TC-12）
- [ ] 双击 1s 内开主界面、单例聚焦、关窗不退出（PRD 4.4）
- [ ] 学员「教师共享」只读：无任何增删改控件、有只读标识（PRD 6.4；DEVELOPMENT.md 必查）
- [ ] 学员端无「发送给全班」选项，固定展示「发送对象：仅教师」（PRD 6.2/6.5）
- [ ] 状态=文字标签+色块，不单靠颜色（PRD 4.6；DEVELOPMENT.md 必查）
- [ ] 关键文案与 PRD 8.3 逐字一致（删除确认/覆盖结果/占用重试/昵称重复/剪贴板为空/离线待发）
- [ ] 主色教师蓝 2563EB、学员青 0D9488；正文 13–14px 微软雅黑（PRD 4.6）
- [ ] 剪贴板：读取不清空不修改、被占用重试、空/不支持类型入口置灰（PRD 4.3、EX-03）
- [ ] 文本确认框：标题必填、显示字数、纯空白禁止、≤10000 字、不清空剪贴板（PRD 5.2/6.2）
- [ ] 首次昵称：重名拒绝文案、PIN 输入、未发现教师机可离线进入、持久化后不再弹（PRD 6.1）
- [ ] 目录拖入明确提示不支持；>100MB 拒绝且不影响后续发送（PRD 5.3、EX-04）
- [ ] 服务未起（EX-10）有提示与重试入口（PRD 第 10 章）
- [ ] 手动测试清单写入 PR 描述并附截图（DEVELOPMENT.md PR 流程）

## service（T-13）

- [ ] 双进程模型：服务 Session 0 无 UI，GUI 用户会话登录自启，不得合并实现（PRD 3.2、RK-01）
- [ ] 未登录可接收落盘，登录后 GUI 立即可见（PRD 4.5、TC-11）
- [ ] 服务崩溃 SCM 按策略自动重启（failure flag）（PRD 4.5、9.2）
- [ ] 卸载注销服务+自启项+右键注册项；用户数据默认保留+显式清除选项（PRD 9.5、TC-13）
- [ ] 安装需管理员权限；无权限降级为「仅当前用户登录自启」并说明局限（PRD 4.5、RK-06）
- [ ] 日志按天切分、保留 7 天、设置面板有查看入口、无隐私内容（PRD 9.6）
- [ ] 升级保留仓库与索引、执行 schema 版本迁移（PRD 9.5）

## CI（T-01a；2026-11-17 起改为本机 CI）

- [ ] 本机 CI 门禁 `.\scripts\local-ci.ps1` **ALL PASS**（6 项：PS BOM 守卫 / fmt / clippy / test / pnpm build / GUI release clippy），汇总输出贴 PR（DEVELOPMENT.md「本机 CI（门禁）」）
- [ ] GitHub Actions 已停用（`.github/workflows/ci.yml` 移除，用户指令 2026-11-17）；无 GHA 相关表述残留
- [ ] 门禁 1 含双面规则：`.ps1` 必须有 UTF-8 BOM、`.json` 数据不得带 BOM

# E4 取证锚点真实格式演练记录

| 项目 | 内容 |
| --- | --- |
| 日期 | 2026-11-16 |
| 执行人 | qa-acceptance |
| 代码基线 | `origin/main` @ `73e17d3`（PR #39，E4 锚点已合入） |
| 验证环境 | 独立 worktree `.wt/verify-main`（分支 `qa/verify-main`，**未推送**）；Rust/cargo 1.100.0 (MSVC) |
| 演练方式 | **真实 host + 真实 agent**（非测试桩）+ **真实 tracing sink**（复刻 `apps/classroom-svc/src/logging.rs`：文件+stderr 双写、`.with_target(false)`、`EnvFilter` 默认 `info`），无需安装 Windows 服务 |
| 演练内容 | 真实广播 3MB 文件（含空格文件名「课堂讲义 副本.pptx」）+ 真实文本（「课堂通知」），学员端接收落盘 |
| 演练脚本 | `.wt/verify-main/crates/agent/tests/qa_anchor_rehearsal.rs`（QA 本地，未入上游） |

## 产物

| 文件 | 说明 |
| --- | --- |
| `E4-anchor-rehearsal-svc.log` | 真实产出的 svc 格式日志（含三条锚点） |
| `E4-anchor-collect-output.txt` | QA 取证脚本（`collect-m1-evidence.ps1`）对上述日志的解析输出，含机器可读 TSV 锚点段 |

## 真实日志（原始行）

```
2026-11-16T03:52:57.488974Z  INFO 教师端启动 room=XK-204 port=1690
2026-11-16T03:52:57.547664Z  INFO 学员加入 nickname=alice session_id=eef7cf44-ee28-4fc1-b2ae-309cbf1f7430
2026-11-16T03:52:57.547817Z  INFO 已加入 addr=127.0.0.1:1690
2026-11-16T03:52:57.646034Z  INFO broadcast item_id=fcb980db-641e-4dd2-ae1e-1185d95d0c88 file=课堂讲义 副本.pptx size=3145728 audience="all"
2026-11-16T03:52:57.681246Z  INFO received item_id=fcb980db-641e-4dd2-ae1e-1185d95d0c88 file=课堂讲义 副本.pptx size=3145728 from=teacher
2026-11-16T03:52:57.700370Z  INFO applied item_id=fcb980db-641e-4dd2-ae1e-1185d95d0c88 version=1
2026-11-16T03:52:57.719377Z  INFO broadcast item_id=0cbe9431-9eaf-4869-a420-7b491fdc2b33 file=课堂通知 size=9 audience="all"
2026-11-16T03:52:57.720554Z  INFO received item_id=0cbe9431-9eaf-4869-a420-7b491fdc2b33 file=课堂通知 size=27 from=teacher
2026-11-16T03:52:57.740378Z  INFO applied item_id=0cbe9431-9eaf-4869-a420-7b491fdc2b33 version=1
```

## 结论

1. **行内布局**：消息在前、字段在后（`INFO broadcast item_id=… file=… size=…`）；无 target 模块路径——与 #14 更正一致，QA 按「关键词 + 字段」匹配。
2. **时间戳为 UTC**（`…Z`）——与文件 mtime（本地时区）**不可混用**，本地 UTC+8 时相差 8 小时；取证口径必须两端同源。
3. **文件名含空格正确解析**（「课堂讲义 副本.pptx」完整取到）。
4. **`applied` 锚点仅含 `item_id` + `version`**（缺 file/size）→ E5；Δ 按 item_id 对齐，判定不受影响。
5. **文本条目 size 两侧语义不同**：teacher=9（字符数，符 PRD 3.5）、agent=27（字节数）→ E6。
6. **teacher 的 `file=` 取净化后 title、agent 的 `file=` 取原始 file_name** → 文件名含非法字符时两侧不同，证据表以 item_id 为准。
7. **端到端链路**：真实日志 → `collect-m1-evidence.ps1` → TSV → `build-delta-table.ps1` → Δ 与判定，实测 Δ=0.054s，与原始时间戳 `57.700370 − 57.646034` 精确吻合。
8. **文件内为完整单行**（实测长度 134/131/120）；此前观察到的「折行」系控制台视觉折行，非文件内容。

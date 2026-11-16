# E6 复验记录（PR #48）

| 项目 | 内容 |
| --- | --- |
| 日期 | 2026-11-16 |
| 执行人 | qa-acceptance |
| 被验对象 | PR #48 `fix(e6-received-size` @ c0b6ee8（`fix(agent): received 锚点 size 改用 item.size 对齐 PRD 3.5（E6）`） |
| 复验方式 | 在独立 worktree `.wt/verify-main` 检出 PR #48 分支，跑**真实 host + 真实 agent** 广播演练（文件 + 文本各一条），读取**真实日志**；并跑上游测试套件 |
| 判定口径 | 文本条目 teacher/agent 两侧 `size` 相等且均等于**字符数**（PRD 3.5）；文件条目两侧均为字节数 |

## 修复前（main，E6 未修）

```
... INFO broadcast item_id=0cbe9431-… file=课堂通知 size=9  audience="all"
... INFO received item_id=0cbe9431-… file=课堂通知 size=27 from=teacher      ← 字节数（UTF-8 9×3）
```

## 修复后（PR #48）

```
... INFO broadcast item_id=11641b78-… file=课堂通知 size=9  audience="all"
... INFO received item_id=11641b78-… file=课堂通知 size=9  from=teacher       ← 字符数，与 broadcast 一致
```

文件条目两侧未受影响（均 3145728 字节）：

```
... INFO broadcast item_id=466fc938-… file=课堂讲义 副本.pptx size=3145728
... INFO received item_id=466fc938-… file=课堂讲义 副本.pptx size=3145728
```

## 测试

- 上游套件 `cargo test -p classroom-agent -p classroom-host -p classroom-protocol -p classroom-store -p classroom-net` → **EXIT=0**，全部通过（agent 8 / host 9 / protocol 25 / store 15 / net 6+7）。
- PR 新增断言把语义分离固化进回归：文本条目 `item.size == 5`（"明天带教材" 5 字符）且正文实体 UTF-8 为 **15 字节**（≠5 属预期）。

## 结论

**E6 复验通过**——文本条目两侧 size 语义已对齐 PRD 3.5，Δ 计算与判定不受影响（Δ 仍按 item_id 对齐）。待 PR #48 合并后关闭。

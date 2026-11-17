# #127 补充：GUI 接入服务端内容搜索（PRD 5.4「按名称与内容搜索」）

## 背景
#126 已把 store 的 `list { keyword }` 改为「**标题或文本正文**包含（大小写不敏感）」命中，
**文件正文不参与**（文本正文按本地 .txt 读取后内存匹配）。GUI 此前仍为客户端 `title.includes(keyword)`，
正文搜索在界面上不可用。

## 变更（仅 `apps/classroom-gui/**`；`crates/**` 零改动）
| 位置 | 内容 |
| --- | --- |
| `store.refresh()` | 关键字非空时调用 **服务端** `list { keyword }`；同时并行请求一次无 keyword 的全量（仅用于「筛选后 N / M」的 M），设置 `serverFiltered=true` / `totalCount` |
| `store.setKeyword()` | **200ms 防抖**（避免逐字符 IPC）；关键字清空时立即重拉全量（「清空恢复」要即时） |
| `store.serverFiltered/totalCount` | 新增状态：视图据此避免把「命中正文但标题不含」的条目二次剔除，并让 M 为未过滤总数 |
| `views/MySharesTab.tsx` / `TeacherSharesTab.tsx` | 服务端已按 keyword 过滤时，客户端只做**类型筛选**；meta 显示「筛选后 N / M 条」（M=未过滤总数） |
| 类型筛选 / 虚拟滚动 / 按 Tab 独立筛选 | 全部保留（与 #90 一致） |

## 真实链路证据（release GUI + 隔离真实 svc + CDP 19233；`docs/t127c/`）
环境：隔离 `LOCALAPPDATA/PROGRAMDATA`，教师 svc（房间 CS-127），经真实 IPC 创建 3 条内容：
- 文本「第一章要点」：标题**不含**关键词，正文含 `CONTENTKW-abc`
- 文本「TITLEKW-标题示例」：标题含关键词 `TITLEKW`，正文不含
- 文件「资料汇编.pdf」：文件名不含关键词，文件正文含 `CONTENTKW-abc`（应**不**被搜索命中）

| 断言 | DOM 实测 |
| --- | --- |
| 初始 | `共 3 条`（3 行） |
| **正文关键词** `CONTENTKW-abc` | 命中 `['第一章要点']`；`筛选后 1 / 3 条`（标题不含也命中 → 服务端正文搜索生效） |
| **标题关键词** `TITLEKW` | 命中 `['TITLEKW-标题示例']`；`筛选后 1 / 3 条` |
| 正文关键词 + 类型筛选 **文件** | `[]`；`筛选后 0 / 3 条`（**文件正文不参与**，契约一致） |
| 正文关键词 + 类型筛选 **文本** | 命中 `['第一章要点']`；`筛选后 1 / 3 条`（搜索与类型筛选并存） |
| 清空关键字 + 类型恢复全部 | 全量 3 行；`共 3 条` |

截图：`01-body-keyword-hit.png`、`02-title-keyword-hit.png`、`03-body-keyword-with-file-filter.png`、`04-cleared-full-list.png`

## 备注
- 防抖 200ms：输入过程中不逐字符发 IPC；清空立即恢复
- 「筛选后 N / M」的 M 由并行无 keyword 查询提供，语义与 #90 一致
- dev mock 的 `list({keyword})` 仍按标题过滤（mock 无正文），真实链路以上述服务端行为为准

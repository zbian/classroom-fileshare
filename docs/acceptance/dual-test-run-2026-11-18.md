# 双机测试执行记录（2026-11-18）

> 关联：#91；环境由 mesh 派发 + Syncthing 证据回流；不使用 GitHub Actions（全部本地执行）。
> 证据目录：`docs/acceptance/evidence/dual/`

## 1 环境

| 项 | 值 |
| --- | --- |
| A 机（教师） | `BIAN-小白机`，Syncthing device `PM4DR7D-LSCQBL3-TBPDLZB-GNBEM5D-V63M6A4-SNLMDPY-G57ONKL-BMZDZQR` |
| B 机（学生） | `Zhen-LPD`（192.168.1.14），Syncthing device `K3IMBH5-47F7WL2-DKZK255-X4KDQDE-6GCNF4G-I2LFFZO-WV56MKV-4T2L3AN` |
| 被测版本 | `v0.2.0-rc.1-7-gf57b51c`（classroom-svc.exe 5.3 MB / classroom-gui.exe 12.4 MB） |
| B 机安装位置 | `D:\ClassShareDual\`（syncthing + artifacts + evidence），C 盘不新增 |
| 通道 | mesh 派发任务（`TCP 45917`）+ Syncthing 直连（`TCP 22000`，TLS1.3）+ 证据目录自动回流 |

## 2 方法

- 教师侧：A 机本地 `scripts/dual/teacher-case.ps1` 起隔离教师 svc（独立 LOCALAPPDATA/APPDATA/PROGRAMDATA），日志与证据落临时目录。
- 学生侧：B 机通过 mesh 任务执行**内联脚本**（不依赖 .ps1 文件同步），起隔离学生 svc，断言日志关键词。
- 判定：双侧日志匹配，教师侧与 B 侧 session_id 必须一致。

## 3 结果

| # | 用例 | 结果 | 证据 |
| --- | --- | --- | --- |
| A1 | 真实双机发现并入班 | **PASS** | 教师日志 `学员加入 session_id=a5f3c581-4fa5-4b72-a16c-95637d998e15`；B 侧 `JOINED=True`、`session_id` 相同、`epoch=1`；`A1-teacher-20261118-215849.json` |
| A4 | EX-11 教师端口冲突自动换端口 | **PASS**（教师侧） | 先占用 47821 → 启动日志 `指定端口被占用，自动更换（EX-11）` → 实际端口 **3717**；`A4-teacher-20261118-220016.json` |
| A2 | 重名昵称拒绝 | **PASS** | S1 `session_id=740c5df5-...` 入班；S2 `加入被拒（致命，不重试） code=DupNickname`；教师侧 `旧连接存活，拒绝重名加入（EX-02）`；`A2-dual-20261118.json` |
| A3 | PIN 校验 | **PASS** | 错误 PIN：`加入被拒（致命，不重试） code=WrongPin`；正确 PIN：`session_id=a407c7e0-...`，教师侧同名 session_id 匹配；`A3-dual-20261118.json` |
| B1 | 跨机 10MB 文件分发 | **PASS** | 源/B 落盘 sha256 `E5B844CC...` 完全一致；item_id `07d01f64...` 双侧一致；session `be40298a...`；`B1-dual-20261118.json` |
| B2+C4 | 文本同步 + 删除同步 | ⏳ 执行中 | — |
| B4/C5/C7 | GUI 退出 / 离线补发 / 占用重试 | ⏳ 待执行 | — |

## 4 过程中发现与修复

1. **B 机 mesh 会话沙箱**：默认 `workspace-write`，写 `D:\ClassShareDual` 被拒 → 由 B 机操作者改为 `danger-full-access` 后正常。
2. **B 机执行 .ps1 报 `Cannot index into a null array`**：内联等效代码可正常跑通并成功入班；后续用例统一用内联命令下发，避免 B 端"修脚本"卡住。
3. **教师侧假阳性**：`Wait-SvcLog` 会命中上一轮残留日志 → 每次启动前清空 `svc.log`（已修入 `teacher-case.ps1`/`student-case.ps1`）。
4. **证据 JSON 膨胀（35–71 MB）**：`Get-Content` 返回对象带 `PSPath/PSDrive` NoteProperty，被 `ConvertTo-Json` 序列化 → 改为 `[string]$_` 后证据约 2.5 KB。
5. **B 端回执排队**：mesh 回执以会话消息排队到达，不即时；测试以证据文件/日志为准，不以轮询为准。
6. 用例结束即关闭教师 svc，学生端日志出现「与教师机断开，稍后自动重连」属预期。
7. **ACL 加固与受限沙箱**：B 机 mesh 会话（workspace-write）无法写 DACL，svc 启动即失败（`SetNamedSecurityInfoW WIN32_ERROR(5)`）→ 新增 console 降级模式软失败（服务模式仍 fail-closed，PR #150），数据可放会话工作区。
8. **同机多实例管道名冲突**：`\\.\pipe\classshare-svc-student` 固定，第二实例报「创建管道实例失败」（不影响入班拒绝判定）——候选 issue：按实例/用户隔离管道名。

## 5 复跑命令

```powershell
# A 机（教师）
.\scripts\dual\teacher-case.ps1 -Case A1 -Room XK-204 -WaitStudentSec 300

# B 机（经 mesh 内联执行；示例为学生 svc 入班）
#   D:\ClassShareDual\artifacts\bin\classroom-svc.exe --console
#   svc.json: {"version":1,"mode":"student","room_code":"XK-204","manual_addr":"192.168.1.7:47821"}
```

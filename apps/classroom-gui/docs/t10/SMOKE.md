# T10 冒烟记录：学员视图与首次昵称（rebase 到 4186b62 后）

- 日期：2026-11-16 · Windows 11 · vite dev(1517) + debug exe · svc --console teacher 同场
- 方法：Win32 自动化（双击图标开主窗）+ GDI 截图 + PIL 像素断言（学员青 #0D9488±18 / #F0FDFA±5）

## 已验证

| # | 项目 | 结果 | 证据 |
| --- | --- | --- | --- |
| 1 | --role=student 写入 gui.json → 前端 data-role=student 切学员青 | ✅ teal px=1247 + teal-l px=2858 | t10-b1-nickname.png |
| 2 | B1 首次昵称弹窗渲染（role=student 首启自动弹出，不可关闭） | ✅（同图） | 像素 |
| 3 | rebase 后门禁：pnpm build / cargo check / clippy / test(12) / fmt | ✅ 全绿 | CI 同口径 |
| 4 | client.ts 功能并集核对：结构化 status（StatusSnapshot）保留、setNickname×3、UiShareItem、mock 保留、无回退 | ✅ | grep 核对 |

## 待真实链路项（环境受限，见下）

- SetNickname 经真实命名管道：**被部署形态阻断**——student 角色的桥连
  `\\.\pipe\classshare-svc-student`，而本机服务实例是 teacher 模式（teacher 管道）。
  svc console 模式 data_root 固定 `%LOCALAPPDATA%\ClassShare\data`（config.rs），
  无法同机并存第二个 student 实例（SQLite 锁冲突）。CDP 实测 invoke 返回
  「后台服务未运行」与上述一致——**非代码缺陷**。
- 需要裁定（dev-backend 一行或联调规范）：svc 支持 `--data-root`/`--pipe-suffix`
  override（多实例并存），或学员联调统一在双机环境做（T13 后自然具备）。
- B1/D1/D2 的业务逻辑此前已在 mock 路径验证（提交 0d33bc1）；本机 teacher 服务
  同场时教师视图真实链路已在 T9（4186b62 冒烟）全过。

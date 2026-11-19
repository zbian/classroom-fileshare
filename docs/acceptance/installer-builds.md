# 安装包构建与发布（教师端 / 学生端）

## 产物

| 文件 | 说明 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | 教师端单文件安装器（IExpress 自解压） |
| `ClassShare-Student-Setup.exe` | 学生端单文件安装器 |
| `ClassShare-*-Setup.zip` | 同内容 ZIP（离线/兜底部署） |
| `README-teacher.txt` / `README-student.txt` | 终端用户安装说明 |

默认输出目录：`%LOCALAPPDATA%\ClassShareDual\dist`

## 构建

```powershell
.\scripts\package-installer.ps1            # 教师端 + 学生端
.\scripts\package-installer.ps1 -Mode teacher
.\scripts\package-installer.ps1 -Build     # 先重建前端 + svc + gui 再打包
```

依赖：Windows 自带 IExpress；不使用 GitHub Actions / 托管 CI。

## 安装行为

1. `setup-<mode>.ps1` 请求管理员权限（UAC）
2. 复制 `classroom-svc.exe` / `classroom-gui.exe` 到 `%ProgramFiles%\ClassShare`
3. 调用 `scripts/install.ps1 -Mode <mode>`：
   - 管理员：注册 `ClassShareSvc`（Session 0、开机自启）+ 防火墙规则（UDP 47820；教师端另加 TCP 47821）+ GUI 登录自启（`--role=<mode>`）
   - 非管理员：降级为当前用户登录自启（`--console`），功能受限
4. 卸载：`%ProgramFiles%\ClassShare\uninstall-<mode>.cmd`

## AI 安装提示词

`docs/install-prompts/teacher.md`、`docs/install-prompts/student.md`：整段复制给能执行命令的 AI
（DSH / Claude Code / Codex CLI / Cursor 等），AI 会下载、校验 SHA-256、安装、验证并指导连接。

## Gitee Release

- 标签：`v0.2.0-rc.2`
- 发布：`https://gitee.com/zbian/classroom-fileshare/releases/tag/v0.2.0-rc.2`
- 附件：4 个安装包/压缩包 + 2 个 README + 2 个 AI 提示词
- **仓库可见性**：Release 附件对未登录用户可下载要求仓库为公开；若为私有需先在
  Gitee 仓库「管理 → 仓库信息」切换公开。
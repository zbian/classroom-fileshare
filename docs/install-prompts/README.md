# 课堂共享 · AI 安装提示词

把对应角色的提示词**整段复制**给能执行命令的 AI（DSH、Claude Code、Codex CLI、Cursor Agent 等），
AI 会下载、校验、**静默安装**、验证并指导连接。

> 前提：Windows 10/11 x64；安装需要管理员权限（UAC 点一次「是」）；同局域网可自动发现。
> 若 AI 执行环境禁止提权（RunAs 被拦），提示词会让它改为指导你手动运行一次管理员命令。

## 文件

| 角色 | 提示词 | 安装包（ZIP 静默 / EXE 人工） |
| --- | --- | --- |
| 教师端 | [teacher.md](teacher.md) | `ClassShare-Teacher-Setup.zip` / `.exe` |
| 学生端 | [student.md](student.md) | `ClassShare-Student-Setup.zip` / `.exe` |

## 版本与校验值（v0.2.0-rc.5）

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | `908cd731fa2d491e389b0792693b16e6ae1b94e3b19ebd23a5aa717753b9d638` |
| `ClassShare-Teacher-Setup.zip` | `4efae880be22a43bd581c5cdbce3b9963a0b8e0a14d5220bddd2612c94adad5b` |
| `ClassShare-Student-Setup.exe` | `8194b752becf2e54c86a51fa878503e2386ed9c0f111e1a36db55700585e17a6` |
| `ClassShare-Student-Setup.zip` | `a773b5daae9840c1d11aba4c1f634a38786d03a4b67355ba83866febeb99094f` |

## 下载地址（Gitee Release；GitHub 备用）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.5/ClassShare-Teacher-Setup.zip
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.5/ClassShare-Student-Setup.zip
```

## 为什么 AI 用 ZIP + 静默脚本

- EXE 向导是**交互式**的（需要人点选、填写），AI 无法操作
- 静默脚本 `silent-install.ps1` 无任何交互，日志写 `%ProgramData%\ClassShareSetup.log`
- AI 提权被环境拦截时，用 `install-as-admin.cmd` 由你手动右键「以管理员身份运行」（只需一次）
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

## 版本与校验值（v0.2.0-rc.6）

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | `b6b5fbfc57ead712365362f25ec4a375b46e21eae3936a0f2ac1aae8f2dd30af` |
| `ClassShare-Teacher-Setup.zip` | `6ed8aeee316864a162f976bf679fe295ec2d042b73efbf3302cbf5e170ab9c58` |
| `ClassShare-Student-Setup.exe` | `40ec6d38ff883bbe0f40ba20869820e0fe6729417fcefa6301b6d583f17b00b6` |
| `ClassShare-Student-Setup.zip` | `4a2609540c5e7cffd31ddd304d7be44f527c1d56c4b6da85fbc20efe31f32e09` |

## 下载地址（Gitee Release；GitHub 备用）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.6/ClassShare-Teacher-Setup.zip
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.6/ClassShare-Student-Setup.zip
```

## 为什么 AI 用 ZIP + 静默脚本

- EXE 向导是**交互式**的（需要人点选、填写），AI 无法操作
- 静默脚本 `silent-install.ps1` 无任何交互，日志写 `%ProgramData%\ClassShareSetup.log`
- AI 提权被环境拦截时，用 `install-as-admin.cmd` 由你手动右键「以管理员身份运行」（只需一次）
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

## 版本与校验值（v0.2.0-rc.4）

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | `54a06acee5ca009590b946f761082aabe561dd1ca07fc4f1b686d900cf85f7ef` |
| `ClassShare-Teacher-Setup.zip` | `8cddb4722ce710f13a8762b369b2534cacf23607257ed0f303c1c350f83880e6` |
| `ClassShare-Student-Setup.exe` | `c7a41d703733497b7ea9ae30d29611e970951f83302b559a266081f95c1ceaf2` |
| `ClassShare-Student-Setup.zip` | `213c973c4fd1dc994f63bf4f4bdec998ff2af175717a61ae8a8da60a98018225` |

## 下载地址（Gitee Release；GitHub 备用）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.4/ClassShare-Teacher-Setup.zip
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.4/ClassShare-Student-Setup.zip
```

## 为什么 AI 用 ZIP + 静默脚本

- EXE 向导是**交互式**的（需要人点选、填写），AI 无法操作
- 静默脚本 `silent-install.ps1` 无任何交互，日志写 `%ProgramData%\ClassShareSetup.log`
- AI 提权被环境拦截时，用 `install-as-admin.cmd` 由你手动右键「以管理员身份运行」（只需一次）
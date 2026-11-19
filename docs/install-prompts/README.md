# 课堂共享 · AI 安装提示词

把对应角色的提示词**整段复制**给能执行命令的 AI（DSH、Claude Code、Codex CLI、Cursor Agent 等），
AI 会下载、校验、**静默安装**、验证并指导连接。

> 前提：Windows 10/11 x64；安装需要管理员权限（UAC 点一次「是」）；同局域网可自动发现。

## 文件

| 角色 | 提示词 | 安装包（ZIP 静默 / EXE 人工） |
| --- | --- | --- |
| 教师端 | [teacher.md](teacher.md) | `ClassShare-Teacher-Setup.zip` / `.exe` |
| 学生端 | [student.md](student.md) | `ClassShare-Student-Setup.zip` / `.exe` |

## 版本与校验值（v0.2.0-rc.3）

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | `0fca6b87eab51539dbbf11227d70a441c6764da00dbb035f4d9ead261b08d8fb` |
| `ClassShare-Teacher-Setup.zip` | `c3abc413eebee1eea0e91fb58b944929f62589cb3404bec17a3b8f26c73423f4` |
| `ClassShare-Student-Setup.exe` | `fbf84557fc02469ff1ef711e973e1023fdcff8b78fb241b1bc8f9eacbe9920e6` |
| `ClassShare-Student-Setup.zip` | `fee55ab88a373cef8e82d8364e1ef885bd02402c5d1a6ed5224567d4582a61ec` |

## 下载地址（Gitee Release，公开可用；GitHub 备用）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.3/ClassShare-Teacher-Setup.zip
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.3/ClassShare-Student-Setup.zip
```

> 备用（GitHub 镜像，公开）：把域名 `gitee.com` 换成 `github.com` 即可。
> 完整校验值见 Release 附件 `SHA256SUMS-v0.2.0-rc.3.txt`。

## 为什么 AI 用 ZIP + 静默脚本

- EXE 向导是**交互式**的（需要人点选、填写），AI 无法操作
- 静默脚本 `silent-install.ps1` 接受参数（`-Room` / `-Pin` / `-Nickname` / `-ManualAddr`），
  无任何交互；安装日志写到 `%ProgramData%\ClassShareSetup.log`
- ZIP 解压到 `%LOCALAPPDATA%\ClassShareSetup` 后执行，不依赖安装器临时目录

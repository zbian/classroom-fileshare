# 课堂共享 · AI 安装提示词

把对应角色的提示词**整段复制**给能执行命令的 AI（如 DSH、Claude Code、Codex CLI、Cursor Agent 等），
AI 会按步骤下载、校验、安装并验证。

> 前提：Windows 10/11 x64；安装需要管理员权限（UAC 点「是」）；与对方机器在同一局域网时可用自动发现。

## 文件

| 角色 | 提示词 | 安装包 |
| --- | --- | --- |
| 教师端 | [teacher.md](teacher.md) | `ClassShare-Teacher-Setup.exe` |
| 学生端 | [student.md](student.md) | `ClassShare-Student-Setup.exe` |

## 版本与校验值（v0.2.0-rc.2）

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Teacher-Setup.exe` | `96b3238ed1ffc7a3539fac435c14a1696f173fd07b9a19e51de06a42df352cdf` |
| `ClassShare-Student-Setup.exe` | `eb9673c1534cd9d44091cb04e459f3f06e0e38785d42ba9491542ce793b70d79` |

## 下载地址（Gitee Release）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.2/ClassShare-Teacher-Setup.exe
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.2/ClassShare-Student-Setup.exe
```

> 若仓库当前为私有，需先在 Gitee 仓库「管理 → 仓库信息」把可见性改为公开，下载链接才对未登录用户生效。

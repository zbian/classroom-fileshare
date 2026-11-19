# 课堂共享 · AI 安装提示词

把对应角色的提示词**整段复制**给能执行命令的 AI（DSH、Claude Code、Codex CLI、Cursor Agent 等），
AI 会下载、校验、**静默安装**、验证并指导连接。

> 前提：Windows 10/11 x64；安装需要管理员权限（UAC 点一次「是」）；同局域网可自动发现。

## 文件

| 角色 | 提示词 |
| --- | --- |
| 教师端 | [teacher.md](teacher.md) |
| 学生端 | [student.md](student.md) |

## 安装包（单文件 EXE，运行时无脚本）

`ClassShare-Setup.exe` —— NSIS 原生安装程序，内置教师端/学生端二选一、服务注册、防火墙、GUI 自启、卸载器。

| 文件 | SHA-256 |
| --- | --- |
| `ClassShare-Setup.exe`（v0.2.0-rc.7） | `8fe62b539c588737ebf9e604c1ca98e9e6e3accf8911ba3977bb9cd54e8dfcca` |

## 下载地址（Gitee Release；GitHub 备用）

```
https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.7/ClassShare-Setup.exe
https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.7/ClassShare-Setup.exe
```

## 静默参数

| 参数 | 说明 |
| --- | --- |
| `/S` | 静默安装（无界面） |
| `/ROLE=teacher\|student` | 角色 |
| `/ROOM=XK-204` | 房间码（教师端默认 XK-204；学生端留空=自动发现） |
| `/PIN=1234` | 房间 PIN（可选） |
| `/NICK=张三` | 学生昵称（可选，支持中文） |
| `/ADDR=192.168.1.7:47821` | 教师机地址（学生端可选；跨网段必填） |

示例：

```powershell
# 教师端
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=teacher','/ROOM=XK-204' -Verb RunAs -Wait
# 学生端
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=student','/NICK=张三','/ADDR=192.168.1.7:47821' -Verb RunAs -Wait
# 静默卸载
Start-Process "$env:ProgramData\ClassShare\bin\uninstall.exe" -ArgumentList '/S' -Verb RunAs -Wait
```

> 交互安装：直接双击 `ClassShare-Setup.exe`，在界面里选角色、填参数即可（同样不需要脚本）。

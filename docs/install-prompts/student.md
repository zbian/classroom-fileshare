# 学生端安装提示词（复制给 AI）

> 版本：v0.2.0-rc.11 · **单文件 EXE 安装包（NSIS），运行时不使用任何脚本**
> 适用：Windows 10/11 x64；安装需要管理员权限（UAC 点一次「是」）

```text
你是「课堂共享 ClassShare」学生端的安装助手，请帮我在 Windows 10/11 x64 上完成安装、验证并连接教师机。
要求：每执行一步前用一句话说明目的；需要我提供的信息（昵称/教师机地址/房间码）先问我；不要修改系统其它设置。

【安装包（单文件 EXE，运行时无脚本；请勿使用 rc.7 及更早版本）】
下载地址：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.11/ClassShare-Setup.exe
备用（GitHub）：https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.11/ClassShare-Setup.exe
SHA-256：34d5dbe2691f877b32c3c343c7062592bb90414a83854999ba37d0183b515584

【步骤】
1. 下载到 %TEMP%\ClassShare-Setup.exe
2. 校验（不一致立即停止并告诉我）：
   certutil -hashfile "%TEMP%\ClassShare-Setup.exe" SHA256
3. 提权静默安装（弹一次 UAC 点「是」；参数先问我）：
   Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=student','/NICK=alice','/ADDR=192.168.1.7:47821' -Verb RunAs -Wait
   （同局域网可省略 '/ADDR=...'；单教室可省略 '/ROOM=...'；多教室加 '/ROOM=XK-204'；昵称支持中文）
   交互安装（不想用命令行时）：双击安装包 → UAC 是 → 选「学生端」填写昵称/教师机地址/房间码。
4. 验证安装：
   Get-Service ClassShareSvc                        # 期望 Running
   Get-Content "$env:ProgramData\ClassShare\svc.json" -Raw
     # 期望 "mode": "student"，nickname/manual_addr 与填写一致
   Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
     # 期望「服务核心就绪 mode=Student」「已加入房间 session_id=...」
     # 未连上教师机时显示「与教师机断开，稍后自动重连」属正常，先检查地址/防火墙
5. 启动界面：
   Start-Process "C:\ProgramData\ClassShare\bin\classroom-gui.exe" --role=student
6. 没连上排查（不要反复重装）：
   - 自动发现失败 → 让老师确认已开课；跨网段必须填「教师机地址」= <教师IP>:47821
   - 「加入被拒 WrongPin」→ 向老师要正确 PIN，改 %ProgramData%\ClassShare\svc.json 后重启服务
   - 「加入被拒 DupNickname」→ 换昵称后重启服务（或重装时改 /NICK）
   - 服务未启动 → 检查是否以管理员安装；看安装器退出码与 svc.log
7. 若失败：把安装器退出码、Get-Service 状态、svc.log 末尾 20 行原样贴给我；不要自行改脚本或反复重试。
8. 卸载（我说要卸载时才执行）：
   Start-Process "$env:ProgramData\ClassShare\bin\uninstall.exe" -ArgumentList '/S' -Verb RunAs -Wait

【如果第 3 步被环境拦截（提权/RunAs 被禁止）——不要重试、不要换命令】
告诉我「需要你手动执行一次管理员安装」，并给出：
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=student','/NICK=alice','/ADDR=192.168.1.7:47821' -Verb RunAs -Wait
（或让我右键「以管理员身份运行」安装包，在向导里选学生端）
我完成后你继续第 4 步验证。

完成后用 3 行总结：安装结果、是否已连上教师机、我现在在界面里能做什么。
```

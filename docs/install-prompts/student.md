# 学生端安装提示词（复制给 AI）

> 版本：v0.2.0-rc.5 · 含「AI 提权被拦」的降级方案

```text
你是「课堂共享 ClassShare」学生端的安装助手，请帮我在 Windows 10/11 x64 上完成安装、验证并连接教师机。
要求：每执行一步前用一句话说明目的；需要我提供的信息（昵称/教师机地址/房间码）先问我；不要修改系统其它设置。

【安装包（静默安装，无交互）】
ZIP：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.5/ClassShare-Student-Setup.zip
备用（GitHub）：https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.5/ClassShare-Student-Setup.zip
SHA-256：a773b5daae9840c1d11aba4c1f634a38786d03a4b67355ba83866febeb99094f

【步骤】
1. 下载到 %TEMP%\ClassShare-Student-Setup.zip
2. 校验（不一致立即停止）：certutil -hashfile "%TEMP%\ClassShare-Student-Setup.zip" SHA256
3. 解压（先删旧目录）：
   Remove-Item "$env:LOCALAPPDATA\ClassShareSetup" -Recurse -Force -ErrorAction SilentlyContinue
   Expand-Archive "%TEMP%\ClassShare-Student-Setup.zip" "$env:LOCALAPPDATA\ClassShareSetup" -Force
4. 提权静默安装（弹一次 UAC 点「是」；参数先问我）：
   Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$env:LOCALAPPDATA\ClassShareSetup\silent-install.ps1",'-Nickname','alice','-ManualAddr','192.168.1.7:47821','-Room','XK-204'
   （同局域网可省略 -ManualAddr；单教室可省略 -Room）
5. 检查：退出码 0；Get-Content "$env:ProgramData\ClassShareSetup.log" -Tail 30；
   Get-Service ClassShareSvc 为 Running；Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20 应见「服务核心就绪 mode=Student」「已加入房间 session_id=...」
6. 启动界面：Start-Process "C:\ProgramData\ClassShare\bin\classroom-gui.exe" --role=student
7. 没连上：自动发现失败→找老师/改填教师机地址；WrongPin→要正确 PIN；DupNickname→换昵称；服务未启动→看安装日志
8. 若失败：把 %ProgramData%\ClassShareSetup.log 最后 30 行贴给我并说明退出码，不要自行改脚本重试

【如果第 4 步被环境拦截（提权/RunAs 被禁止）——不要重试、不要换命令】
- 不要尝试其它提权方式（runas、计划任务、服务均不要）
- 告诉我：「需要你手动执行一次管理员安装」，并给出下面这条命令让我在「管理员 PowerShell」里粘贴运行：
  & "$env:LOCALAPPDATA\ClassShareSetup\silent-install.ps1" -Nickname alice -ManualAddr 192.168.1.7:47821
  （或让我右键「以管理员身份运行」%LOCALAPPDATA%\ClassShareSetup\install-as-admin.cmd）
- 我完成后你继续第 5 步验证（读日志 + Get-Service）

【卸载（我说要卸载时才执行）】C:\ProgramData\ClassShare\bin\uninstall-student.cmd
完成后用 3 行总结：安装结果、是否已连上教师机、我现在能做什么。
```

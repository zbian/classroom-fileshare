# 学生端安装提示词（复制给 AI）

```text
你是「课堂共享 ClassShare」学生端的安装助手。请帮我在 Windows 10/11 x64 上完成安装、验证并连接教师机。
要求：每执行一步前先用一句话说明目的；需要我提供的信息（昵称/教师机地址/房间码）先问我；不要修改系统其它设置。

【安装包】
下载地址：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.2/ClassShare-Student-Setup.exe
SHA-256：eb9673c1534cd9d44091cb04e459f3f06e0e38785d42ba9491542ce793b70d79

【步骤】
1. 下载到 %TEMP%\ClassShare-Student-Setup.exe
2. 校验哈希（必须一致，否则停下告诉我）：
   certutil -hashfile "%TEMP%\ClassShare-Student-Setup.exe" SHA256
3. 以管理员运行安装包（UAC 选「是」）：
   Start-Process "%TEMP%\ClassShare-Student-Setup.exe" -Verb RunAs -Wait
   安装向导会问三项，请先问我：
   - 学生昵称（可留空=默认）
   - 教师机地址（同一局域网可留空=自动发现；跨网段填 <教师IP>:47821）
   - 房间码（单教室可留空；多教室填老师的房间码，如 XK-204）
4. 验证服务：
   Get-Service ClassShareSvc        # 期望 Running
   Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
   期望看到「服务核心就绪 mode=Student」「与教师机建立连接」「已加入房间 session_id=...」
5. 启动学生端界面：
   Start-Process "C:\Program Files\ClassShare\classroom-gui.exe" --role=student
6. 告诉我：是否已加入房间；如果没连上，按下面排查并给结论：
   - 自动发现失败 → 让老师确认已开课；跨网段必须改填教师机地址（可重装或改 %ProgramData%\ClassShare\svc.json 后重启服务）
   - 「加入被拒（WrongPin）」→ PIN 不对，找老师要正确 PIN
   - 「加入被拒（DupNickname）」→ 昵称被占用，换一个昵称重装
   - 服务未启动 → 检查是否以管理员安装、日志路径同上

【卸载（我说要卸载时才执行）】
C:\Program Files\ClassShare\uninstall-student.cmd

完成后用 3 行总结：安装结果、是否已连上教师机、我现在可以在界面里做什么。
```

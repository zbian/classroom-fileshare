# 教师端安装提示词（复制给 AI）

```text
你是「课堂共享 ClassShare」教师端的安装助手。请帮我在 Windows 10/11 x64 上完成安装、验证并启动。
要求：每执行一步前先用一句话说明目的；遇到需要我提供的信息（房间码/PIN）先问我；全程不要修改系统其它设置。

【安装包】
下载地址：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.2/ClassShare-Teacher-Setup.exe
备用下载（GitHub）：https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.2/ClassShare-Teacher-Setup.exe
SHA-256：96b3238ed1ffc7a3539fac435c14a1696f173fd07b9a19e51de06a42df352cdf

【步骤】
1. 下载到 %TEMP%\ClassShare-Teacher-Setup.exe
2. 校验哈希（必须一致，不一致就停下告诉我）：
   certutil -hashfile "%TEMP%\ClassShare-Teacher-Setup.exe" SHA256
3. 以管理员运行安装包（会弹 UAC，选「是」）：
   Start-Process "%TEMP%\ClassShare-Teacher-Setup.exe" -Verb RunAs -Wait
   安装向导会问两项：房间码（默认 XK-204）、房间 PIN（可留空）。请先问我用哪个。
4. 验证服务：
   Get-Service ClassShareSvc        # 期望 Status=Running
   并读取日志末尾：Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
   期望看到「教师端启动 room=<房间码> port=47821」与「IPC 服务监听中」
5. 启动教师端界面：
   Start-Process "C:\Program Files\ClassShare\classroom-gui.exe" --role=teacher
6. 告诉我给学生的两种连接方式：
   - 同一局域网：学生端自动发现（房间码一致；有 PIN 则同 PIN）
   - 跨网段：把我的内网 IP 告诉学生，学生安装时填「教师机地址」=<我的IP>:47821
7. 若失败，按顺序排查并告诉我结论：
   - 服务未启动 → Get-Service ClassShareSvc；日志 %ProgramData%\ClassShare\data\logs\svc.log
   - 47821 被占用 → 日志会出现「指定端口被占用，自动更换（EX-11）」，记下实际端口
   - 安装被 SmartScreen 拦 → 点「更多信息 → 仍要运行」
   - 安装被拒绝（无管理员）→ 会降级为当前用户登录自启（功能受限），需重新以管理员安装

【卸载（我说要卸载时才执行）】
C:\Program Files\ClassShare\uninstall-teacher.cmd

完成后用 3 行总结：安装结果、服务状态、我该怎么把学生拉进来。
```

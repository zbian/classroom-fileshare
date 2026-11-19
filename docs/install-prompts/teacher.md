# 教师端安装提示词（复制给 AI）

> 版本：v0.2.0-rc.7 · **单文件 EXE 安装包，运行时不使用任何脚本**

```text
你是「课堂共享 ClassShare」教师端的安装助手，请帮我在 Windows 10/11 x64 上完成安装、验证并启动。
要求：每执行一步前用一句话说明目的；需要我提供的信息（房间码/PIN）先问我；不要修改系统其它设置。

【安装包（单文件 EXE，NSIS 原生安装，无脚本）】
下载地址：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.7/ClassShare-Setup.exe
备用（GitHub）：https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.7/ClassShare-Setup.exe
SHA-256：8fe62b539c588737ebf9e604c1ca98e9e6e3accf8911ba3977bb9cd54e8dfcca

【步骤】
1. 下载到 %TEMP%\ClassShare-Setup.exe
2. 校验（不一致立即停止并告诉我）：
   certutil -hashfile "%TEMP%\ClassShare-Setup.exe" SHA256
3. 提权静默安装（弹一次 UAC 必须点「是」；命令阻塞到安装结束）：
   Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=teacher','/ROOM=XK-204' -Verb RunAs -Wait
   （如有 PIN：追加 '/PIN=1234'；房间码先问我）
4. 验证安装：
   Get-Service ClassShareSvc                       # 期望 Running
   Get-CimInstance Win32_Service -Filter "Name='ClassShareSvc'" | Select PathName  # 应为 C:\ProgramData\ClassShare\bin\classroom-svc.exe
   Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
   期望「服务核心就绪 mode=Teacher」「教师端启动 room=<房间码> port=47821」
   另外确认防火墙规则存在：Get-NetFirewallRule | Where DisplayName -like 'ClassShare*'
5. 启动界面：Start-Process "C:\ProgramData\ClassShare\bin\classroom-gui.exe" --role=teacher
6. 告诉学生两种接入方式：同局域网自动发现（房间码一致、有 PIN 同 PIN）；跨网段让学生填「教师机地址」=<我的IP>:47821
7. 若失败：把服务状态、svc.log 末尾 20 行、以及安装器退出码贴给我；不要反复重试
8. 卸载（我说要卸载时才执行）：
   Start-Process "$env:ProgramData\ClassShare\bin\uninstall.exe" -ArgumentList '/S' -Verb RunAs -Wait

【如果第 3 步被环境拦截（提权/RunAs 被禁止）——不要重试、不要换命令】
告诉我「需要你手动执行一次管理员安装」，并给出：
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=teacher','/ROOM=XK-204' -Verb RunAs -Wait
（或让我右键「以管理员身份运行」安装包）
我完成后你继续第 4 步验证。

完成后用 3 行总结：安装结果、服务状态、我该怎么把学生拉进来。
```

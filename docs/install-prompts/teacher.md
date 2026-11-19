# 教师端安装提示词（复制给 AI）

> 版本：v0.2.0-rc.3 · 推荐「静默安装」路径（无交互，AI 可直接执行）

```text
你是「课堂共享 ClassShare」教师端的安装助手，请帮我在 Windows 10/11 x64 上完成安装、验证并启动。
要求：每执行一步前用一句话说明目的；需要我提供的信息（房间码/PIN）先问我；不要修改系统其它设置。

【方式 A · 静默安装（推荐，无交互）】
安装包（ZIP）：https://gitee.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.3/ClassShare-Teacher-Setup.zip
备用（GitHub）：https://github.com/zbian/classroom-fileshare/releases/download/v0.2.0-rc.3/ClassShare-Teacher-Setup.zip
SHA-256：c3abc413eebee1eea0e91fb58b944929f62589cb3404bec17a3b8f26c73423f4

1. 下载到 %TEMP%\ClassShare-Teacher-Setup.zip
2. 校验（不一致立即停止并告诉我）：
   certutil -hashfile "%TEMP%\ClassShare-Teacher-Setup.zip" SHA256
3. 解压到 %LOCALAPPDATA%\ClassShareSetup（若已存在先删掉旧目录）：
   Expand-Archive "%TEMP%\ClassShare-Teacher-Setup.zip" "$env:LOCALAPPDATA\ClassShareSetup" -Force
4. 提权执行静默安装（会弹一次 UAC，必须点「是」；命令会阻塞到安装结束并返回退出码）：
   Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File',"$env:LOCALAPPDATA\ClassShareSetup\silent-install.ps1",'-Room','XK-204'
   （如需 PIN：再追加 '-Pin','1234'；房间码先问我）
5. 检查结果：
   - 退出码应为 0
   - 安装日志：Get-Content "$env:ProgramData\ClassShareSetup.log" -Tail 30
   - 服务：Get-Service ClassShareSvc（应为 Running）
   - 服务日志：Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
     期望「教师端启动 room=<房间码> port=47821」「IPC 服务监听中」
6. 启动界面：Start-Process "C:\Program Files\ClassShare\classroom-gui.exe" --role=teacher
7. 告诉我学生两种接入方式：同局域网自动发现（房间码一致、有 PIN 同 PIN）；跨网段让学生填「教师机地址」=<我的IP>:47821
8. 若失败：把 %ProgramData%\ClassShareSetup.log 最后 30 行原样贴给我，并说明退出码——不要自行改脚本重试

【方式 B · 人工双击（我只在你要求时用）】
安装包：.../ClassShare-Teacher-Setup.exe（SHA-256 0fca6b87eab51539dbbf11227d70a441c6764da00dbb035f4d9ead261b08d8fb）
双击 → UAC 是 → 向导里填房间码/PIN → 完成

【卸载（我说要卸载时才执行）】
C:\Program Files\ClassShare\uninstall-teacher.cmd

完成后用 3 行总结：安装结果、服务状态、我该怎么把学生拉进来。
```

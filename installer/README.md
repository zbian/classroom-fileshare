# ClassShare 安装包（NSIS，单文件 EXE，运行时无脚本）

## 产物

`ClassShare-Setup.exe` —— 一个 EXE 完成全部安装（原生 NSIS 界面）：

- 选择角色：教师端 / 学生端
- 参数：房间码、PIN、学生昵称、教师机地址
- 内置 `classroom-svc.exe` + `classroom-gui.exe` + `classshare-config.exe`（配置写入器，UTF-8，支持中文昵称）
- 安装到 `%ProgramData%\ClassShare\bin`（无空格路径）
- 注册服务 `ClassShareSvc`（Session 0、开机自启、失败自动重启）、防火墙规则、GUI 登录自启（带 `--role`）
- 自带 `uninstall.exe`，并在「设置 → 应用」注册卸载项

**安装/卸载全程不调用 PowerShell/cmd 脚本。**

## 交互安装

双击 `ClassShare-Setup.exe` → UAC 选「是」→ 选择角色并填参数 → 完成。

## 静默安装（AI/批量）

```powershell
# 教师端
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=teacher','/ROOM=XK-204' -Verb RunAs -Wait

# 教师端 + PIN
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=teacher','/ROOM=XK-204','/PIN=1234' -Verb RunAs -Wait

# 学生端（同局域网可省略 /ADDR；单教室可省略 /ROOM）
Start-Process "$env:TEMP\ClassShare-Setup.exe" -ArgumentList '/S','/ROLE=student','/NICK=张三','/ADDR=192.168.1.7:47821' -Verb RunAs -Wait

# 静默卸载
Start-Process "$env:ProgramData\ClassShare\bin\uninstall.exe" -ArgumentList '/S' -Verb RunAs -Wait
```

参数：`/S` 静默；`/ROLE=teacher|student`；`/ROOM=`；`/PIN=`；`/NICK=`；`/ADDR=`。
静默安装默认值：教师端房间 `XK-204`；学生端房间为空（自动发现首个教室）。

## 验证

```powershell
Get-Service ClassShareSvc                      # Running
Get-Content "$env:ProgramData\ClassShare\data\logs\svc.log" -Tail 20
# 期望：服务核心就绪 mode=Teacher/Student；教师端「教师端启动 room=... port=47821」
```

## 构建

```powershell
winget install NSIS.NSIS        # 一次性
.\installer\build.ps1 -Build    # 构建前端+三个 exe，再打包
```

脚本 `.nsi`：`installer/classshare.nsi`；配置写入器：`apps/classshare-config`。

## 注意

- 安装器未签名：SmartScreen →「更多信息 → 仍要运行」
- 提权 UAC 必须由人点「是」（Windows 限制）
- 卸载保留 `%ProgramData%\ClassShare\data`（共享数据），需要时手动删除

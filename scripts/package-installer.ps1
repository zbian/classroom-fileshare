#Requires -Version 5.1
<#
.SYNOPSIS
  教师端 / 学生端安装包打包脚本（交互式 Setup.exe + AI 静默安装）。
.DESCRIPTION
  产物（<OutDir>）：
    ClassShare-Teacher-Setup.exe / .zip
    ClassShare-Student-Setup.exe / .zip
    README-teacher.txt / README-student.txt
  ZIP 内额外包含 silent-install.ps1：供 AI/无人值守一条命令静默安装（无交互、写日志）。
  交互式 EXE：解压到 %LOCALAPPDATA%\ClassShareSetup 后再运行向导（避开 IExpress 临时目录被删）。
  前提：target\release\classroom-svc.exe 与 classroom-gui.exe 已构建（-Build 可代建）。
.PARAMETER Mode
  teacher / student / both（默认 both）
.PARAMETER OutDir
  输出目录（默认 %LOCALAPPDATA%\ClassShareDual\dist）
.PARAMETER Build
  打包前重新构建前端与两个 release 二进制
#>
[CmdletBinding()]
param(
    [ValidateSet('teacher', 'student', 'both')][string]$Mode = 'both',
    [string]$OutDir = '',
    [switch]$Build
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (-not $OutDir) { $OutDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual\dist' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
$utf8bom = New-Object System.Text.UTF8Encoding($true)

if ($Build) {
    Write-Host '[build] 前端 ...'
    & pnpm --dir (Join-Path $repo 'apps\classroom-gui\ui') build | Out-Null
    Write-Host '[build] classroom-svc ...'
    & cargo build --release -p classroom-svc --manifest-path (Join-Path $repo 'Cargo.toml') | Out-Null
    Write-Host '[build] classroom-gui ...'
    & cargo build --release -p classroom-gui --features custom-protocol --manifest-path (Join-Path $repo 'Cargo.toml') | Out-Null
}

$svcExe = Join-Path $repo 'target\release\classroom-svc.exe'
$guiExe = Join-Path $repo 'target\release\classroom-gui.exe'
foreach ($p in $svcExe, $guiExe) { if (-not (Test-Path $p)) { throw "缺少二进制：$p（先构建或加 -Build）" } }

function Build-Installer([string]$m) {
    $isTeacher = ($m -eq 'teacher')
    $label = if ($isTeacher) { '教师端' } else { '学生端' }
    $cap = $m.Substring(0, 1).ToUpper() + $m.Substring(1)
    $stage = Join-Path $env:TEMP "cs-$m-setup"
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item $svcExe, $guiExe $stage -Force
    Copy-Item (Join-Path $repo 'scripts\install.ps1') $stage -Force
    Copy-Item (Join-Path $repo 'scripts\uninstall.ps1') $stage -Force

    $askBlock = if ($isTeacher) {
@'
$room = Read-Host '房间码（回车=默认 XK-204）'
if (-not $room) { $room = 'XK-204' }
$pin = Read-Host '房间 PIN（回车=不设 PIN）'
'@
    } else {
@'
$nick = Read-Host '学生昵称（回车=默认）'
$addr = Read-Host '教师机地址（回车=自动发现；示例 192.168.1.7:47821）'
$room = Read-Host '房间码（回车=自动发现首个教室；多教室请填，如 XK-204）'
'@
    }
    $paramBlock = if ($isTeacher) {
@'
$params = @{
    Mode   = 'teacher'
    Room   = $room
    SvcExe = (Join-Path $installDir 'classroom-svc.exe')
    GuiExe = (Join-Path $installDir 'classroom-gui.exe')
}
if ($pin) { $params.Pin = $pin }
'@
    } else {
@'
$params = @{
    Mode   = 'student'
    SvcExe = (Join-Path $installDir 'classroom-svc.exe')
    GuiExe = (Join-Path $installDir 'classroom-gui.exe')
}
if ($nick) { $params.Nickname = $nick }
if ($addr) { $params.ManualAddr = $addr }
if ($room) { $params.Room = $room }
'@
    }
    $hintBlock = if ($isTeacher) {
@'
Write-Host '提示：房间码需与学员端一致；PIN 可选（设置后学员需输入）。'
'@
    } else {
@'
Write-Host '提示：同一局域网可留空自动发现；跨网段请填教师机地址。'
'@
    }

    $setupPs1 = @"
#Requires -Version 5.1
<#
  课堂共享 · $label 安装向导（交互式，供人工双击使用）。
  由 install-$m.cmd 从 %LOCALAPPDATA%\ClassShareSetup 启动（避免 IExpress 临时目录被删）。
#>
[CmdletBinding()]
param([switch]`$Elevated)
`$ErrorActionPreference = 'Stop'
`$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not `$isAdmin) {
    Write-Host '需要管理员权限来安装 Windows 服务，正在请求提权（UAC）...'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + `$PSCommandPath + '"'), '-Elevated'
    ) | Out-Null
    exit
}
`$here = Split-Path -Parent `$PSCommandPath
Write-Host '=========================================='
Write-Host '   课堂共享 · $label 安装程序'
Write-Host '=========================================='
Write-Host ''
Write-Host '提示：以下均可直接回车跳过。'
$askBlock
`$installDir = Join-Path `$env:ProgramData 'ClassShare\bin'
New-Item -ItemType Directory -Force -Path `$installDir | Out-Null
Copy-Item (Join-Path `$here 'classroom-svc.exe') (Join-Path `$installDir 'classroom-svc.exe') -Force
Copy-Item (Join-Path `$here 'classroom-gui.exe') (Join-Path `$installDir 'classroom-gui.exe') -Force
Copy-Item (Join-Path `$here 'install.ps1') (Join-Path `$installDir 'install.ps1') -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path `$here 'uninstall-$m.ps1') (Join-Path `$installDir 'uninstall-$m.ps1') -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path `$here 'uninstall-$m.cmd') (Join-Path `$installDir 'uninstall-$m.cmd') -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path `$here 'uninstall.ps1') (Join-Path `$installDir 'uninstall.ps1') -Force -ErrorAction SilentlyContinue
Write-Host "程序已复制到: `$installDir"
$paramBlock
& (Join-Path `$here 'install.ps1') @params
$hintBlock
Write-Host ''
Write-Host '=== 安装完成 ==='
Write-Host "程序目录: `$installDir"
Write-Host 'GUI 将在下次登录时自动启动（也可手动运行 classroom-gui.exe --role=$m）。'
Write-Host '卸载：运行 C:\ProgramData\ClassShare\bin\uninstall-$m.cmd'
Read-Host '按回车关闭'
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "setup-$m.ps1"), $setupPs1, $utf8bom)

    $uninstallPs1 = @"
#Requires -Version 5.1
[CmdletBinding()]
param([switch]`$Elevated)
`$ErrorActionPreference = 'Stop'
`$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not `$isAdmin) {
    Write-Host '卸载需要管理员权限，正在请求提权（UAC）...'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + `$PSCommandPath + '"'), '-Elevated'
    ) | Out-Null
    exit
}
`$here = Split-Path -Parent `$PSCommandPath
Write-Host '=== 课堂共享 · $label 卸载 ==='
& (Join-Path `$here 'uninstall.ps1')
`$installDir = Join-Path `$env:ProgramData 'ClassShare\bin'
if (Test-Path -LiteralPath `$installDir) {
    Remove-Item -LiteralPath `$installDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "已删除程序目录: `$installDir"
}
Write-Host '卸载完成（共享数据默认保留；如需清除请手动删除 %ProgramData%\ClassShare 与 %LOCALAPPDATA%\ClassShare）。'
Read-Host '按回车关闭'
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "uninstall-$m.ps1"), $uninstallPs1, $utf8bom)

    # 静默安装：AI/无人值守用，一条命令、无交互、写日志
    $silentDefaultRoom = if ($isTeacher) { "'XK-204'" } else { "''" }
    $silentPs1 = @"
#Requires -Version 5.1
<#
  课堂共享 · $label 静默安装（AI/无人值守）。
  用法（以管理员运行）：
    powershell -NoProfile -ExecutionPolicy Bypass -File silent-install.ps1 $(if ($isTeacher) { "-Room 'XK-204' [-Pin '1234']" } else { "[-Nickname 'alice'] [-ManualAddr '192.168.1.7:47821'] [-Room 'XK-204']" })
  日志：%ProgramData%\ClassShareSetup.log
#>
[CmdletBinding()]
param(
    [string]`$Room = $silentDefaultRoom,
    [string]`$Pin = '',
    [string]`$Nickname = '',
    [string]`$ManualAddr = ''
)
`$ErrorActionPreference = 'Stop'
`$log = Join-Path `$env:ProgramData 'ClassShareSetup.log'
function Write-SetupLog([string]`$Message) {
    `$line = "{0} {1}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss'), `$Message
    `$line | Tee-Object -FilePath `$log -Append
}
`$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not `$isAdmin) {
    Write-SetupLog '[FAIL] 需要管理员权限：请用管理员 PowerShell 运行本脚本'
    exit 2
}
try {
    `$here = Split-Path -Parent `$PSCommandPath
    `$installDir = Join-Path `$env:ProgramData 'ClassShare\bin'
    New-Item -ItemType Directory -Force -Path `$installDir | Out-Null
    Copy-Item (Join-Path `$here 'classroom-svc.exe') (Join-Path `$installDir 'classroom-svc.exe') -Force
    Copy-Item (Join-Path `$here 'classroom-gui.exe') (Join-Path `$installDir 'classroom-gui.exe') -Force
    Copy-Item (Join-Path `$here 'install.ps1') `$installDir -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path `$here 'uninstall-$m.ps1') `$installDir -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path `$here 'uninstall-$m.cmd') `$installDir -Force -ErrorAction SilentlyContinue
    Copy-Item (Join-Path `$here 'uninstall.ps1') `$installDir -Force -ErrorAction SilentlyContinue
    `$params = @{
        Mode   = '$m'
        SvcExe = (Join-Path `$installDir 'classroom-svc.exe')
        GuiExe = (Join-Path `$installDir 'classroom-gui.exe')
    }
    if (`$Room) { `$params.Room = `$Room }
    if (`$Pin) { `$params.Pin = `$Pin }
    if (`$Nickname) { `$params.Nickname = `$Nickname }
    if (`$ManualAddr) { `$params.ManualAddr = `$ManualAddr }
    Write-SetupLog ("[start] Mode=$m " + (`$params | ConvertTo-Json -Compress))
    & (Join-Path `$here 'install.ps1') @params 2>&1 | ForEach-Object { Write-SetupLog ([string]`$_) }
    Write-SetupLog '[ok] 安装完成'
    exit 0
} catch {
    Write-SetupLog ('[FAIL] ' + `$_.Exception.Message)
    exit 1
}
"@
    [System.IO.File]::WriteAllText((Join-Path $stage 'silent-install.ps1'), $silentPs1, $utf8bom)

    # 用户手动提权入口（AI 执行环境禁止 RunAs 时，由用户右键「以管理员身份运行」本 cmd）
    $adminPs1 = @"
#Requires -Version 5.1
<#
  以管理员身份运行同目录的 silent-install.ps1。
  用途：AI/自动化环境禁止自行提权时，由用户手动右键「以管理员身份运行」本脚本（或 install-as-admin.cmd）。
  参数原样透传给 silent-install.ps1（-Room/-Pin/-Nickname/-ManualAddr）。
#>
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments = `$true)][string[]]`$Forward = @())
`$ErrorActionPreference = 'Stop'
`$script = Join-Path `$PSScriptRoot 'silent-install.ps1'
if (-not (Test-Path `$script)) { throw '缺少 silent-install.ps1（请确认已完整解压 ZIP）' }
`$argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', `$script) + `$Forward
Write-Host ("将要执行的参数: " + (`$Forward -join ' '))
`$proc = Start-Process powershell -Verb RunAs -Wait -PassThru -ArgumentList `$argList
Write-Host ("安装进程退出码: " + `$proc.ExitCode)
if (Test-Path (Join-Path `$env:ProgramData 'ClassShareSetup.log')) {
    Write-Host '--- 安装日志（末 20 行） ---'
    Get-Content (Join-Path `$env:ProgramData 'ClassShareSetup.log') -Tail 20
}
Read-Host '按回车关闭'
"@
    [System.IO.File]::WriteAllText((Join-Path $stage 'install-as-admin.ps1'), $adminPs1, $utf8bom)
    $adminCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0install-as-admin.ps1`" %*`r`n"
    [System.IO.File]::WriteAllText((Join-Path $stage 'install-as-admin.cmd'), $adminCmd, (New-Object System.Text.UTF8Encoding($false)))

    $installCmd = @"
@echo off
chcp 65001 >nul
rem 先复制到持久目录，避免 IExpress 解压目录在提权期间被删除
set "DEST=%LOCALAPPDATA%\ClassShareSetup"
if not exist "%DEST%" mkdir "%DEST%" >nul 2>&1
xcopy "%~dp0*" "%DEST%\" /E /I /Y >nul 2>&1
if exist "%DEST%\setup-$m.ps1" (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%DEST%\setup-$m.ps1" %*
) else (
  powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0setup-$m.ps1" %*
)
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "install-$m.cmd"), $installCmd + "`r`n", (New-Object System.Text.UTF8Encoding($false)))
    $uninstCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0uninstall-$m.ps1`"`r`n"
    [System.IO.File]::WriteAllText((Join-Path $stage "uninstall-$m.cmd"), $uninstCmd, (New-Object System.Text.UTF8Encoding($false)))

    $roleDesc = if ($isTeacher) { '教师端（建课、广播文件/文本、接收学员提交）' } else { '学生端（自动发现/入班、接收共享、提交作业）' }
    $silentExample = if ($isTeacher) {
        'Start-Process powershell -Verb RunAs -Wait -ArgumentList ''-NoProfile'',''-ExecutionPolicy'',''Bypass'',''-File'',''"%LOCALAPPDATA%\ClassShareSetup\silent-install.ps1"'',''-Room'',''XK-204'''
    } else {
        'Start-Process powershell -Verb RunAs -Wait -ArgumentList ''-NoProfile'',''-ExecutionPolicy'',''Bypass'',''-File'',''"%LOCALAPPDATA%\ClassShareSetup\silent-install.ps1"'',''-ManualAddr'',''<教师IP>:47821'',''-Nickname'',''alice'''
    }
    $readme = @"
课堂共享 · $label 安装包
==========================

适用：Windows 10 / 11 x64
角色：$roleDesc
版本：v0.2.0-rc.3（本地构建，含 classroom-svc + classroom-gui）

一、人工安装（双击 EXE）
1. 双击 ClassShare-$cap-Setup.exe
2. UAC 选「是」
3. 按提示输入$(if ($isTeacher) { '：房间码（默认 XK-204）、PIN（可空）' } else { '：昵称（可空）、教师机地址（可空=自动发现）、房间码（可空）' })
4. 安装完成：程序在 C:\ProgramData\ClassShare\bin，服务 ClassShareSvc 自动启动

二、AI / 静默安装（推荐给 AI 使用，无交互）
1. 下载 ClassShare-$cap-Setup.zip 并解压到 %LOCALAPPDATA%\ClassShareSetup
2. 以管理员运行（只需确认一次 UAC）：
   $silentExample
3. 若 AI 环境禁止提权（RunAs）：由你手动右键「以管理员身份运行」%LOCALAPPDATA%\ClassShareSetup\install-as-admin.cmd（可带参数，如 -Room XK-204）
4. 日志：%ProgramData%\ClassShareSetup.log
4. 验证：Get-Service ClassShareSvc（Running）；%ProgramData%\ClassShare\data\logs\svc.log

三、验证与连接
- 服务：services.msc → ClassShareSvc
- 日志：%ProgramData%\ClassShare\data\logs\svc.log
- $(if ($isTeacher) { '学生端用同一房间码（有 PIN 同 PIN）或填你的 IP:47821 直连' } else { '同局域网自动发现；跨网段在安装时填教师机地址' })

四、卸载
C:\ProgramData\ClassShare\bin\uninstall-$m.cmd

五、注意
- 未签名：SmartScreen →「更多信息 → 仍要运行」
- 自动发现仅限同一广播域；跨网段必须填教师机地址
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "README-$m.txt"), $readme, $utf8bom)

    $files = @(
        "install-$m.cmd", "setup-$m.ps1", 'install.ps1', 'silent-install.ps1',
        'install-as-admin.ps1', 'install-as-admin.cmd',
        "uninstall-$m.ps1", "uninstall-$m.cmd", 'uninstall.ps1',
        'classroom-svc.exe', 'classroom-gui.exe', "README-$m.txt"
    )
    $fileVals = for ($i = 0; $i -lt $files.Count; $i++) { "FILE$i=`"$($files[$i])`"" }
    $fileRefs = for ($i = 0; $i -lt $files.Count; $i++) { "%FILE$i%=" }
    $targetName = Join-Path $OutDir ("ClassShare-{0}-Setup.exe" -f $cap)
    $sed = @"
[Version]
Class=IEXPRESS
SEDVersion=3
[Options]
PackagePurpose=InstallApp
ShowInstallProgramWindow=1
HideExtractAnimation=0
UseLongFileName=1
InsideCompressed=0
CAB_FixedSize=0
CAB_ResvCodeSigning=0
RebootMode=N
InstallPrompt=%InstallPrompt%
DisplayLicense=%DisplayLicense%
FinishMessage=%FinishMessage%
TargetName=%TargetName%
FriendlyName=%FriendlyName%
AppLaunched=%AppLaunched%
PostInstallCmd=%PostInstallCmd%
AdminQuietInstCmd=%AdminQuietInstCmd%
UserQuietInstCmd=%UserQuietInstCmd%
SourceFiles=SourceFiles
[Strings]
InstallPrompt=
DisplayLicense=
FinishMessage=
TargetName=$targetName
FriendlyName=ClassShare $label Setup
AppLaunched=install-$m.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=install-$m.cmd
UserQuietInstCmd=install-$m.cmd
$($fileVals -join "`r`n")
[SourceFiles]
SourceFiles0=$stage\
[SourceFiles0]
$($fileRefs -join "`r`n")
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "pack-$m.sed"), $sed, [System.Text.Encoding]::GetEncoding(936))

    Write-Host "[pack] $label IExpress 构建中 ..."
    $iexpress = Join-Path $env:WINDIR 'System32\iexpress.exe'
    $proc = Start-Process -FilePath $iexpress -ArgumentList @('/N', '/Q', (Join-Path $stage "pack-$m.sed")) -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "iexpress($m) 退出码 $($proc.ExitCode)" }
    if (-not (Test-Path $targetName)) { throw "安装包未生成: $targetName" }

    $zip = Join-Path $OutDir ("ClassShare-{0}-Setup.zip" -f $cap)
    Remove-Item $zip -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path ($files | ForEach-Object { Join-Path $stage $_ }) -DestinationPath $zip -CompressionLevel Optimal
    Copy-Item (Join-Path $stage "README-$m.txt") (Join-Path $OutDir "README-$m.txt") -Force
    Write-Host "[pack] $label 完成: $targetName"
}

$modes = if ($Mode -eq 'both') { @('teacher', 'student') } else { @($Mode) }
foreach ($m in $modes) { Build-Installer $m }

Write-Host ''
Write-Host '=== 打包完成 ==='
Get-ChildItem $OutDir -File | Where-Object { $_.Name -like 'ClassShare-*-Setup.*' -or $_.Name -like 'README-*.txt' } |
    Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String -Width 120 | Write-Host
Write-Host "svc sha256 = $((Get-FileHash $svcExe -Algorithm SHA256).Hash)"
Write-Host "gui sha256 = $((Get-FileHash $guiExe -Algorithm SHA256).Hash)"

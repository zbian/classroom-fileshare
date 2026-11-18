#Requires -Version 5.1
<#
.SYNOPSIS
  学生端安装包打包脚本（单文件 Setup.exe + ZIP 兜底）。
.DESCRIPTION
  产物：
    <OutDir>\ClassShare-Student-Setup.exe   IExpress 自解压安装器（内置 svc+GUI+安装脚本）
    <OutDir>\ClassShare-Student-Setup.zip   同一内容的 ZIP（兜底/离线部署）
    <OutDir>\README-zh.txt                  安装说明
  安装流程：EXE 解压 → install-student.cmd → setup-student.ps1（UAC 提权）
    → 复制程序到 %ProgramFiles%\ClassShare → install.ps1 -Mode student
  前提：target\release\classroom-svc.exe 与 classroom-gui.exe 已构建（-Build 可代建）。
  注意：不使用 GitHub Actions，全部本地构建。
.PARAMETER OutDir
  输出目录（默认 %LOCALAPPDATA%\ClassShareDual\dist）
.PARAMETER Build
  打包前重新构建前端与两个 release 二进制
.EXAMPLE
  .\scripts\package-student-installer.ps1 -Build
#>
[CmdletBinding()]
param(
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

$stage = Join-Path $env:TEMP 'cs-student-setup'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $svcExe, $guiExe $stage -Force
Copy-Item (Join-Path $repo 'scripts\install.ps1') $stage -Force
Copy-Item (Join-Path $repo 'scripts\uninstall.ps1') $stage -Force

$setupPs1 = @'
#Requires -Version 5.1
<#
  课堂共享 · 学生端安装向导（由安装包调用）。
  未取得管理员权限时 install.ps1 会降级为当前用户登录自启（--console）。
#>
[CmdletBinding()]
param([switch]$Elevated)
$ErrorActionPreference = 'Stop'

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host '需要管理员权限来安装 Windows 服务，正在请求提权（UAC）...'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'), '-Elevated'
    ) | Out-Null
    exit
}

$here = Split-Path -Parent $PSCommandPath
Write-Host '=========================================='
Write-Host '   课堂共享 · 学生端 安装程序'
Write-Host '=========================================='
Write-Host ''
Write-Host '提示：以下均可直接回车跳过（自动发现教师机 / 默认昵称）。'
$nick = Read-Host '学生昵称（回车=默认）'
$addr = Read-Host '教师机地址（回车=自动发现；示例 192.168.1.7:47821）'
$room = Read-Host '房间码（回车=自动发现首个教室；多教室请填，如 XK-204）'

$installDir = Join-Path $env:ProgramFiles 'ClassShare'
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item (Join-Path $here 'classroom-svc.exe') (Join-Path $installDir 'classroom-svc.exe') -Force
Copy-Item (Join-Path $here 'classroom-gui.exe') (Join-Path $installDir 'classroom-gui.exe') -Force
Copy-Item (Join-Path $here 'uninstall-student.ps1') (Join-Path $installDir 'uninstall-student.ps1') -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $here 'uninstall-student.cmd') (Join-Path $installDir 'uninstall-student.cmd') -Force -ErrorAction SilentlyContinue
Copy-Item (Join-Path $here 'uninstall.ps1') (Join-Path $installDir 'uninstall.ps1') -Force -ErrorAction SilentlyContinue
Write-Host "程序已复制到: $installDir"

$params = @{
    Mode   = 'student'
    SvcExe = (Join-Path $installDir 'classroom-svc.exe')
    GuiExe = (Join-Path $installDir 'classroom-gui.exe')
}
if ($nick) { $params.Nickname = $nick }
if ($addr) { $params.ManualAddr = $addr }
if ($room) { $params.Room = $room }

& (Join-Path $here 'install.ps1') @params

Write-Host ''
Write-Host '=== 安装完成 ==='
Write-Host "程序目录: $installDir"
Write-Host 'GUI 将在下次登录时自动启动（也可手动运行 classroom-gui.exe --role=student）。'
Write-Host '卸载：运行 C:\Program Files\ClassShare\uninstall-student.cmd'
Read-Host '按回车关闭'
'@
[System.IO.File]::WriteAllText((Join-Path $stage 'setup-student.ps1'), $setupPs1, $utf8bom)

$uninstPs1 = @'
#Requires -Version 5.1
[CmdletBinding()]
param([switch]$Elevated)
$ErrorActionPreference = 'Stop'
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host '卸载需要管理员权限，正在请求提权（UAC）...'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'), '-Elevated'
    ) | Out-Null
    exit
}
$here = Split-Path -Parent $PSCommandPath
Write-Host '=== 课堂共享 · 学生端 卸载 ==='
& (Join-Path $here 'uninstall.ps1')
$installDir = Join-Path $env:ProgramFiles 'ClassShare'
if (Test-Path -LiteralPath $installDir) {
    Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "已删除程序目录: $installDir"
}
Write-Host '卸载完成（共享数据默认保留；如需清除请手动删除 %ProgramData%\ClassShare 与 %LOCALAPPDATA%\ClassShare）。'
Read-Host '按回车关闭'
'@
[System.IO.File]::WriteAllText((Join-Path $stage 'uninstall-student.ps1'), $uninstPs1, $utf8bom)

$installCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0setup-student.ps1`"`r`n"
[System.IO.File]::WriteAllText((Join-Path $stage 'install-student.cmd'), $installCmd, (New-Object System.Text.UTF8Encoding($false)))
$uninstCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0uninstall-student.ps1`"`r`n"
[System.IO.File]::WriteAllText((Join-Path $stage 'uninstall-student.cmd'), $uninstCmd, (New-Object System.Text.UTF8Encoding($false)))

$readme = @'
课堂共享 · 学生端 安装包
==========================

适用：Windows 10 / 11 x64（学生机）
版本：v0.2.0-rc.1（本地构建，含 classroom-svc + classroom-gui）

一、安装
--------
1. 双击 ClassShare-Student-Setup.exe
2. UAC 弹窗选择「是」（需要管理员权限注册 Windows 服务）
3. 按提示输入（均可直接回车）：
   - 学生昵称：回车=默认
   - 教师机地址：回车=自动发现（同一局域网）；也可填 192.168.1.7:47821
   - 房间码：单教室直接回车；多教室请填与教师一致的房间码（如 XK-204）
4. 安装完成：程序复制到 C:\Program Files\ClassShare，服务 ClassShareSvc 自动启动

二、验证
--------
- 服务状态：Win+R 输入 services.msc，查找 ClassShareSvc（应为「正在运行」）
- 日志：%ProgramData%\ClassShare\data\logs\svc.log
- GUI：登录后自动启动；也可手动运行 "C:\Program Files\ClassShare\classroom-gui.exe" --role=student
- 若 GUI 显示「服务未运行」：确认服务已启动，或注销后重新登录一次

三、常见问题
------------
- 自动发现不到教师机：确认两台机器同一网段、教师机已开课；或安装时填写「教师机地址」
- 学生机未登录时也能收文件（服务在 Session 0 运行，属预期）
- Windows SmartScreen 提示「未知发布者」：安装包为本地构建、未签名，点「更多信息 → 仍要运行」

四、卸载
--------
运行 C:\Program Files\ClassShare\uninstall-student.cmd（需要管理员权限）
共享数据默认保留；如需彻底清除，手动删除 %ProgramData%\ClassShare 与 %LOCALAPPDATA%\ClassShare

五、注意
--------
- 安装包内已含 classroom-svc.exe / classroom-gui.exe，无需额外运行库
- 若教师端与学生在不同网段，必须填写教师机地址（自动发现仅限同一广播域）
'@
[System.IO.File]::WriteAllText((Join-Path $stage 'README-zh.txt'), $readme, $utf8bom)

$files = @(
    'install-student.cmd', 'setup-student.ps1', 'install.ps1',
    'uninstall-student.ps1', 'uninstall-student.cmd', 'uninstall.ps1',
    'classroom-svc.exe', 'classroom-gui.exe', 'README-zh.txt'
)
$fileVals = for ($i = 0; $i -lt $files.Count; $i++) { "FILE$i=`"$($files[$i])`"" }
$fileRefs = for ($i = 0; $i -lt $files.Count; $i++) { "%FILE$i%=" }
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
TargetName=$OutDir\ClassShare-Student-Setup.exe
FriendlyName=ClassShare Student Setup
AppLaunched=install-student.cmd
PostInstallCmd=<None>
AdminQuietInstCmd=install-student.cmd
UserQuietInstCmd=install-student.cmd
$($fileVals -join "`r`n")
[SourceFiles]
SourceFiles0=$stage\
[SourceFiles0]
$($fileRefs -join "`r`n")
"@
[System.IO.File]::WriteAllText((Join-Path $stage 'student.sed'), $sed, [System.Text.Encoding]::GetEncoding(936))

Write-Host '[pack] IExpress 构建中 ...'
$iexpress = Join-Path $env:WINDIR 'System32\iexpress.exe'
$proc = Start-Process -FilePath $iexpress -ArgumentList @('/N', '/Q', (Join-Path $stage 'student.sed')) -Wait -PassThru
if ($proc.ExitCode -ne 0) { throw "iexpress 退出码 $($proc.ExitCode)" }
$exe = Join-Path $OutDir 'ClassShare-Student-Setup.exe'
if (-not (Test-Path $exe)) { throw '安装包未生成' }

$zip = Join-Path $OutDir 'ClassShare-Student-Setup.zip'
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path ($files | ForEach-Object { Join-Path $stage $_ }) -DestinationPath $zip -CompressionLevel Optimal
Copy-Item (Join-Path $stage 'README-zh.txt') (Join-Path $OutDir 'README-zh.txt') -Force

Write-Host ''
Write-Host '=== 打包完成 ==='
Get-ChildItem $OutDir -File | Where-Object { $_.Name -like 'ClassShare-Student-Setup.*' -or $_.Name -eq 'README-zh.txt' } |
    Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize | Out-String -Width 120 | Write-Host
Write-Host "svc sha256 = $((Get-FileHash $svcExe -Algorithm SHA256).Hash)"
Write-Host "gui sha256 = $((Get-FileHash $guiExe -Algorithm SHA256).Hash)"

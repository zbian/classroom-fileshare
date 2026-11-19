#Requires -Version 5.1
<#
.SYNOPSIS
  教师端 / 学生端安装包打包脚本（单文件 Setup.exe + ZIP 兜底）。
.DESCRIPTION
  产物（<OutDir>）：
    ClassShare-Teacher-Setup.exe / ClassShare-Teacher-Setup.zip
    ClassShare-Student-Setup.exe / ClassShare-Student-Setup.zip
    README-teacher.txt / README-student.txt
  安装流程：EXE 解压 → install-<mode>.cmd → setup-<mode>.ps1（UAC 提权）
    → 复制程序到 %ProgramFiles%\ClassShare → install.ps1 -Mode <mode>
  前提：target\release\classroom-svc.exe 与 classroom-gui.exe 已构建（-Build 可代建）。
  不使用 GitHub Actions，全部本地构建。
.PARAMETER Mode
  teacher 或 student（默认 both：两种都打）
.PARAMETER OutDir
  输出目录（默认 %LOCALAPPDATA%\ClassShareDual\dist）
.PARAMETER Build
  打包前重新构建前端与两个 release 二进制
.EXAMPLE
  .\scripts\package-installer.ps1 -Build
.EXAMPLE
  .\scripts\package-installer.ps1 -Mode teacher
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
  课堂共享 · $label 安装向导（由安装包调用）。
  未取得管理员权限时 install.ps1 会降级为当前用户登录自启（--console）。
#>
[CmdletBinding()]
param([switch]`$Elevated)
`$ErrorActionPreference = 'Stop'
`$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not `$isAdmin) {
    Write-Host '需要管理员权限来安装 Windows 服务，正在请求提权（UAC）...'
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @(
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
`$installDir = Join-Path `$env:ProgramFiles 'ClassShare'
New-Item -ItemType Directory -Force -Path `$installDir | Out-Null
Copy-Item (Join-Path `$here 'classroom-svc.exe') (Join-Path `$installDir 'classroom-svc.exe') -Force
Copy-Item (Join-Path `$here 'classroom-gui.exe') (Join-Path `$installDir 'classroom-gui.exe') -Force
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
Write-Host '卸载：运行 C:\Program Files\ClassShare\uninstall-$m.cmd'
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
    Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + `$PSCommandPath + '"'), '-Elevated'
    ) | Out-Null
    exit
}
`$here = Split-Path -Parent `$PSCommandPath
Write-Host '=== 课堂共享 · $label 卸载 ==='
& (Join-Path `$here 'uninstall.ps1')
`$installDir = Join-Path `$env:ProgramFiles 'ClassShare'
if (Test-Path -LiteralPath `$installDir) {
    Remove-Item -LiteralPath `$installDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "已删除程序目录: `$installDir"
}
Write-Host '卸载完成（共享数据默认保留；如需清除请手动删除 %ProgramData%\ClassShare 与 %LOCALAPPDATA%\ClassShare）。'
Read-Host '按回车关闭'
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "uninstall-$m.ps1"), $uninstallPs1, $utf8bom)

    $installCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0setup-$m.ps1`"`r`n"
    [System.IO.File]::WriteAllText((Join-Path $stage "install-$m.cmd"), $installCmd, (New-Object System.Text.UTF8Encoding($false)))
    $uninstCmd = "@echo off`r`nchcp 65001 >nul`r`npowershell -NoProfile -ExecutionPolicy Bypass -File `"%~dp0uninstall-$m.ps1`"`r`n"
    [System.IO.File]::WriteAllText((Join-Path $stage "uninstall-$m.cmd"), $uninstCmd, (New-Object System.Text.UTF8Encoding($false)))

    $roleDesc = if ($isTeacher) { '教师端（建课、广播文件/文本、接收学员提交）' } else { '学生端（自动发现/入班、接收共享、提交作业）' }
    $readme = @"
课堂共享 · $label 安装包
==========================

适用：Windows 10 / 11 x64
角色：$roleDesc
版本：v0.2.0-rc.2（本地构建，含 classroom-svc + classroom-gui）

一、安装
--------
1. 双击 ClassShare-$($m.Substring(0,1).ToUpper() + $m.Substring(1))-Setup.exe
2. UAC 弹窗选择「是」（需要管理员权限注册 Windows 服务）
3. 按提示输入：
$(if ($isTeacher) { '   - 房间码：默认 XK-204（学员端需填同一房间码）' + "`r`n" + '   - PIN：可选（设置后学员加入需输入）' } else { '   - 学生昵称：回车=默认' + "`r`n" + '   - 教师机地址：回车=自动发现（同一局域网）；跨网段填 192.168.1.7:47821' + "`r`n" + '   - 房间码：单教室回车；多教室填与教师一致' })
4. 安装完成：程序复制到 C:\Program Files\ClassShare，服务 ClassShareSvc 自动启动

二、验证
--------
- 服务状态：services.msc 中 ClassShareSvc 应为「正在运行」
- 日志：%ProgramData%\ClassShare\data\logs\svc.log
- GUI：登录后自动启动；也可手动运行 "C:\Program Files\ClassShare\classroom-gui.exe" --role=$m

三、卸载
--------
运行 C:\Program Files\ClassShare\uninstall-$m.cmd（需要管理员权限）

四、注意
--------
- 未签名安装包：SmartScreen 提示「未知发布者」→「更多信息 → 仍要运行」
- 自动发现仅限同一广播域；跨网段必须填教师机地址
"@
    [System.IO.File]::WriteAllText((Join-Path $stage "README-$m.txt"), $readme, $utf8bom)

    $files = @(
        "install-$m.cmd", "setup-$m.ps1", 'install.ps1',
        "uninstall-$m.ps1", "uninstall-$m.cmd", 'uninstall.ps1',
        'classroom-svc.exe', 'classroom-gui.exe', "README-$m.txt"
    )
    $fileVals = for ($i = 0; $i -lt $files.Count; $i++) { "FILE$i=`"$($files[$i])`"" }
    $fileRefs = for ($i = 0; $i -lt $files.Count; $i++) { "%FILE$i%=" }
    $targetName = Join-Path $OutDir ("ClassShare-{0}-Setup.exe" -f ($m.Substring(0,1).ToUpper() + $m.Substring(1)))
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
    [System.IO.File]::WriteAllText((Join-Path $stage "student-$m.sed"), $sed, [System.Text.Encoding]::GetEncoding(936))

    Write-Host "[pack] $label IExpress 构建中 ..."
    $iexpress = Join-Path $env:WINDIR 'System32\iexpress.exe'
    $proc = Start-Process -FilePath $iexpress -ArgumentList @('/N', '/Q', (Join-Path $stage "student-$m.sed")) -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "iexpress($m) 退出码 $($proc.ExitCode)" }
    if (-not (Test-Path $targetName)) { throw "安装包未生成: $targetName" }

    $zip = Join-Path $OutDir ("ClassShare-{0}-Setup.zip" -f ($m.Substring(0,1).ToUpper() + $m.Substring(1)))
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

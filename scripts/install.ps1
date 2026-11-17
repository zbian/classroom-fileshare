#Requires -Version 5.1
<#
.SYNOPSIS
  课堂共享后台服务安装脚本（PRD FR-G-05、TC-11/12、RK-06）。

.DESCRIPTION
  管理员路径：写 %PROGRAMDATA%\ClassShare\svc.json → 注册 Windows 服务
  （Session 0、开机自启）→ SCM failure 自动重启（5s/5s/30s）→ 防火墙规则
  （teacher：入站 TCP 47821 / UDP 47820）→ 启动服务 → GUI 登录自启
  （HKCU Run）。
  非管理员路径（降级，EX-10）：svc.json 与数据根转 %LOCALAPPDATA%\ClassShare，
  经 HKCU Run 以登录自启前台进程（--console）运行。局限：未登录不接收、
  服务崩溃不自动重启。

.EXAMPLE
  .\install.ps1 -Mode teacher -Room XK-204 -Pin 9527
  .\install.ps1 -Mode student -Nickname alice -ManualAddr 192.168.1.10:47821
#>
[CmdletBinding()]
param(
    [ValidateSet('teacher', 'student')][string]$Mode = 'teacher',
    [string]$Room = 'XK-204',
    [string]$Pin = '',
    [string]$Nickname = '',
    [string]$ManualAddr = '',
    [string]$SvcExe = '',
    [string]$GuiExe = '',
    [string]$UserSid = ''
)
$ErrorActionPreference = 'Stop'

function Resolve-Binary([string]$Path, [string]$FallbackName) {
    if ($Path) { return (Resolve-Path -LiteralPath $Path).Path }
    # M1 验收 O-4：脚本不在二进制同目录时（如从仓库根/验收目录调用），
    # 除脚本同级外再搜标准构建输出目录，并在失败信息里给出显式参数指引。
    $candidates = @(
        (Join-Path $PSScriptRoot $FallbackName),
        (Join-Path $PSScriptRoot "..\target\release\$FallbackName"),
        (Join-Path $PSScriptRoot "..\target\debug\$FallbackName")
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate) { return (Resolve-Path -LiteralPath $candidate).Path }
    }
    $tried = $candidates -join '; '
    throw "未找到 $FallbackName。已查找：$tried。若二进制不在此处，请显式指定 -SvcExe <path>（服务）/ -GuiExe <path>（GUI）；生产构建请先执行 scripts\build-gui.ps1。"
}

function Write-SvcConfig([string]$Dir, [hashtable]$Config) {
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $json = $Config | ConvertTo-Json
    # 无 BOM UTF-8：serde_json 解析 svc.json 不接受 BOM
    [IO.File]::WriteAllText((Join-Path $Dir 'svc.json'), $json, [Text.UTF8Encoding]::new($false))
}

$svcExe = Resolve-Binary $SvcExe 'classroom-svc.exe'
$guiExe = if ($GuiExe) { (Resolve-Path -LiteralPath $GuiExe).Path } else { $null }

# PR #69 发现 1（PRD 2.4「学生 1 步设昵称」）：学员模式未显式传 -Room 时
# 写空字符串 → svc 侧 room_filter=None → 自动连接首个发现（单教室学生
# 无需知道房间码）；多教室需显式 -Room 与教师一致。教师模式保持默认。
$roomExplicit = $PSBoundParameters.ContainsKey('Room')
$effectiveRoom = if ($Mode -eq 'student' -and -not $roomExplicit) { '' } else { $Room }

$svcConfig = [ordered]@{
    mode        = $Mode
    room_code   = $effectiveRoom
    pin         = if ($Pin) { $Pin } else { $null }
    nickname    = if ($Nickname) { $Nickname } else { '' }
    manual_addr = if ($ManualAddr) { $ManualAddr } else { $null }
    tcp_port    = 47821
    user_sid    = $null
}

# E7 令牌分发（issue #49）：主登录用户 SID——服务模式 GUI 读令牌的授权
# 对象。-UserSid 显式传入优先；否则自动探测活动控制台会话用户
# （quser → CIM Win32_UserAccount 查 SID）；裸机镜像探测不到时跳过并
# WARN（GUI 令牌待首次登录后由服务 SessionChange 自愈）。
function Resolve-UserSid([string]$Explicit) {
    if ($Explicit) {
        if ($Explicit -notlike 'S-1-*') { throw "-UserSid 必须是 SID（S-1-...）形式" }
        return $Explicit
    }
    try {
        $quser = quser.exe 2>$null | Out-String
        $row = ($quser -split "`r?`n") |
            Where-Object { $_ -match '^\s*(.+?)\s+\d+\s+Console\s' } |
            Select-Object -First 1
        if ($row) {
            $userName = $Matches[1].Trim('>', ' ').Trim()
            $sid = (Get-CimInstance Win32_UserAccount -Filter "LocalAccount=True AND Name='$userName'" |
                Select-Object -First 1).SID
            if ($sid) { return $sid }
        }
    } catch {
        Write-Warning "自动探测登录用户 SID 失败：$_"
    }
    return $null
}
$userSid = Resolve-UserSid $UserSid
if ($userSid) {
    $svcConfig.user_sid = $userSid
} else {
    Write-Warning '未捕获主登录用户 SID：服务模式 GUI 令牌待首次登录后由服务自愈（SessionChange）授权；如需立即可用请携带 -UserSid 重装'
}

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$firewallSummary = '（降级模式无法配置防火墙；自动发现可能不可用，manual_addr 兜底）'
if ($isAdmin) {
    $base = Join-Path $env:ProgramData 'ClassShare'
    Write-SvcConfig $base $svcConfig

    sc.exe create ClassShareSvc binPath= "`"$svcExe`"" start= auto | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'sc.exe create 失败' }
    sc.exe description ClassShareSvc '课堂共享后台服务（ClassShare）' | Out-Null
    # PRD FR-G-05：崩溃由 SCM 自动重启（5s / 5s / 30s），计数 86400s 重置
    sc.exe failure ClassShareSvc reset= 86400 actions= restart/5000/restart/5000/restart/30000 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'sc.exe failure 配置失败' }

    # UDP 发现入站：两种角色都需要（Windows 默认入站策略=阻止，学员机不建
    # 规则则自动发现必失败——qa v1.2 E1 判定为产品缺陷）；程序限定服务进程
    netsh advfirewall firewall delete rule name='ClassShare Discovery UDP' 2>$null | Out-Null
    netsh advfirewall firewall add rule name='ClassShare Discovery UDP' dir=in action=allow protocol=UDP localport=47820 program="$svcExe" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '防火墙 UDP 47820 规则添加失败' }

    if ($Mode -eq 'teacher') {
        netsh advfirewall firewall delete rule name='ClassShare Service TCP' 2>$null | Out-Null
        netsh advfirewall firewall add rule name='ClassShare Service TCP' dir=in action=allow protocol=TCP localport=47821 program="$svcExe" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw '防火墙 TCP 47821 规则添加失败' }
        $firewallSummary = 'ClassShare Service TCP（入站 TCP/47821）；ClassShare Discovery UDP（入站 UDP/47820，程序限定）'
    } else {
        $firewallSummary = 'ClassShare Discovery UDP（入站 UDP/47820，程序限定）——student 无 TCP 入站'
    }

    sc.exe start ClassShareSvc | Out-Null
    if ($LASTEXITCODE -ne 0) { throw '服务启动失败（查看 data\logs\svc.log）' }
    $dataRoot = Join-Path $base 'data'
    $runMode = 'Windows 服务（Session 0；SCM failure 自动重启 5s/5s/30s）'
} else {
    $base = Join-Path $env:LOCALAPPDATA 'ClassShare'
    Write-SvcConfig $base $svcConfig
    reg.exe add 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v ClassShareSvc `
        /t REG_SZ /d "`"$svcExe`" --console" /f | Out-Null
    $dataRoot = Join-Path $base 'data'
    $runMode = '降级（HKCU Run 登录自启前台进程 --console）'
    Write-Warning '无管理员权限：已降级为仅当前用户登录自启。局限：未登录时不接收文件；服务崩溃不自动重启。'
}

# GUI 登录自启（写入执行安装的用户的 HKCU；机房多用户场景需逐用户或镜像配置）
if ($guiExe) {
    reg.exe add 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v ClassShareGUI `
        /t REG_SZ /d "`"$guiExe`"" /f | Out-Null
} else {
    # qa v1.2 E2：不静默跳过——显式警告（TC-11 步骤 5 依赖此注册）
    Write-Warning '未指定 -GuiExe：GUI 登录自启未注册（登录后需手动启动 GUI；验收用例依赖此注册项）'
}

Write-Host ''
Write-Host '=== 课堂共享安装取证（issue #14 五项）==='
Write-Host "服务名:     ClassShareSvc"
Write-Host "进程:       $svcExe"
Write-Host "运行方式:   $runMode"
Write-Host "数据根:     $dataRoot"
Write-Host "日志:       $(Join-Path $dataRoot 'logs\svc.log')（RFC3339 毫秒时间戳，RUST_LOG 可调）"
Write-Host "防火墙:     $firewallSummary"
Write-Host "GUI 自启:   $(if ($guiExe) { "HKCU Run → $guiExe" } else { '未注册（未指定 -GuiExe）——TC-11 步骤 5 依赖此注册项' })"
Write-Host "令牌授权:   $(if ($userSid) { "LocalSystem + 主登录用户（$userSid）" } else { '仅 LocalSystem——GUI 待首次登录自愈或 -UserSid 重装' })"
Write-Host "房间/发现:  $(if ($Mode -eq 'teacher') { "教师建课房间码 $effectiveRoom（安装后可在 GUI 修改）" } elseif ($effectiveRoom) { "学员自动发现匹配房间 $effectiveRoom（多教室，需与教师一致）" } else { '学员自动连接首个发现的教师机（单教室默认；多教室请传 -Room 与教师一致）' })"
Write-Host "多实例:     命名管道多连接并发（每 GUI 一连接，已测 ≥2）；服务本体单实例（SCM 语义）"

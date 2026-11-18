#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— Syncthing 环境安装与配置（A/B 两机各跑一次）。
.DESCRIPTION
  用 Syncthing 在两台机器之间自动同步「测试证据目录」（点对点，不需要 SMB/SFTP）。
  本脚本幂等：
    1. 安装（缺失时下载 pinned 版本到 %LOCALAPPDATA%\ClassShareDual\syncthing）；
    2. 生成/复用配置（独立 home，不污染系统 Syncthing）；
    3. 启动（--no-browser，GUI 127.0.0.1:<GuiPort>）；
    4. 通过 REST 配置：设备（对端）、共享文件夹（本机路径 + 类型）；
    5. 输出本机 Device ID 与对端配置命令（JSON）。
.PARAMETER Role
  teacher（A 机，Receive Only）/ student（B 机，Send Only）。
.PARAMETER PeerDeviceId
  对端 Device ID（第一次配置时由对端脚本输出，复制过来）。
.PARAMETER PeerName
  对端显示名（默认 Peer）。
.PARAMETER EvidenceDir
  本机证据目录（默认 <repo>\docs\acceptance\evidence\dual）。
.PARAMETER StHome
  Syncthing 配置目录（默认 %LOCALAPPDATA%\ClassShareDual\syncthing-home）。
.PARAMETER GuiPort / SyncPort / LocalDiscoveryPort
  GUI 端口（默认 8384）/ 同步端口（22000）/ 本地发现端口（21027）；本机多实例时改。
.PARAMETER LocalOnly
  关闭全局发现/中继/NAT（纯内网测试）。
.PARAMETER NoStart
  只配置不启动。
.PARAMETER Stop
  停止本 home 对应的 Syncthing 进程。
.PARAMETER WaitPeerSec
  启动后等待对端连接秒数（0=不等待）。
.EXAMPLE
  .\syncthing-setup.ps1 -Role teacher -PeerDeviceId ABCD123-... -WaitPeerSec 60
#>
[CmdletBinding()]
param(
    [ValidateSet('teacher', 'student')][string]$Role = 'teacher',
    [string]$PeerDeviceId = '',
    [string]$PeerName = 'Peer',
    [string]$EvidenceDir = '',
    [string]$StHome = '',
    [int]$GuiPort = 8384,
    [int]$SyncPort = 22000,
    [int]$LocalDiscoveryPort = 21027,
    [switch]$LocalOnly,
    [switch]$NoStart,
    [switch]$Stop,
    [int]$WaitPeerSec = 0
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $EvidenceDir) { $EvidenceDir = Join-Path $repoRoot 'docs\acceptance\evidence\dual' }
if (-not $StHome) { $StHome = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing-home' }
$baseDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual'
$stExe = Join-Path $baseDir 'syncthing\syncthing.exe'
$stVersion = 'v2.1.7'
$folderId = 'classshare-dual-evidence'

function Ensure-Syncthing {
    if (Test-Path $stExe) { return }
    Write-Host "[install] 下载 Syncthing $stVersion ..."
    New-Item -ItemType Directory -Force $baseDir | Out-Null
    $zip = Join-Path $baseDir "syncthing-$stVersion.zip"
    if (-not (Test-Path $zip)) {
        $url = "https://github.com/syncthing/syncthing/releases/download/$stVersion/syncthing-windows-amd64-$stVersion.zip"
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing -TimeoutSec 300
    }
    Expand-Archive -Path $zip -DestinationPath $baseDir -Force
    $sub = Get-ChildItem $baseDir -Directory | Where-Object { $_.Name -like 'syncthing-windows-amd64*' } | Select-Object -First 1
    if ($sub) { Move-Item $sub.FullName (Join-Path $baseDir 'syncthing') -Force }
    if (-not (Test-Path $stExe)) { throw "Syncthing 安装失败：$stExe 不存在" }
    Write-Host "[install] OK: $(& $stExe version 2>&1 | Select-Object -First 1)"
}

function Get-ProcessForHome {
    Get-CimInstance Win32_Process -Filter "Name='syncthing.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.CommandLine -and $_.CommandLine -like "*$StHome*" }
}

function Get-ApiKey {
    $cfg = Join-Path $StHome 'config.xml'
    if (-not (Test-Path $cfg)) { throw "config.xml 不存在: $cfg" }
    $m = [regex]::Match((Get-Content $cfg -Raw), '<apikey>([^<]+)</apikey>')
    if (-not $m.Success) { throw 'config.xml 中未找到 apikey' }
    return $m.Groups[1].Value
}

function Invoke-St([string]$method, [string]$path, $body = $null) {
    $headers = @{ 'X-API-Key' = (Get-ApiKey) }
    $uri = "http://127.0.0.1:$GuiPort$path"
    if ($null -ne $body) {
        # 注意：PS 5.1 的 -Body 字符串按 ANSI 编码发送，中文路径会变成 ??（Syncthing 报 filename syntax incorrect）
        # 必须显式转 UTF-8 字节 + charset，否则含中文的仓库路径无法同步
        $headers['Content-Type'] = 'application/json; charset=utf-8'
        $json = $body | ConvertTo-Json -Depth 8
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($json)
        return Invoke-RestMethod -Method $method -Uri $uri -Headers $headers -Body $bytes -TimeoutSec 30
    }
    return Invoke-RestMethod -Method $method -Uri $uri -Headers $headers -TimeoutSec 30
}

function Wait-Gui([int]$timeoutSec = 30) {
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        try { Invoke-RestMethod -Uri "http://127.0.0.1:$GuiPort/rest/noauth/health" -TimeoutSec 2 | Out-Null; return $true } catch { Start-Sleep -Milliseconds 500 }
    }
    return $false
}

if ($Stop) {
    $procs = Get-ProcessForHome
    foreach ($p in $procs) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue; Write-Host "[stop] pid=$($p.ProcessId)" }
    if (-not $procs) { Write-Host '[stop] 无匹配进程' }
    return
}

Ensure-Syncthing
New-Item -ItemType Directory -Force $StHome | Out-Null
New-Item -ItemType Directory -Force $EvidenceDir | Out-Null

if (-not (Test-Path (Join-Path $StHome 'config.xml'))) {
    Write-Host '[config] 生成配置 ...'
    & $stExe generate --home="$StHome" 2>&1 | Out-Null
}
$myId = (& $stExe device-id --home="$StHome" 2>&1 | Select-Object -First 1).Trim()
Write-Host "[config] home=$StHome"
Write-Host "[config] my device id=$myId"

if (-not $NoStart -and -not (Get-ProcessForHome)) {
    $log = Join-Path $StHome 'syncthing.log'
    Start-Process -FilePath $stExe -ArgumentList @('serve', "--home=$StHome", '--no-browser', "--gui-address=127.0.0.1:$GuiPort", "--log-file=$log", '--log-level=INFO') -WindowStyle Hidden | Out-Null
    Write-Host "[start] 已启动（gui=http://127.0.0.1:$GuiPort）"
}
if ($NoStart) { Write-Host '[start] 跳过（-NoStart）' }
elseif (-not (Wait-Gui 30)) { throw "GUI 未就绪: http://127.0.0.1:$GuiPort" }

if (-not $NoStart) {
    $opts = Invoke-St 'GET' '/rest/config/options'
    $opts.listenAddresses = @("tcp://0.0.0.0:$SyncPort", "quic://0.0.0.0:$SyncPort")
    $opts.localAnnouncePort = $LocalDiscoveryPort
    $opts.localAnnounceMCAddr = "[ff12::8384]:$LocalDiscoveryPort"
    $opts.startBrowser = $false
    if ($LocalOnly) {
        $opts.globalAnnounceEnabled = $false
        $opts.relaysEnabled = $false
        $opts.natEnabled = $false
    }
    Invoke-St 'PUT' '/rest/config/options' $opts | Out-Null
    Write-Host "[cfg] options: sync=$SyncPort discovery=$LocalDiscoveryPort localOnly=$([bool]$LocalOnly)"

    if ($PeerDeviceId) {
        $peer = [ordered]@{ deviceID = $PeerDeviceId; name = $PeerName; addresses = @('dynamic'); paused = $false }
        try { Invoke-St 'PUT' "/rest/config/devices/$PeerDeviceId" $peer | Out-Null; Write-Host "[cfg] peer device=$PeerDeviceId ($PeerName)" }
        catch { Write-Warning "添加对端设备失败（Device ID 格式？）：$($_.Exception.Message)" }
    } else {
        Write-Host '[cfg] 未提供 -PeerDeviceId：仅配置本机，等对端 Device ID 后重跑'
    }

    $type = if ($Role -eq 'teacher') { 'receiveonly' } else { 'sendonly' }
    $devices = @()
    if ($PeerDeviceId) { $devices += [ordered]@{ deviceID = $PeerDeviceId } }
    $folder = [ordered]@{
        id = $folderId; label = 'ClassShare Dual Evidence'; path = $EvidenceDir; type = $type
        devices = $devices; rescanIntervalS = 10; fsWatcherEnabled = $true; ignorePerms = $true
    }
    Invoke-St 'PUT' "/rest/config/folders/$folderId" $folder | Out-Null
    Write-Host "[cfg] folder=$folderId type=$type path=$EvidenceDir"
}

$summary = [ordered]@{
    role = $Role
    machine = $env:COMPUTERNAME
    deviceId = $myId
    home = $StHome
    gui = "http://127.0.0.1:$GuiPort"
    folderId = $folderId
    folderPath = $EvidenceDir
    folderType = $(if ($Role -eq 'teacher') { 'receiveonly' } else { 'sendonly' })
    peerDeviceId = $PeerDeviceId
    syncPort = $SyncPort
    localDiscoveryPort = $LocalDiscoveryPort
    localOnly = [bool]$LocalOnly
}

if ($WaitPeerSec -gt 0 -and $PeerDeviceId -and -not $NoStart) {
    Write-Host "[wait] 等待对端连接（最多 ${WaitPeerSec}s）..."
    $deadline = (Get-Date).AddSeconds($WaitPeerSec)
    $connected = $false
    while ((Get-Date) -lt $deadline) {
        try {
            $conn = Invoke-St 'GET' '/rest/system/connections'
            $c = $conn.connections.PSObject.Properties[$PeerDeviceId]
            if ($c -and $c.Value.connected) { $connected = $true; break }
        } catch { }
        Start-Sleep -Seconds 1
    }
    $summary['peerConnected'] = $connected
    Write-Host "[wait] peerConnected=$connected"
}

Write-Output ($summary | ConvertTo-Json -Depth 6)
Write-Host ''
Write-Host '=== 对端机器执行（把 Device ID 换成上面输出）===' -ForegroundColor Cyan
Write-Host ("  .\scripts\dual\syncthing-setup.ps1 -Role {0} -PeerDeviceId {1} -PeerName '{2}'" -f $(if ($Role -eq 'teacher') { 'student' } else { 'teacher' }), $myId, $env:COMPUTERNAME)

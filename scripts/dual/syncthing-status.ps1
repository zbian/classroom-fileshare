#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— Syncthing 状态查看 / 等待对端连接。
.DESCRIPTION
  读取本机 Syncthing REST，输出：本机 Device ID、对端连接、共享文件夹状态。
  -WaitConnectedSec 用于双机测试前等待对端上线（超时返回非 0）。
.PARAMETER StHome
  Syncthing 配置目录（默认 %LOCALAPPDATA%\ClassShareDual\syncthing-home）。
.PARAMETER GuiPort
  GUI 端口（默认 8384；本机多实例时改）。
.PARAMETER PeerDeviceId
  对端 Device ID（等待连接用）。
.PARAMETER WaitConnectedSec
  等待对端连接秒数（0=不等待）。
.EXAMPLE
  .\syncthing-status.ps1 -PeerDeviceId ABCD123-... -WaitConnectedSec 60
#>
[CmdletBinding()]
param(
    [string]$StHome = '',
    [int]$GuiPort = 8384,
    [string]$PeerDeviceId = '',
    [int]$WaitConnectedSec = 0
)

$ErrorActionPreference = 'Stop'
if (-not $StHome) { $StHome = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing-home' }
$cfgPath = Join-Path $StHome 'config.xml'
if (-not (Test-Path $cfgPath)) { throw "config.xml 不存在: $cfgPath（先跑 syncthing-setup.ps1）" }
$m = [regex]::Match((Get-Content $cfgPath -Raw), '<apikey>([^<]+)</apikey>')
if (-not $m.Success) { throw 'config.xml 中未找到 apikey' }
$apiKey = $m.Groups[1].Value
$headers = @{ 'X-API-Key' = $apiKey }
$base = "http://127.0.0.1:$GuiPort"

function Get-St([string]$path) { Invoke-RestMethod -Uri "$base$path" -Headers $headers -TimeoutSec 10 }

try {
    $status = Get-St '/rest/system/status'
} catch {
    Write-Host "[error] 连不上 Syncthing GUI（$base）：$($_.Exception.Message)" -ForegroundColor Red
    Write-Host '先执行：.\scripts\dual\syncthing-setup.ps1'
    exit 2
}

$conns = Get-St '/rest/system/connections'
$folders = @()
try {
    foreach ($f in (Get-St '/rest/config/folders')) {
        $fs = $null
        try { $fs = Get-St "/rest/db/status?folder=$($f.id)" } catch { }
        $folders += [ordered]@{
            id = $f.id
            label = $f.label
            path = $f.path
            type = $f.type
            state = $(if ($fs) { $fs.state } else { 'unknown' })
            needFiles = $(if ($fs) { $fs.needFiles } else { $null })
            localFiles = $(if ($fs) { $fs.localFiles } else { $null })
        }
    }
} catch { }

$peerConn = $null
if ($PeerDeviceId) {
    $p = $conns.connections.PSObject.Properties[$PeerDeviceId]
    if ($p) { $peerConn = [ordered]@{ deviceID = $PeerDeviceId; connected = $p.Value.connected; address = $p.Value.address } }
}

$result = [ordered]@{
    machine = $env:COMPUTERNAME
    myID = $status.myID
    uptimeSec = $status.uptime
    gui = $base
    peers = @($conns.connections.PSObject.Properties | ForEach-Object {
        [ordered]@{ deviceID = $_.Name; connected = $_.Value.connected; address = $_.Value.address }
    })
    peer = $peerConn
    folders = $folders
}

if ($WaitConnectedSec -gt 0) {
    if (-not $PeerDeviceId) { throw '-WaitConnectedSec 需要同时给 -PeerDeviceId' }
    Write-Host "[wait] 等待对端 $PeerDeviceId 连接（最多 ${WaitConnectedSec}s）..."
    $deadline = (Get-Date).AddSeconds($WaitConnectedSec)
    $ok = $false
    while ((Get-Date) -lt $deadline) {
        try {
            $c = (Get-St '/rest/system/connections').connections.PSObject.Properties[$PeerDeviceId]
            if ($c -and $c.Value.connected) { $ok = $true; break }
        } catch { }
        Start-Sleep -Seconds 1
    }
    $result['peerConnected'] = $ok
    Write-Output ($result | ConvertTo-Json -Depth 6)
    if (-not $ok) { Write-Host '[fail] 对端未连接' -ForegroundColor Yellow; exit 1 }
    Write-Host '[ok] 对端已连接' -ForegroundColor Green
    exit 0
}

Write-Output ($result | ConvertTo-Json -Depth 6)

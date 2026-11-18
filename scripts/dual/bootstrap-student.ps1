#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— B 机（无人值守）一次性引导。
.DESCRIPTION
  在有 DSH 但无人干预的 B 机执行：配置 Syncthing（student 角色）、准备证据目录、
  写环境自检文件。之后由 student-runner.ps1 跑用例，证据经 Syncthing 自动回流 A 机。
  幂等：可重复执行。
.PARAMETER TeacherDeviceId
  A 机（教师）的 Syncthing Device ID（必填）。
.PARAMETER TeacherName
  A 机显示名（默认 A-Teacher）。
.PARAMETER ArtifactsDir
  被测环境包目录（默认本脚本上级目录：artifacts\）。
.PARAMETER EvidenceDir
  证据目录（默认 <ArtifactsDir>\evidence）。
.PARAMETER SyncthingHome
  Syncthing 配置目录（默认 %LOCALAPPDATA%\ClassShareDual\syncthing-home）。
.PARAMETER LocalOnly
  纯内网模式（关闭全局发现/中继/NAT），默认开启。
.PARAMETER SkipSyncthing
  跳过 Syncthing 配置（B 机已配好时）。
.EXAMPLE
  .\bootstrap-student.ps1 -TeacherDeviceId PM4DR7D-...
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$TeacherDeviceId,
    [string]$TeacherName = 'A-Teacher',
    [string[]]$TeacherAddresses = @('dynamic'),
    [string]$ArtifactsDir = '',
    [string]$EvidenceDir = '',
    [string]$SyncthingHome = '',
    [int]$GuiPort = 8384,
    [int]$SyncPort = 22000,
    [int]$LocalDiscoveryPort = 21027,
    [switch]$LocalOnly,
    [switch]$SkipSyncthing
)

$ErrorActionPreference = 'Stop'
if (-not $ArtifactsDir) { $ArtifactsDir = Split-Path $PSScriptRoot -Parent }
if (-not $EvidenceDir) { $EvidenceDir = Join-Path $ArtifactsDir 'evidence' }
if (-not $SyncthingHome) { $SyncthingHome = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing-home' }
$stExe = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing\syncthing.exe'

$startedAt = (Get-Date).ToString('o')
$steps = New-Object System.Collections.ArrayList
function Add-Step([string]$name, [string]$status, [string]$detail) {
    [void]$steps.Add([ordered]@{ name = $name; status = $status; detail = $detail })
}

# 1) 环境信息
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$svcExe = Join-Path $ArtifactsDir 'bin\classroom-svc.exe'
$guiExe = Join-Path $ArtifactsDir 'bin\classroom-gui.exe'
Add-Step 'env' 'info' ("machine={0} ps={1} admin={2}" -f $env:COMPUTERNAME, $PSVersionTable.PSVersion, $isAdmin)
Add-Step 'artifacts' $(if (Test-Path $svcExe) { 'ok' } else { 'warn' }) ("svcExe={0} guiExe={1}" -f (Test-Path $svcExe), (Test-Path $guiExe))

# 2) 证据目录 + 写权限自检
New-Item -ItemType Directory -Force $EvidenceDir | Out-Null
$probe = Join-Path $EvidenceDir ('.write-probe-{0}.tmp' -f $PID)
try {
    [System.IO.File]::WriteAllText($probe, 'ok', (New-Object System.Text.UTF8Encoding($false)))
    Remove-Item $probe -Force -ErrorAction SilentlyContinue
    Add-Step 'evidence-writable' 'ok' $EvidenceDir
} catch {
    Add-Step 'evidence-writable' 'fail' ("无法写证据目录: {0} ({1})" -f $EvidenceDir, $_.Exception.Message)
    throw "证据目录不可写：$EvidenceDir"
}

# 3) Syncthing（student 角色，证据 Send Only → A Receive Only）
if (-not $SkipSyncthing) {
    $setup = Join-Path $PSScriptRoot 'syncthing-setup.ps1'
    if (-not (Test-Path $setup)) { throw "缺少 syncthing-setup.ps1：$setup" }
    Write-Host '[bootstrap] 配置 Syncthing（student）...'
    & $setup -Role student -PeerDeviceId $TeacherDeviceId -PeerName $TeacherName -PeerAddresses $TeacherAddresses -StHome $SyncthingHome -EvidenceDir $EvidenceDir -ArtifactsDir $ArtifactsDir -GuiPort $GuiPort -SyncPort $SyncPort -LocalDiscoveryPort $LocalDiscoveryPort -LocalOnly 6>&1 | ForEach-Object { Write-Host $_ }
    Add-Step 'syncthing-setup' 'ok' $SyncthingHome
} else {
    Add-Step 'syncthing-setup' 'skipped' 'SkipSyncthing'
}

# 4) 本机 device id
$myDeviceId = ''
if (Test-Path $stExe) {
    try { $myDeviceId = (& $stExe device-id --home="$SyncthingHome" 2>&1 | Select-Object -First 1).Trim() } catch { }
}
Add-Step 'device-id' $(if ($myDeviceId) { 'ok' } else { 'warn' }) $myDeviceId

# 5) 环境自检文件（供 A 机确认 B 就绪）
$envInfo = [ordered]@{
    role = 'student'
    machine = $env:COMPUTERNAME
    user = $env:USERNAME
    admin = $isAdmin
    psVersion = $PSVersionTable.PSVersion.ToString()
    deviceId = $myDeviceId
    artifactsDir = $ArtifactsDir
    evidenceDir = $EvidenceDir
    svcExeExists = (Test-Path $svcExe)
    guiExeExists = (Test-Path $guiExe)
    freeDiskGB = [math]::Round((Get-PSDrive C).Free / 1GB, 1)
    startedAt = $startedAt
    finishedAt = (Get-Date).ToString('o')
    steps = $steps
    verdict = $(if ((Test-Path $svcExe) -and $myDeviceId) { 'pass' } else { 'blocked' })
}
$envFile = Join-Path $EvidenceDir 'student-env.json'
[System.IO.File]::WriteAllText($envFile, ($envInfo | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
Write-Output ($envInfo | ConvertTo-Json -Depth 6)
Write-Host "[bootstrap] 完成：$envFile"

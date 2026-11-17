#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— 学生机（B 机）用例执行器（骨架 v1）。
.DESCRIPTION
  在第二台机器上执行固定用例，输出统一 JSON 证据信封（供 mesh 任务结果回收）。
  设计约束：
    1. 时序测量必须在脚本内完成（跨机 agent 调度延迟不可用于毫秒级判据）；
    2. 只输出原始事实（退出码 / 日志行 / 哈希），verdict 只允许 pass/fail/blocked/not_implemented；
    3. 所有数据根隔离（LOCALAPPDATA/APPDATA/PROGRAMDATA），不污染本机真实数据。
.PARAMETER Case
  用例 id：A1/A2/A3/A4/B1/B2/B3/B4/C1/C2/C4/C5/C6/C7。
.PARAMETER TeacherAddr
  教师机地址（自动发现失败时的兜底 manual_addr，形如 192.168.1.7:47821）。
.PARAMETER SvcExe
  classroom-svc.exe 路径（默认仓库 target\release\classroom-svc.exe）。
.PARAMETER DataRoot
  隔离数据根（默认 $env:TEMP\classshare-dual\student）。
.PARAMETER OutDir
  证据输出目录（默认 <DataRoot>\evidence）。
.EXAMPLE
  .\student-case.ps1 -Case A1 -TeacherAddr 192.168.1.7:47821
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Case,
    [string]$TeacherAddr = '',
    [string]$SvcExe = '',
    [string]$DataRoot = (Join-Path $env:TEMP 'classshare-dual\student'),
    [string]$OutDir = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $SvcExe) { $SvcExe = Join-Path $repoRoot 'target\release\classroom-svc.exe' }
if (-not $OutDir) { $OutDir = Join-Path $DataRoot 'evidence' }
New-Item -ItemType Directory -Force $OutDir | Out-Null
New-Item -ItemType Directory -Force $DataRoot | Out-Null

$script:Steps = New-Object System.Collections.ArrayList
$script:Hashes = @{}
$script:Proc = $null
$startedAt = (Get-Date).ToString('o')

function Add-Step([string]$name, [string]$status, [string]$detail) {
    [void]$script:Steps.Add([ordered]@{ name = $name; status = $status; detail = $detail; at = (Get-Date).ToString('o') })
}

function Save-Evidence([string]$verdict, [string]$reason) {
    $env = [ordered]@{
        case = $Case
        machine = $env:COMPUTERNAME
        startedAt = $startedAt
        finishedAt = (Get-Date).ToString('o')
        verdict = $verdict
        reason = $reason
        steps = $script:Steps
        hashes = $script:Hashes
        dataRoot = $DataRoot
        svcLogTail = @()
    }
    $log = Join-Path $DataRoot 'logs\svc.log'
    if (Test-Path $log) { $env.svcLogTail = @(Get-Content $log -Tail 40) }
    $file = Join-Path $OutDir ("{0}-{1}.json" -f $Case, (Get-Date).ToString('HHmmss'))
    [System.IO.File]::WriteAllText($file, ($env | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
    Write-Output ($env | ConvertTo-Json -Depth 6)
    Write-Host "evidence: $file"
}

function Start-IsoSvc([string]$mode) {
    if (-not (Test-Path $SvcExe)) { throw "svc exe 不存在: $SvcExe（先 scripts/build-gui.ps1 / cargo build --release -p classroom-svc）" }
    $cfg = [ordered]@{ version = 1; mode = $mode; room_code = '' }
    if ($TeacherAddr) { $cfg.manual_addr = $TeacherAddr }
    $cfgDir = Join-Path $DataRoot 'config'
    New-Item -ItemType Directory -Force $cfgDir | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $cfgDir 'svc.json'), ($cfg | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $SvcExe
    $psi.Arguments = '--console'
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.EnvironmentVariables['LOCALAPPDATA'] = (Join-Path $DataRoot 'localappdata')
    $psi.EnvironmentVariables['APPDATA'] = (Join-Path $DataRoot 'appdata')
    $psi.EnvironmentVariables['PROGRAMDATA'] = (Join-Path $DataRoot 'programdata')
    New-Item -ItemType Directory -Force $psi.EnvironmentVariables['LOCALAPPDATA'] | Out-Null
    New-Item -ItemType Directory -Force $psi.EnvironmentVariables['APPDATA'] | Out-Null
    New-Item -ItemType Directory -Force $psi.EnvironmentVariables['PROGRAMDATA'] | Out-Null
    $script:Proc = [System.Diagnostics.Process]::Start($psi)
    Add-Step 'svc-start' 'ok' ("pid={0} mode={1}" -f $script:Proc.Id, $mode)
}

function Wait-SvcLog([string]$pattern, [int]$timeoutSec = 20) {
    $log = Join-Path $DataRoot 'logs\svc.log'
    $deadline = (Get-Date).AddSeconds($timeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $log) {
            $hit = Select-String -Path $log -Pattern $pattern -ErrorAction SilentlyContinue | Select-Object -Last 1
            if ($hit) { Add-Step 'log-wait' 'ok' ("pattern={0} line={1}" -f $pattern, $hit.Line.Trim()); return $hit }
        }
        Start-Sleep -Milliseconds 200
    }
    Add-Step 'log-wait' 'fail' ("pattern={0} timeout={1}s" -f $pattern, $timeoutSec)
    return $null
}

try {
    switch ($Case) {
        'A1' {
            Start-IsoSvc 'student'
            $hit = Wait-SvcLog '已加入' 25
            if ($hit) { Save-Evidence 'pass' '学生端自动发现并加入教师机' }
            else { Save-Evidence 'fail' '超时未见「已加入」日志（检查网络/教师机/防火墙）' }
        }
        'B4' {
            # 进程级：退出 GUI 后服务仍在（需 GUI 已安装/运行）
            $svc = Get-Service ClassShareSvc -ErrorAction SilentlyContinue
            if ($svc -and $svc.Status -eq 'Running') { Add-Step 'svc-running' 'ok' 'ClassShareSvc Running' }
            else { Add-Step 'svc-running' 'fail' 'ClassShareSvc 未运行（服务模式未安装？）' }
            $gui = Get-Process classroom-gui -ErrorAction SilentlyContinue
            if ($gui) { Add-Step 'gui-present' 'info' ("pid={0}" -f ($gui.Id -join ',')) }
            Save-Evidence 'blocked' 'B4 需人工点击右键退出 GUI 后复跑本脚本核对服务/数据'
        }
        default {
            Add-Step 'case' 'not_implemented' ("case {0} 尚未实现（见 docs/acceptance/dual-machine-test-plan.md §4）" -f $Case)
            Save-Evidence 'not_implemented' '骨架未实现该用例；请按方案人工执行或补实现'
        }
    }
}
finally {
    if ($script:Proc -and -not $script:Proc.HasExited) {
        try { $script:Proc.Kill() } catch { }
        Add-Step 'svc-stop' 'ok' '已停止隔离 svc'
    }
}
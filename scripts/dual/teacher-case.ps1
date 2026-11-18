#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— 教师机（A 机）用例执行器（骨架 v1）。
.DESCRIPTION
  启动隔离教师 svc（--console，独立数据根），等待就绪/学生加入，输出 JSON 证据信封。
.PARAMETER Case
  用例 id：A1/A2/A3/A4/B1/B2/B3/C4/C5/C6/C7。
.PARAMETER Room
  房间码（默认 XK-204）。
.PARAMETER Pin
  可选 PIN。
.PARAMETER WaitStudentSec
  等待学生加入的秒数（0=不等待）。
.PARAMETER SvcExe
  classroom-svc.exe 路径（默认仓库 target\release\classroom-svc.exe）。
.PARAMETER DataRoot
  隔离数据根（默认 $env:TEMP\classshare-dual\teacher）。
.EXAMPLE
  .\teacher-case.ps1 -Case A1 -Room XK-204 -WaitStudentSec 60
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Case,
    [string]$Room = 'XK-204',
    [string]$Pin = '',
    [int]$WaitStudentSec = 0,
    [string]$SvcExe = '',
    [string]$DataRoot = (Join-Path $env:TEMP 'classshare-dual\teacher')
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $SvcExe) { $SvcExe = Join-Path $repoRoot 'target\release\classroom-svc.exe' }
$OutDir = Join-Path $DataRoot 'evidence'
New-Item -ItemType Directory -Force $OutDir | Out-Null
New-Item -ItemType Directory -Force $DataRoot | Out-Null

$script:Steps = New-Object System.Collections.ArrayList
$script:Proc = $null
$startedAt = (Get-Date).ToString('o')

function Add-Step([string]$name, [string]$status, [string]$detail) {
    [void]$script:Steps.Add([ordered]@{ name = $name; status = $status; detail = $detail; at = (Get-Date).ToString('o') })
}

function Save-Evidence([string]$verdict, [string]$reason) {
    $evidence = [ordered]@{
        case = $Case
        machine = $env:COMPUTERNAME
        role = 'teacher'
        room = $Room
        startedAt = $startedAt
        finishedAt = (Get-Date).ToString('o')
        verdict = $verdict
        reason = $reason
        steps = $script:Steps
        dataRoot = $DataRoot
        svcLogTail = @()
    }
    $log = Join-Path $DataRoot 'localappdata\ClassShare\data\logs\svc.log'
    if (Test-Path $log) { $evidence.svcLogTail = @(Get-Content $log -Tail 40 | ForEach-Object { [string]$_ }) }
    $file = Join-Path $OutDir ("{0}-teacher-{1}.json" -f $Case, (Get-Date).ToString('HHmmss'))
    [System.IO.File]::WriteAllText($file, ($evidence | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
    Write-Output ($evidence | ConvertTo-Json -Depth 6)
    Write-Host "evidence: $file"
}

function Start-IsoTeacher {
    if (-not (Test-Path $SvcExe)) { throw "svc exe 不存在: $SvcExe（先 cargo build --release -p classroom-svc）" }
    $cfg = [ordered]@{ version = 1; mode = 'teacher'; room_code = $Room }
    if ($Pin) { $cfg.pin = $Pin }
    $cfgDir = Join-Path $DataRoot 'localappdata\ClassShare'
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
    # 姣忔鐢ㄤ緥鍚姩鍓嶆竻绌烘棫鏃ュ織锛岄伩鍏?Wait-SvcLog 鍛戒腑涓婁竴杞畫鐣欒锛堝亣闃虫€э級
    $logFile = Join-Path $psi.EnvironmentVariables['LOCALAPPDATA'] 'ClassShare\data\logs\svc.log'
    if (Test-Path $logFile) { Remove-Item $logFile -Force -ErrorAction SilentlyContinue }
    $script:Proc = [System.Diagnostics.Process]::Start($psi)
    Add-Step 'svc-start' 'ok' ("pid={0} room={1}" -f $script:Proc.Id, $Room)
}

function Wait-SvcLog([string]$pattern, [int]$timeoutSec = 20) {
    $log = Join-Path $DataRoot 'localappdata\ClassShare\data\logs\svc.log'
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
            Start-IsoTeacher
            $ready = Wait-SvcLog '教师端启动|IPC 服务监听中' 20
            if (-not $ready) { Save-Evidence 'fail' '教师 svc 未就绪'; break }
            if ($WaitStudentSec -gt 0) {
                $joined = Wait-SvcLog 'student joined|学员加入' $WaitStudentSec
                if ($joined) { Save-Evidence 'pass' '学生机已加入（教师端日志确认）' }
                else { Save-Evidence 'fail' ("等待 {0}s 未见学生加入" -f $WaitStudentSec) }
            }
            else { Save-Evidence 'pass' '教师 svc 就绪（未等待学生）' }
        }
        'A4' {
            # EX-11：先占用 47821，再启动教师 svc，预期自动换端口
            $listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Any, 47821)
            try {
                $listener.Start(); Add-Step 'port-occupy' 'ok' '47821 已占用'
                Start-IsoTeacher
                $hit = Wait-SvcLog '指定端口被占用|自动更换' 20
                if ($hit) { Save-Evidence 'pass' 'EX-11 自动换端口日志命中' }
                else { Save-Evidence 'fail' '未见端口冲突处理日志' }
            }
            finally { try { $listener.Stop() } catch { } }
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
        Add-Step 'svc-stop' 'ok' '已停止隔离教师 svc'
    }
}
#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— B 机用例执行器（无人值守，一次跑一批用例）。
.DESCRIPTION
  读取 cases.json，按序执行用例（每个用例内部带超时），把每个用例的 JSON 证据写入证据目录，
  最后写汇总报告 dual-report-<RunId>.json 并打印到 stdout（供 mesh 任务结果回收）。
  设计约束：
    - 全程无交互、不弹窗；
    - 单个用例失败/超时不影响后续用例；
    - 只输出原始事实，verdict 由脚本判定。
.PARAMETER Cases
  要执行的用例 id（默认全部）。
.PARAMETER ArtifactsDir
  被测环境包目录（默认本脚本上级目录）。
.PARAMETER EvidenceDir
  证据目录（默认 <ArtifactsDir>\evidence）。
.PARAMETER TeacherAddr
  教师机地址（自动发现失败时的兜底，形如 192.168.1.7:47821）。
.PARAMETER RunId
  本轮标识（默认时间戳），用于报告文件名。
.EXAMPLE
  .\student-runner.ps1 -Cases SELFCHECK,A1 -TeacherAddr 192.168.1.7:47821
#>
[CmdletBinding()]
param(
    [string[]]$Cases = @(),
    [string]$ArtifactsDir = '',
    [string]$EvidenceDir = '',
    [string]$TeacherAddr = '',
    [string]$RunId = '',
    [string]$SvcExe = ''
)

$ErrorActionPreference = 'Stop'
if (-not $ArtifactsDir) { $ArtifactsDir = Split-Path $PSScriptRoot -Parent }
if (-not $EvidenceDir) { $EvidenceDir = Join-Path $ArtifactsDir 'evidence' }
if (-not $RunId) { $RunId = Get-Date -Format 'yyyyMMdd-HHmmss' }
New-Item -ItemType Directory -Force $EvidenceDir | Out-Null

if (-not $SvcExe) {
    $cand = Join-Path $ArtifactsDir 'bin\classroom-svc.exe'
    if (Test-Path $cand) { $SvcExe = $cand }
    else {
        $repo = Split-Path $ArtifactsDir -Parent
        $cand2 = Join-Path $repo 'target\release\classroom-svc.exe'
        if (Test-Path $cand2) { $SvcExe = $cand2 }
    }
}

# 用例清单
$casesFile = @(
    (Join-Path $ArtifactsDir 'cases.json'),
    (Join-Path $PSScriptRoot 'cases.json'),
    (Join-Path (Split-Path $ArtifactsDir -Parent) 'scripts\dual\cases.json')
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $casesFile) { throw "找不到 cases.json（ArtifactsDir=$ArtifactsDir）" }
$manifest = Get-Content $casesFile -Raw -Encoding UTF8 | ConvertFrom-Json
$allCases = @($manifest.cases)
if ($Cases.Count -gt 0) {
    $selected = @()
    foreach ($c in $Cases) {
        $hit = $allCases | Where-Object { $_.id -eq $c } | Select-Object -First 1
        if (-not $hit) { throw "未知用例: $c" }
        $selected += $hit
    }
} else {
    $selected = $allCases
}

$runStartedAt = (Get-Date).ToString('o')
$results = @()
$caseIds = (($selected | ForEach-Object { $_.id }) -join ',')
Write-Host ("[runner] runId={0} cases={1} artifacts={2} evidence={3}" -f $RunId, $caseIds, $ArtifactsDir, $EvidenceDir)

foreach ($case in $selected) {
    $caseId = $case.id
    $caseOut = Join-Path $EvidenceDir ("dual-case-{0}-{1}.json" -f $RunId, $caseId)
    $startedAt = (Get-Date).ToString('o')
    $verdict = 'fail'; $reason = ''; $raw = $null

    try {
        if ($case.mode -eq 'inline') {
            if ($caseId -eq 'SELFCHECK') {
                $envFile = Join-Path $EvidenceDir 'student-env.json'
                $envInfo = $null
                if (Test-Path $envFile) { $envInfo = Get-Content $envFile -Raw -Encoding UTF8 | ConvertFrom-Json }
                $stExe = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing\syncthing.exe'
                $devId = ''
                if (Test-Path $stExe) { try { $devId = (& $stExe device-id --home=(Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing-home') 2>&1 | Select-Object -First 1).Trim() } catch { } }
                $verdict = if ($envInfo -and $envInfo.verdict -eq 'pass') { 'pass' } else { 'blocked' }
                $raw = [ordered]@{
                    case = $caseId; machine = $env:COMPUTERNAME; startedAt = $startedAt
                    verdict = $verdict; reason = $(if ($verdict -eq 'pass') { '环境就绪' } else { 'bootstrap 未通过或缺失' })
                    deviceId = $devId; artifactsDir = $ArtifactsDir; evidenceDir = $EvidenceDir
                    svcExe = $SvcExe; svcExeExists = (Test-Path $SvcExe)
                }
            } else {
                $verdict = 'not_implemented'; $raw = [ordered]@{ case = $caseId; verdict = $verdict; reason = 'inline 用例未实现' }
            }
        } else {
            $scriptPath = Join-Path $PSScriptRoot $case.script
            if (-not (Test-Path $scriptPath)) {
                $scriptPath = Join-Path (Join-Path $ArtifactsDir 'scripts') $case.script
            }
            if (-not (Test-Path $scriptPath)) { throw "缺脚本: $($case.script)" }
            $argList = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $scriptPath)
            if ($case.args) { $argList += $case.args }
            $argList += @('-JsonPath', $caseOut)
            if ($TeacherAddr) { $argList += @('-TeacherAddr', $TeacherAddr) }
            if ($SvcExe) { $argList += @('-SvcExe', $SvcExe) }

            $outLog = Join-Path $EvidenceDir ("dual-case-{0}-{1}.out.txt" -f $RunId, $caseId)
            $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argList -PassThru -WindowStyle Hidden -RedirectStandardOutput $outLog -RedirectStandardError ($outLog + '.err')
            $timeout = [int]($case.timeoutSec)
            if (-not $p.WaitForExit($timeout * 1000)) {
                try { $p.Kill() } catch { }
                $verdict = 'fail'; $reason = "用例超时（${timeout}s）"
            } else {
                if (Test-Path $caseOut) {
                    $raw = Get-Content $caseOut -Raw -Encoding UTF8 | ConvertFrom-Json
                    $verdict = [string]$raw.verdict
                    $reason = if ($raw.reason) { [string]$raw.reason } else { '' }
                } else {
                    $verdict = 'fail'; $reason = '用例未产出证据 JSON'
                }
            }
        }
    } catch {
        $verdict = 'fail'; $reason = $_.Exception.Message
    }

    if (-not $raw) {
        $raw = [ordered]@{ case = $caseId; machine = $env:COMPUTERNAME; startedAt = $startedAt; verdict = $verdict; reason = $reason }
        [System.IO.File]::WriteAllText($caseOut, ($raw | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
    }
    $results += [ordered]@{
        id = $caseId
        title = $case.title
        verdict = $verdict
        reason = $reason
        evidence = (Split-Path $caseOut -Leaf)
        startedAt = $startedAt
        finishedAt = (Get-Date).ToString('o')
    }
    Write-Host ("[runner] {0} => {1}" -f $caseId, $verdict)
}

$failCount = @($results | Where-Object { $_.verdict -in @('fail', 'blocked') }).Count
$overall = if ($failCount -gt 0) { 'fail' } else { 'pass' }
$report = [ordered]@{
    runId = $RunId
    role = 'student'
    machine = $env:COMPUTERNAME
    startedAt = $runStartedAt
    finishedAt = (Get-Date).ToString('o')
    artifactsDir = $ArtifactsDir
    evidenceDir = $EvidenceDir
    teacherAddr = $TeacherAddr
    overall = $overall
    failCount = $failCount
    cases = $results
}
$reportFile = Join-Path $EvidenceDir ("dual-report-{0}.json" -f $RunId)
[System.IO.File]::WriteAllText($reportFile, ($report | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))
Write-Output ($report | ConvertTo-Json -Depth 8)
Write-Host "[runner] report: $reportFile"
if ($overall -ne 'pass') { exit 1 }

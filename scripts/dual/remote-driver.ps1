#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— A 机（教师）驱动：准备环境、生成 B 机任务文本、等待证据回流、汇总报告。
.DESCRIPTION
  无人值守流程（B 机只有 DSH，没有人工）：
    1. 导出「被测环境包」（二进制 + 脚本 + cases.json），经 Syncthing 同步到 B 机；
    2. 打印给 B 机的 mesh 任务文本（由 Lead agent 调用 mesh_run 派发）；
    3. 等待 B 机把 dual-report-<RunId>.json 写进证据目录（Syncthing 自动回流）；
    4. 校验报告并生成汇总 dual-report-merged-<RunId>.json。
  -LocalStudent 可在本机模拟 B 机执行，用于框架自测（不需要第二台机器）。
.PARAMETER Cases
  用例 id 列表（默认 SELFCHECK）。
.PARAMETER StudentMachine
  mesh 里 B 机的名字（用于生成派发文本）。
.PARAMETER PeerDeviceId
  B 机 Syncthing Device ID（用于 A 机配置对端与等待证据）。
.PARAMETER RunId
  本轮标识（默认时间戳），用于报告文件名。
.PARAMETER ArtifactsDir
  A 机导出的环境包目录（默认 %LOCALAPPDATA%\ClassShareDual\artifacts）。
.PARAMETER EvidenceDir
  A 机证据目录（默认 <repo>\docs\acceptance\evidence\dual，Syncthing Receive Only）。
.PARAMETER LocalEvidenceDir
  -LocalStudent 模拟 B 机时使用的证据目录（默认 %LOCALAPPDATA%\ClassShareDual\evidence-b）。
.PARAMETER WaitSec
  等待证据回流秒数（默认 600）。
.PARAMETER LocalStudent
  在本机模拟 B 机执行（框架自测）。
.PARAMETER SkipExport
  跳过环境包导出。
.PARAMETER DispatchOnly
  只打印派发文本，不等待证据。
.EXAMPLE
  .\remote-driver.ps1 -Cases SELFCHECK,A1 -PeerDeviceId <B device id> -DispatchOnly
.EXAMPLE
  .\remote-driver.ps1 -Cases SELFCHECK -LocalStudent
#>
[CmdletBinding()]
param(
    [string[]]$Cases = @('SELFCHECK'),
    [string]$StudentMachine = '<B机mesh名>',
    [string]$PeerDeviceId = '',
    [string[]]$PeerAddresses = @('dynamic'),
    [string]$RunId = '',
    [string]$ArtifactsDir = '',
    [string]$EvidenceDir = '',
    [string]$LocalEvidenceDir = '',
    [int]$WaitSec = 600,
    [switch]$LocalStudent,
    [switch]$SkipExport,
    [switch]$DispatchOnly
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $ArtifactsDir) { $ArtifactsDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual\artifacts' }
if (-not $EvidenceDir) { $EvidenceDir = Join-Path $repoRoot 'docs\acceptance\evidence\dual' }
if (-not $LocalEvidenceDir) { $LocalEvidenceDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual\evidence-b' }
if (-not $RunId) { $RunId = Get-Date -Format 'yyyyMMdd-HHmmss' }
New-Item -ItemType Directory -Force $EvidenceDir, $LocalEvidenceDir | Out-Null

$stExe = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing\syncthing.exe'
$stHome = Join-Path $env:LOCALAPPDATA 'ClassShareDual\syncthing-home'
$aDeviceId = ''
if (Test-Path $stExe) {
    try { $aDeviceId = (& $stExe device-id --home="$stHome" 2>&1 | Select-Object -First 1).Trim() } catch { }
}

Write-Host '=== [1/4] 导出被测环境包 ===' -ForegroundColor Cyan
if ($SkipExport) { Write-Host '[skip] -SkipExport' }
else {
    & (Join-Path $PSScriptRoot 'export-artifacts.ps1') -OutDir $ArtifactsDir | Out-Null
    Write-Host "[ok] artifacts=$ArtifactsDir"
}

Write-Host '=== [2/4] 启动/确认 A 机 Syncthing ===' -ForegroundColor Cyan
if (Test-Path (Join-Path $PSScriptRoot 'syncthing-setup.ps1')) {
    $setArgs = @{ Role = 'teacher'; StHome = $stHome; EvidenceDir = $EvidenceDir; LocalOnly = $true }
    if ($PeerDeviceId) { $setArgs['PeerDeviceId'] = $PeerDeviceId; $setArgs['PeerName'] = 'B-Student'; $setArgs['PeerAddresses'] = $PeerAddresses }
    & (Join-Path $PSScriptRoot 'syncthing-setup.ps1') @setArgs 6>&1 | ForEach-Object { Write-Host $_ }
    Write-Host "[ok] A device id=$aDeviceId"
} else {
    Write-Warning '缺 syncthing-setup.ps1，跳过 A 机 Syncthing 配置'
}

Write-Host '=== [3/4] B 机任务（由 Lead agent 通过 mesh_run 派发）===' -ForegroundColor Cyan
$ids = ($Cases -join ',')
$bArtifacts = '%LOCALAPPDATA%\ClassShareDual\artifacts'
$bootstrapCmd = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$bArtifacts\scripts\bootstrap-student.ps1`" -TeacherDeviceId $aDeviceId -TeacherName '$env:COMPUTERNAME'"
$runnerCmd = "powershell -NoProfile -ExecutionPolicy Bypass -File `"$bArtifacts\scripts\student-runner.ps1`" -Cases $ids -RunId $RunId"
if ($PeerDeviceId) { $runnerCmd += " -PeerDeviceId $PeerDeviceId" }
$taskText = "在 B 机执行（无人值守，无需人工）：`n1) 首次运行：$bootstrapCmd`n2) 本轮执行：$runnerCmd`n只回传 student-runner stdout 的 JSON 与 dual-report-$RunId.json 路径；不要改写 verdict。"
Write-Host ''
Write-Host '--- mesh_run 参数 ---' -ForegroundColor Yellow
Write-Host ("peer  : {0}" -f $StudentMachine)
Write-Host ("thread: dual-{0}" -f $RunId)
Write-Host ("task  :`n{0}" -f $taskText)
Write-Host ''

if ($DispatchOnly) {
    Write-Host '[dispatch-only] 不等待证据回流。' -ForegroundColor Yellow
    $plan = [ordered]@{
        runId = $RunId; cases = $Cases; aDeviceId = $aDeviceId; studentMachine = $StudentMachine
        artifactsDir = $ArtifactsDir; evidenceDir = $EvidenceDir; dispatchTask = $taskText
        generatedAt = (Get-Date).ToString('o')
    }
    [System.IO.File]::WriteAllText((Join-Path $ArtifactsDir ("dispatch-plan-{0}.json" -f $RunId)), ($plan | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
    return
}

Write-Host '=== [4/4] 等待 B 机报告回流 ===' -ForegroundColor Cyan
if ($LocalStudent) {
    Write-Host '[sim] 本机模拟 B 机执行 student-runner ...'
    $runner = Join-Path $ArtifactsDir 'scripts\student-runner.ps1'
    if (-not (Test-Path $runner)) { $runner = Join-Path $PSScriptRoot 'student-runner.ps1' }
    $simArgs = @{ Cases = $Cases; ArtifactsDir = $ArtifactsDir; EvidenceDir = $LocalEvidenceDir; RunId = $RunId; TeacherAddr = '127.0.0.1:47821' }
    & $runner @simArgs 6>&1 | ForEach-Object { Write-Host $_ }
}

$reportName = "dual-report-$RunId.json"
$reportPath = Join-Path $EvidenceDir $reportName
$deadline = (Get-Date).AddSeconds($WaitSec)
while ((Get-Date) -lt $deadline -and -not (Test-Path $reportPath)) {
    Start-Sleep -Seconds 1
}
if (-not (Test-Path $reportPath)) {
    Write-Host ("[fail] {0}s 内未收到 {1}" -f $WaitSec, $reportName) -ForegroundColor Red
    Write-Host '排查：① B 机 mesh 可达？② B 机 Syncthing 连上 A？③ 证据目录是否同步？'
    exit 2
}

$report = Get-Content $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
$merged = [ordered]@{
    runId = $RunId
    receivedAt = (Get-Date).ToString('o')
    aDeviceId = $aDeviceId
    student = [ordered]@{ machine = $report.machine; role = $report.role; artifactsDir = $report.artifactsDir }
    overall = $report.overall
    failCount = $report.failCount
    cases = $report.cases
    reportFile = $reportName
}
$mergedPath = Join-Path $EvidenceDir ("dual-report-merged-{0}.json" -f $RunId)
[System.IO.File]::WriteAllText($mergedPath, ($merged | ConvertTo-Json -Depth 8), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Write-Host ("=== 汇总：overall={0} failCount={1} cases={2} ===" -f $report.overall, $report.failCount, @($report.cases).Count) -ForegroundColor $(if ($report.overall -eq 'pass') { 'Green' } else { 'Red' })
foreach ($c in $report.cases) { Write-Host ("  {0,-10} {1,-14} {2}" -f $c.id, $c.verdict, $c.reason) }
Write-Host "[ok] merged report: $mergedPath"
if ($report.overall -ne 'pass') { exit 1 }

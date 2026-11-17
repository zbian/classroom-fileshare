#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试编排器（A 机运行）：校验前置条件、生成本地执行计划与 mesh 派发任务模板。
.DESCRIPTION
  说明：mesh 派发由 Lead agent 调用 mesh_run 完成（脚本无法直接调用 agent 工具）。
  本脚本负责：
    1. 前置条件校验（仓库/tag/构建产物/端口/防火墙提示）；
    2. 为每个用例打印「教师机命令」与「学生机 mesh 任务文本」；
    3. -LocalStudent：在本机用隔离数据根跑学生脚本（单机预演，验证脚本本身可用）。
.PARAMETER Cases
  用例 id 列表（默认 A1,B1,C4,C5）。
.PARAMETER StudentMachine
  mesh 里的学生机名（用于生成派发文本）。
.PARAMETER TeacherAddr
  学生机访问教师机的地址（默认本机局域网 IP:47821）。
.PARAMETER LocalStudent
  在本机执行 student-case.ps1（预演；教师侧需另起 teacher-case.ps1）。
.EXAMPLE
  .\run-dual.ps1 -Cases A1,B1,C4 -StudentMachine Bian-学生机
#>
[CmdletBinding()]
param(
    [string[]]$Cases = @('A1', 'B1', 'C4', 'C5'),
    [string]$StudentMachine = '<学生机 mesh 名称>',
    [string]$TeacherAddr = '',
    [switch]$LocalStudent
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$svcExe = Join-Path $repoRoot 'target\release\classroom-svc.exe'
$guiExe = Join-Path $repoRoot 'apps\classroom-gui\src-tauri\target\release\classroom-gui.exe'

Write-Host '=== 双机测试前置检查 ===' -ForegroundColor Cyan
$ok = $true
function Check([string]$name, [bool]$cond, [string]$hint) {
    if ($cond) { Write-Host ("  [OK]   {0}" -f $name) -ForegroundColor Green }
    else { Write-Host ("  [MISS] {0}  -> {1}" -f $name, $hint) -ForegroundColor Yellow; $script:ok = $false }
}

$tag = (git -C $repoRoot describe --tags --exact-match 2>$null)
if (-not $tag) { $tag = (git -C $repoRoot rev-parse --short HEAD 2>$null) }
Check '仓库' (Test-Path (Join-Path $repoRoot '.git')) '在仓库根运行'
Check '版本' ($null -ne $tag) 'git describe 失败'
Check 'classroom-svc.exe' (Test-Path $svcExe) '先 cargo build --release -p classroom-svc'
Check 'classroom-gui.exe' (Test-Path $guiExe) '先 scripts/build-gui.ps1'
$udp = Get-NetUDPEndpoint -LocalPort 47820 -ErrorAction SilentlyContinue
Check '本机 47820 未被占用' ($null -eq $udp) '停止已有 classroom-svc / 换机测试'
if (-not $TeacherAddr) {
    $ip = (Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress -notmatch '^127\.' -and $_.PrefixOrigin -ne 'WellKnown' } | Select-Object -First 1).IPAddress
    if ($ip) { $TeacherAddr = "$ip`:47821" }
}
Write-Host ("  版本={0}  教师地址={1}" -f $tag, $TeacherAddr)

Write-Host ''
Write-Host '=== 执行计划（教师侧本机执行；学生侧由 mesh 派发）===' -ForegroundColor Cyan
foreach ($case in $Cases) {
    Write-Host ("-- {0} --" -f $case) -ForegroundColor White
    Write-Host ("  教师机: .\scripts\dual\teacher-case.ps1 -Case {0}" -f $case)
    $task = "在 {0} 上执行：cd <repo>; .\scripts\dual\student-case.ps1 -Case {1} -TeacherAddr {2}；只回传脚本 stdout 的 JSON 与 evidence 文件路径，不要改写 verdict。" -f $StudentMachine, $case, $TeacherAddr
    Write-Host ("  mesh 派发文本: {0}" -f $task)
    Write-Host ''
}

if ($LocalStudent) {
    Write-Host '=== 本机预演 student-case.ps1（需要教师侧已启动）===' -ForegroundColor Cyan
    $addr = '127.0.0.1:47821'
    foreach ($case in $Cases) {
        Write-Host ("-- 预演 {0} --" -f $case)
        & (Join-Path $PSScriptRoot 'student-case.ps1') -Case $case -TeacherAddr $addr
    }
}

if (-not $ok) { Write-Host '前置检查未全通过，先补齐再执行双机测试。' -ForegroundColor Yellow; exit 2 }
Write-Host '前置检查通过。按上面计划执行；mesh 派发由 Lead agent 调用 mesh_run 完成。' -ForegroundColor Green
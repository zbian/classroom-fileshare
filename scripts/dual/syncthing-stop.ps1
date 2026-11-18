#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— 停止 Syncthing 进程。
.DESCRIPTION
  停止本仓库双机测试用到的 Syncthing 进程（按 home 路径匹配），不触碰系统其它 Syncthing。
.PARAMETER StHome
  只停该 home 对应的进程（默认 %LOCALAPPDATA%\ClassShareDual\syncthing-home）。
.PARAMETER All
  停掉 %LOCALAPPDATA%\ClassShareDual 下所有 Syncthing 进程（多实例/本机预演用）。
.EXAMPLE
  .\syncthing-stop.ps1
.EXAMPLE
  .\syncthing-stop.ps1 -All
#>
[CmdletBinding()]
param(
    [string]$StHome = '',
    [switch]$All
)

$ErrorActionPreference = 'Stop'
$baseDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual'
if (-not $StHome) { $StHome = Join-Path $baseDir 'syncthing-home' }

$procs = Get-CimInstance Win32_Process -Filter "Name='syncthing.exe'" -ErrorAction SilentlyContinue
if ($All) {
    $procs = $procs | Where-Object { $_.CommandLine -and $_.CommandLine -like "*$baseDir*" }
} else {
    $procs = $procs | Where-Object { $_.CommandLine -and $_.CommandLine -like "*$StHome*" }
}

if (-not $procs) {
    Write-Host '[stop] 无匹配的 Syncthing 进程'
    return
}

foreach ($p in $procs) {
    try {
        Stop-Process -Id $p.ProcessId -Force -ErrorAction Stop
        Write-Host "[stop] pid=$($p.ProcessId) 已停止"
    } catch {
        Write-Warning "停止 pid=$($p.ProcessId) 失败：$($_.Exception.Message)"
    }
}

Start-Sleep -Milliseconds 500
$left = Get-CimInstance Win32_Process -Filter "Name='syncthing.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and $_.CommandLine -like "*$baseDir*" }
if ($left) { Write-Host "[warn] 仍有 $($left.Count) 个进程" -ForegroundColor Yellow } else { Write-Host '[ok] 已全部停止' -ForegroundColor Green }

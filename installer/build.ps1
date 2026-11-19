#Requires -Version 5.1
<#
.SYNOPSIS
  构建 NSIS 单文件安装包（ClassShare-Setup.exe）。
.DESCRIPTION
  前置：makensis（NSIS 3.x）已安装；target\release 下已有
  classroom-svc.exe / classroom-gui.exe / classshare-config.exe（-Build 可代建）。
  产物：installer\ClassShare-Setup.exe（原生安装界面，运行时不需要任何脚本）。
.EXAMPLE
  .\installer\build.ps1 -Build
#>
[CmdletBinding()]
param([switch]$Build)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

if ($Build) {
    Write-Host '[build] 前端 ...'
    & pnpm --dir (Join-Path $repo 'apps\classroom-gui\ui') build | Out-Null
    Write-Host '[build] classroom-svc ...'
    & cargo build --release -p classroom-svc | Out-Null
    Write-Host '[build] classroom-gui ...'
    & cargo build --release -p classroom-gui --features custom-protocol | Out-Null
    Write-Host '[build] classshare-config ...'
    & cargo build --release -p classshare-config | Out-Null
}

$bin = Join-Path $PSScriptRoot 'bin'
New-Item -ItemType Directory -Force $bin | Out-Null
foreach ($name in 'classroom-svc.exe', 'classroom-gui.exe', 'classshare-config.exe') {
    $src = Join-Path $repo "target\release\$name"
    if (-not (Test-Path $src)) { throw "缺少 $src（先构建或加 -Build）" }
    Copy-Item $src $bin -Force
}

$makensis = @(
    'C:\Program Files (x86)\NSIS\makensis.exe',
    'C:\Program Files\NSIS\makensis.exe'
) | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $makensis) {
    $cmd = Get-Command makensis -ErrorAction SilentlyContinue
    if ($cmd) { $makensis = $cmd.Source }
}
if (-not $makensis) { throw '未找到 makensis（安装 NSIS：winget install NSIS.NSIS）' }

& $makensis '/V2' (Join-Path $PSScriptRoot 'classshare.nsi')
if ($LASTEXITCODE -ne 0) { throw "makensis 退出码 $LASTEXITCODE" }

$out = Join-Path $PSScriptRoot 'ClassShare-Setup.exe'
$item = Get-Item $out
Write-Host ''
Write-Host '=== 构建完成 ==='
Write-Host ("文件: {0}" -f $item.FullName)
Write-Host ("大小: {0:N0} 字节" -f $item.Length)
Write-Host ("SHA-256: {0}" -f (Get-FileHash $out -Algorithm SHA256).Hash.ToLower())

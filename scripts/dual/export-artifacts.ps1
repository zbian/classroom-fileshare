#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— A 机导出「被测环境包」（二进制 + 脚本 + 用例清单）。
.DESCRIPTION
  B 机（无人值守、可能没有仓库/工具链）只需要接收这个目录即可运行用例：
    artifacts/
      bin/classroom-svc.exe, classroom-gui.exe
      scripts/*.ps1, cases.json
      artifacts-manifest.json
  目录通过 Syncthing 同步到 B 机（见 docs/acceptance/dual-machine-test-plan.md §6.6）。
.PARAMETER OutDir
  输出目录（默认 %LOCALAPPDATA%\ClassShareDual\artifacts）。
.PARAMETER SkipBinaries
  只导出脚本（B 机已有二进制时）。
.EXAMPLE
  .\export-artifacts.ps1
#>
[CmdletBinding()]
param(
    [string]$OutDir = '',
    [switch]$SkipBinaries
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not $OutDir) { $OutDir = Join-Path $env:LOCALAPPDATA 'ClassShareDual\artifacts' }

$scriptDir = Join-Path $OutDir 'scripts'
$binDir = Join-Path $OutDir 'bin'
New-Item -ItemType Directory -Force $OutDir, $scriptDir, $binDir | Out-Null

# 1) 脚本 + 用例清单
Get-ChildItem $PSScriptRoot -Filter '*.ps1' | ForEach-Object {
    Copy-Item $_.FullName (Join-Path $scriptDir $_.Name) -Force
}
Copy-Item (Join-Path $PSScriptRoot 'cases.json') (Join-Path $OutDir 'cases.json') -Force
Write-Host "[export] scripts -> $scriptDir"

# 2) 二进制
$version = (git -C $repoRoot describe --tags --always 2>$null)
if (-not $version) { $version = 'unknown' }
$bins = @()
if (-not $SkipBinaries) {
    $candidates = @(
        @{ name = 'classroom-svc.exe'; path = (Join-Path $repoRoot 'target\release\classroom-svc.exe') },
        @{ name = 'classroom-gui.exe'; path = (Join-Path $repoRoot 'target\release\classroom-gui.exe') }
    )
    foreach ($c in $candidates) {
        if (Test-Path $c.path) {
            Copy-Item $c.path (Join-Path $binDir $c.name) -Force
            $hash = (Get-FileHash $c.path -Algorithm SHA256).Hash
            $bins += [ordered]@{ name = $c.name; sha256 = $hash; size = (Get-Item $c.path).Length }
            Write-Host "[export] $($c.name) $([math]::Round((Get-Item $c.path).Length/1MB,1)) MB"
        } else {
            Write-Warning "二进制缺失（先构建）: $($c.path)"
        }
    }
}

# 3) manifest
$scripts = @()
Get-ChildItem $scriptDir -Filter '*.ps1' | ForEach-Object {
    $scripts += [ordered]@{ name = $_.Name; sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash; size = $_.Length }
}
$manifest = [ordered]@{
    version = $version
    generatedAt = (Get-Date).ToString('o')
    machine = $env:COMPUTERNAME
    casesSha256 = (Get-FileHash (Join-Path $OutDir 'cases.json') -Algorithm SHA256).Hash
    binaries = $bins
    scripts = $scripts
}
[System.IO.File]::WriteAllText((Join-Path $OutDir 'artifacts-manifest.json'), ($manifest | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
Write-Output ($manifest | ConvertTo-Json -Depth 6)
Write-Host "[export] out=$OutDir version=$version"

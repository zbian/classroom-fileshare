#Requires -Version 5.1
<#
.SYNOPSIS
  校验 ClassShare 安装结果（评审 B1/M5/M6 回归检查）。
.DESCRIPTION
  检查项：
  1. 服务 ClassShareSvc Running
  2. HKLM 右键菜单键（Default/Icon/MultiSelectModel/command）
  3. HKLM Run 中 GUI 自启项（带 --role）
  4. HKCU 旧版菜单/Run 已清理（rc.9 及更早安装会写 HKCU）
  5. 拖放临时目录 %TEMP%\ClassShareDrop 无残留（M3）
.EXAMPLE
  .\scripts\verify-installer-registry.ps1
  .\scripts\verify-installer-registry.ps1 -Json
#>
[CmdletBinding()]
param([switch]$Json, [switch]$AfterUninstall)

$ErrorActionPreference = 'Continue'
$checks = New-Object System.Collections.ArrayList

function Add-Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    [void]$checks.Add([pscustomobject]@{ Name = $Name; Ok = $Ok; Detail = $Detail })
}

if ($AfterUninstall) {
    $svc = Get-Service -Name 'ClassShareSvc' -ErrorAction SilentlyContinue
    Add-Check '服务已卸载' ($null -eq $svc)
    Add-Check 'HKLM 菜单键已删除' (-not (Test-Path -LiteralPath 'HKLM:\Software\Classes\*\shell\课堂共享'))
    $run = Get-ItemProperty -LiteralPath 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'ClassShareGUI' -ErrorAction SilentlyContinue
    Add-Check 'HKLM Run 已删除' ($null -eq $run)
    Add-Check 'HKCU 旧菜单已删除' (-not (Test-Path -LiteralPath 'HKCU:\Software\Classes\*\shell\课堂共享'))
    $legacy = (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue)
    Add-Check 'HKCU Run 已删除' (-not $legacy.ClassShareGUI)
    $failed = @($checks | Where-Object { -not $_.Ok })
    if ($Json) {
        [pscustomobject]@{ ok = ($failed.Count -eq 0); checks = $checks } | ConvertTo-Json -Depth 4
    } else {
        $checks | ForEach-Object {
            $mark = if ($_.Ok) { 'PASS' } else { 'FAIL' }
            Write-Host ("[{0}] {1}{2}" -f $mark, $_.Name, $(if ($_.Detail) { " — $($_.Detail)" } else { '' }))
        }
        Write-Host ''
        if ($failed.Count -eq 0) { Write-Host '结果: ALL PASS'; exit 0 } else { Write-Host "结果: $($failed.Count) 项失败"; exit 1 }
    }
    return
}

# 1) 服务
$svc = Get-Service -Name 'ClassShareSvc' -ErrorAction SilentlyContinue
Add-Check '服务 ClassShareSvc' ($null -ne $svc -and $svc.Status -eq 'Running') $(if ($svc) { "$($svc.Status)" } else { '未安装' })

# 2) 右键菜单（HKLM；* 通配需 -LiteralPath）
$shellKey = 'HKLM:\Software\Classes\*\shell\课堂共享'
$shellExists = Test-Path -LiteralPath $shellKey
Add-Check 'HKLM 右键菜单键存在' $shellExists $shellKey
if ($shellExists) {
    $default = (Get-ItemProperty -LiteralPath $shellKey -Name '(default)' -ErrorAction SilentlyContinue).'(default)'
    Add-Check '菜单名称=课堂共享' ($default -eq '课堂共享') "$default"
    $icon = (Get-ItemProperty -LiteralPath $shellKey -Name 'Icon' -ErrorAction SilentlyContinue).Icon
    Add-Check '菜单 Icon 指向 GUI' ($icon -like '*classroom-gui.exe') "$icon"
    $multi = (Get-ItemProperty -LiteralPath $shellKey -Name 'MultiSelectModel' -ErrorAction SilentlyContinue).MultiSelectModel
    Add-Check 'MultiSelectModel=Player（多选）' ($multi -eq 'Player') "$multi"
    $cmd = (Get-ItemProperty -LiteralPath "$shellKey\command" -Name '(default)' -ErrorAction SilentlyContinue).'(default)'
    Add-Check '菜单命令含 gui.exe 且传 %*' (($cmd -like '*classroom-gui.exe*') -and $cmd.Contains('%*')) "$cmd"
}

# 3) HKLM Run 自启
$run = Get-ItemProperty -LiteralPath 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'ClassShareGUI' -ErrorAction SilentlyContinue
Add-Check 'HKLM Run ClassShareGUI 存在且带 --role' ($null -ne $run -and $run.ClassShareGUI -like '*--role=*') $(if ($run) { $run.ClassShareGUI } else { '缺失' })

# 4) HKCU 旧键清理
$legacyMenu = Test-Path -LiteralPath 'HKCU:\Software\Classes\*\shell\课堂共享'
Add-Check 'HKCU 旧版菜单已清理' (-not $legacyMenu) $(if ($legacyMenu) { '仍存在' } else { '无' })
$legacyRun = (Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -ErrorAction SilentlyContinue)
Add-Check 'HKCU 旧版 GUI 自启已清理' (-not $legacyRun.ClassShareGUI) $(if ($legacyRun) { "$($legacyRun.ClassShareGUI)" } else { '无' })

# 5) 拖放临时目录残留
$dropRoot = Join-Path $env:TEMP 'ClassShareDrop'
$leftover = @(Get-ChildItem -LiteralPath $dropRoot -ErrorAction SilentlyContinue).Count
Add-Check '拖放临时目录无残留' ($leftover -eq 0) "剩余 $leftover 项"

$failed = @($checks | Where-Object { -not $_.Ok })
if ($Json) {
    [pscustomobject]@{ ok = ($failed.Count -eq 0); checks = $checks } | ConvertTo-Json -Depth 4
} else {
    $checks | ForEach-Object {
        $mark = if ($_.Ok) { 'PASS' } else { 'FAIL' }
        Write-Host ("[{0}] {1}{2}" -f $mark, $_.Name, $(if ($_.Detail) { " — $($_.Detail)" } else { '' }))
    }
    Write-Host ''
    if ($failed.Count -eq 0) { Write-Host '结果: ALL PASS'; exit 0 } else { Write-Host "结果: $($failed.Count) 项失败"; exit 1 }
}

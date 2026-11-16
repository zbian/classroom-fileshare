#Requires -Version 5.1
<#
.SYNOPSIS
  课堂共享卸载脚本（PRD 9.5、TC-13 前半）。

.DESCRIPTION
  注销：Windows 服务、SCM failure 策略（随服务删除）、防火墙规则、
  HKCU Run 自启项（GUI 与降级 svc）、资源管理器右键扩展注册项（尽力清理）。
  共享数据默认保留；-CleanData 显式删除（PROGRAMDATA 与 LOCALAPPDATA）。
#>
[CmdletBinding()]
param([switch]$CleanData)
$ErrorActionPreference = 'Stop'

# 1) 服务（含 failure 策略，随服务注销）
$svc = Get-Service -Name ClassShareSvc -ErrorAction SilentlyContinue
if ($svc) {
    if ($svc.Status -ne 'Stopped') {
        sc.exe stop ClassShareSvc | Out-Null
        Start-Sleep -Seconds 2
    }
    sc.exe delete ClassShareSvc | Out-Null
    Write-Host '已注销服务: ClassShareSvc'
} else {
    Write-Host '服务未注册（跳过）'
}

# 2) 防火墙规则（幂等）
netsh advfirewall firewall delete rule name='ClassShare Service TCP' 2>$null | Out-Null
netsh advfirewall firewall delete rule name='ClassShare Discovery UDP' 2>$null | Out-Null
Write-Host '已清理防火墙规则（若有）'

# 3) 自启项（GUI 与降级 svc）
reg.exe delete 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v ClassShareSvc /f 2>$null | Out-Null
reg.exe delete 'HKCU\Software\Microsoft\Windows\CurrentVersion\Run' /v ClassShareGUI /f 2>$null | Out-Null
Write-Host '已清理 HKCU Run 自启项（若有）'

# 4) 资源管理器右键扩展注册项（dev-gui T-12 注册；键名尽力覆盖两种候选）
reg.exe delete 'HKCU\Software\Classes\*\shell\ClassShare' /f 2>$null | Out-Null
reg.exe delete 'HKCU\Software\Classes\*\shell\课堂共享' /f 2>$null | Out-Null
Write-Host '已清理右键扩展注册项（若有）'

# 5) 数据：默认保留（PRD 9.5）
if ($CleanData) {
    Remove-Item -Recurse -Force (Join-Path $env:ProgramData 'ClassShare') -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force (Join-Path $env:LOCALAPPDATA 'ClassShare') -ErrorAction SilentlyContinue
    Write-Host '已按 -CleanData 删除共享数据（PROGRAMDATA 与 LOCALAPPDATA）'
} else {
    Write-Host '共享数据默认保留；如需清除请执行: .\uninstall.ps1 -CleanData'
}

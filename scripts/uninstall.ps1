#Requires -Version 5.1
<#
.SYNOPSIS
  课堂共享卸载脚本（PRD 9.5、TC-13 前半）。

.DESCRIPTION
  注销：Windows 服务、SCM failure 策略（随服务删除）、防火墙规则、
  HKCU Run 自启项（GUI 与降级 svc）、资源管理器右键扩展注册项（尽力清理）。
  共享数据默认保留；-CleanData 显式删除（PROGRAMDATA 与 LOCALAPPDATA）。

.NOTES
  M1 验收 O-3：原实现用 reg.exe / netsh 并配 `2>$null`，在
  `$ErrorActionPreference = 'Stop'` 下 PS 5.1 会把原生 stderr 变成
  NativeCommandError 抛出，验收时表现为「卸载中断 + stderr 噪音」。
  现改为：注册表一律用 PowerShell 原生 cmdlet（-LiteralPath，天然静默且
  不受 `*` 通配符影响）；必须调用的原生命令经 Invoke-Quiet 统一吞掉输出。
#>
[CmdletBinding()]
param([switch]$CleanData)
$ErrorActionPreference = 'Stop'

# 静默执行原生命令：临时放宽错误策略并吞掉 stdout/stderr（避免 EAP=Stop 时
# 原生命令的 stderr 变成 NativeCommandError 异常）。
function Invoke-Quiet {
    param([scriptblock]$Command)
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $Command 2>&1 | Out-Null }
    finally { $ErrorActionPreference = $previous }
}

# 1) 服务（含 failure 策略，随服务注销）
$svc = Get-Service -Name ClassShareSvc -ErrorAction SilentlyContinue
if ($svc) {
    if ($svc.Status -ne 'Stopped') {
        Invoke-Quiet { sc.exe stop ClassShareSvc }
        Start-Sleep -Seconds 2
    }
    Invoke-Quiet { sc.exe delete ClassShareSvc }
    Write-Host '已注销服务: ClassShareSvc'
} else {
    Write-Host '服务未注册（跳过）'
}

# 2) 防火墙规则（幂等）
Invoke-Quiet { netsh advfirewall firewall delete rule name='ClassShare Service TCP' }
Invoke-Quiet { netsh advfirewall firewall delete rule name='ClassShare Discovery UDP' }
Write-Host '已清理防火墙规则（若有）'

# 3) 自启项（GUI 与降级 svc）——原生注册表 cmdlet，缺失即静默跳过
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
foreach ($valueName in @('ClassShareSvc', 'ClassShareGUI')) {
    if (Test-Path -LiteralPath $runKey) {
        Remove-ItemProperty -LiteralPath $runKey -Name $valueName -ErrorAction SilentlyContinue
    }
}
Write-Host '已清理 HKCU Run 自启项（若有）'

# 4) 资源管理器右键扩展注册项（dev-gui T-12 注册；键名尽力覆盖两种候选）
#    注意 -LiteralPath：键路径含 `*`（Software\Classes\*\shell），非 Literal 会
#    按通配遍历整个 Classes 树（实测长时间无响应）。
foreach ($shellKey in @(
        'HKCU:\Software\Classes\*\shell\ClassShare',
        'HKCU:\Software\Classes\*\shell\课堂共享'
    )) {
    if (Test-Path -LiteralPath $shellKey) {
        Remove-Item -LiteralPath $shellKey -Recurse -Force -ErrorAction SilentlyContinue
    }
}
Write-Host '已清理右键扩展注册项（若有）'

# 5) 数据：默认保留（PRD 9.5）
if ($CleanData) {
    Remove-Item -LiteralPath (Join-Path $env:ProgramData 'ClassShare') -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $env:LOCALAPPDATA 'ClassShare') -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host '已按 -CleanData 删除共享数据（PROGRAMDATA 与 LOCALAPPDATA）'
} else {
    Write-Host '共享数据默认保留；如需清除请执行: .\uninstall.ps1 -CleanData'
}

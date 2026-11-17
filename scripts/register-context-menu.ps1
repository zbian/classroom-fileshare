# 课堂共享 · 资源管理器右键扩展注册（HKCU，免管理员，PRD 3.7 / T-12）
# 用法：
#   register-context-menu.ps1            # 自动找 target\debug|release 下的 exe
#   register-context-menu.ps1 -ExePath <path>
#   register-context-menu.ps1 -Remove    # 清理（与 T-13 uninstall.ps1 键名一致）
#
# 注意：键路径含通配字符 `*`（Software\Classes\*\shell），所有「读取/写值」
# 必须 -LiteralPath，否则 Set-ItemProperty 按通配遍历整个 Classes 树会长时间
# 无响应（实测）。New-Item 创建语义按字面量处理，-Path 即可。
param(
  [string]$ExePath = "",
  [switch]$Remove
)
$ErrorActionPreference = 'Stop'
$KeyName = '课堂共享'
$key = 'HKCU:\Software\Classes\*\shell\' + $KeyName

function Resolve-Exe {
  param([string]$P)
  if ($P -ne '') { return (Resolve-Path -LiteralPath $P).Path }
  $candidates = @(
    (Join-Path $PSScriptRoot '..\target\debug\classroom-gui.exe'),
    (Join-Path $PSScriptRoot '..\target\release\classroom-gui.exe')
  )
  foreach ($c in $candidates) { if (Test-Path -LiteralPath $c) { return (Resolve-Path -LiteralPath $c).Path } }
  throw '未找到 classroom-gui.exe，请用 -ExePath 指定'
}

if ($Remove) {
  Remove-Item -LiteralPath $key -Recurse -Force -ErrorAction SilentlyContinue
  Write-Host '已清理右键扩展（HKCU\Software\Classes\*\shell\课堂共享）'
  exit 0
}

$exe = Resolve-Exe $ExePath
# 多选经 argv 传单实例（single-instance 插件合并后走直发链路）；%* 展开全部选中路径
$cmd = "`"$exe`" `"%*`""

New-Item -Path $key -Force | Out-Null
Set-ItemProperty -LiteralPath $key -Name '(Default)' -Value '课堂共享'
Set-ItemProperty -LiteralPath $key -Name 'Icon' -Value $exe
New-Item -Path "$key\command" -Force | Out-Null
Set-ItemProperty -LiteralPath "$key\command" -Name '(Default)' -Value $cmd
$check = (Get-ItemProperty -LiteralPath "$key\command").'(Default)'
if ($check -ne $cmd) { throw '注册校验失败' }
Write-Host "已注册右键扩展：文件右键 →「课堂共享」（exe=$exe）"
Write-Host '清理：register-context-menu.ps1 -Remove'

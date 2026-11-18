#Requires -Version 5.1
<#
.SYNOPSIS
  双机测试 —— 最小 IPC 客户端（向 classroom-svc 发请求，用于无 GUI 的内容分发测试）。
.DESCRIPTION
  IPC 为行分帧 JSON（crates/ipc/src/codec.rs）：
    {"Handshake":{"protocol_version":1,"token":"..."}}
    {"Request":{"id":1,"req":{"SendText":{"title":"...","body":"..."}}}}
  服务端回 {"Reply":{"id":1,"result":{"Ok":"Ok"}}} 或 {"Err":"..."}。
.PARAMETER Action
  text / file / delete
.PARAMETER PipeName
  管道名（console 教师：classshare-svc-teacher；学生：classshare-svc-student）
.PARAMETER TokenPath
  ipc.token 路径（默认 %LOCALAPPDATA%\ClassShare\data\ipc.token）
.EXAMPLE
  .\ipc-client.ps1 -Action text -Title hello -Body world
#>
[CmdletBinding()]
param(
    [ValidateSet('text', 'file', 'delete')][string]$Action = 'text',
    [string]$Title = '',
    [string]$Body = '',
    [string]$Path = '',
    [string]$Id = '',
    [string]$PipeName = 'classshare-svc-teacher',
    [string]$TokenPath = '',
    [int]$TimeoutMs = 15000
)

$ErrorActionPreference = 'Stop'
if (-not $TokenPath) { $TokenPath = Join-Path $env:LOCALAPPDATA 'ClassShare\data\ipc.token' }
if (-not (Test-Path $TokenPath)) { throw "ipc.token 不存在: $TokenPath" }
$token = (Get-Content $TokenPath -Raw).Trim()

$client = New-Object System.IO.Pipes.NamedPipeClientStream('.', $PipeName, [System.IO.Pipes.PipeDirection]::InOut)
$client.Connect($TimeoutMs)
$enc = New-Object System.Text.UTF8Encoding($false)
$writer = New-Object System.IO.StreamWriter($client, $enc)
$writer.NewLine = "`n"
$writer.AutoFlush = $true
$reader = New-Object System.IO.StreamReader($client, $enc)

try {
    $hs = @{ Handshake = @{ protocol_version = 1; token = $token } } | ConvertTo-Json -Compress
    $writer.WriteLine($hs)
    $resp = $reader.ReadLine()
    if ($null -eq $resp) { throw '握手无响应' }
    if ($resp -notmatch 'HandshakeOk') { throw "握手被拒: $resp" }

    $req = switch ($Action) {
        'text' { @{ SendText = @{ title = $Title; body = $Body } } }
        'file' { @{ SendFiles = @{ paths = @($Path) } } }
        'delete' { @{ DeleteItem = @{ id = $Id } } }
    }
    $msg = @{ Request = @{ id = 1; req = $req } } | ConvertTo-Json -Compress -Depth 8
    $writer.WriteLine($msg)
    $reply = $reader.ReadLine()
    Write-Output ('REPLY=' + $reply)
} finally {
    try { $client.Dispose() } catch { }
}

#Requires -Version 5.1
<#
.SYNOPSIS
  把本仓库的版本控制整体迁移到 Gitee（推送 main+标签，重配远端）。
.DESCRIPTION
  默认行为：
    1. 校验仓库与远端状态；
    2. 把 main 分支 + 全部标签推送到 Gitee（-MirrorAll 可推送全部本地分支）；
    3. origin 重命名为 github，新增 origin=<GiteeUrl>，并设置 main 跟踪；
    4. 校验 Gitee 侧 refs；
    5. -RemoveGitHub 时删除 github 远端（默认保留作镜像/历史参考）。
  认证：
    - SSH：本机公钥需已加到 Gitee 账户（git@gitee.com:<user>/<repo>.git）
    - HTTPS 私人令牌：-Token 传入，仅用于本次推送 URL，不写入 git 配置
  注意：不使用 GitHub Actions；迁移不涉及 CI。
.PARAMETER GiteeUrl
  Gitee 仓库地址，如 https://gitee.com/zbian/classroom-fileshare.git 或 git@gitee.com:zbian/classroom-fileshare.git
.PARAMETER Token
  Gitee 私人令牌（HTTPS 推送用；也可用于 -CreateRepo 调 API 建仓）
.PARAMETER CreateRepo
  仓库不存在时用 API 创建（需要 -Token；名称取自 GiteeUrl 的路径）
.PARAMETER Private
  -CreateRepo 时创建私有仓库（默认私有）
.PARAMETER MirrorAll
  除 main+标签外，推送全部本地分支
.PARAMETER RemoveGitHub
  迁移完成后删除 github 远端
.PARAMETER DryRun
  只打印将执行的命令，不实际推送
.EXAMPLE
  .\scripts\migrate-to-gitee.ps1 -GiteeUrl git@gitee.com:zbian/classroom-fileshare.git -DryRun
.EXAMPLE
  .\scripts\migrate-to-gitee.ps1 -GiteeUrl https://gitee.com/zbian/classroom-fileshare.git -Token $env:GITEE_TOKEN -CreateRepo
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$GiteeUrl,
    [string]$Token = '',
    [switch]$CreateRepo,
    [switch]$Private,
    [switch]$MirrorAll,
    [switch]$RemoveGitHub,
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Set-Location $repo

function Invoke-Git {
    param([string[]]$Arguments)
    $shown = $Arguments -join ' '
    if ($DryRun) { Write-Host "[dry-run] git $shown"; return }
    # PS 5.1 + EAP=Stop 下，原生命令 stderr（Gitee 推送横幅）会变成
    # NativeCommandError 终止脚本——临时放宽并捕获，按退出码判定成败。
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & git @Arguments 2>&1 | ForEach-Object { Write-Host $_ } } finally { $ErrorActionPreference = $prev }
    if ($LASTEXITCODE -ne 0) { throw "git $shown 失败（exit $LASTEXITCODE）" }
}

Write-Host '=== [1/5] 仓库状态 ==='
$top = (& git rev-parse --show-toplevel).Trim()
$topNorm = $top -replace '\\', '/'; $repoNorm = $repo -replace '\\', '/'; if ($topNorm -ne $repoNorm) { throw "当前目录不是仓库根：$top" }
$current = (& git rev-parse --abbrev-ref HEAD).Trim()
$head = (& git rev-parse --short HEAD).Trim()
Write-Host "仓库: $repo"
Write-Host "分支: $current @ $head"
$dirty = (& git status --porcelain)
if ($dirty) { Write-Warning "工作区有未提交改动（$($dirty.Count) 项）——迁移不受影响，但请确认内容已提交到 main" }

$hasGithub = (& git remote) -contains 'github'
$hasOrigin = (& git remote) -contains 'origin'
$originUrl = if ($hasOrigin) { (& git remote get-url origin).Trim() } else { '' }
Write-Host "现有 origin: $originUrl"

# --- 解析 Gitee 地址 ---
$isSsh = $GiteeUrl -match '^git@gitee\.com:'
$pushUrl = $GiteeUrl
if ($Token -and -not $isSsh) {
    # 令牌只用于本次推送，不落 git 配置
    $pushUrl = $GiteeUrl -replace '^https://', "https://oauth2:$Token@"
}

Write-Host ''
Write-Host '=== [2/5] 创建 Gitee 仓库（按需） ==='
$owner = ''
$repoName = ''
if ($GiteeUrl -match 'gitee\.com[:/](?<owner>[^/]+)/(?<name>[^/]+?)(?:\.git)?$') {
    $owner = $Matches['owner']; $repoName = $Matches['name']
}
Write-Host "目标: $owner/$repoName"
if ($CreateRepo) {
    if (-not $Token) { throw '-CreateRepo 需要 -Token（Gitee 私人令牌）' }
    if ($DryRun) {
        Write-Host "[dry-run] POST https://gitee.com/api/v5/user/repos (name=$repoName, private=$([bool]$Private))"
    } else {
        $body = @{ name = $repoName; private = [bool]$Private; auto_init = $false; description = '课堂共享（ClassShare）' } | ConvertTo-Json
        try {
            $created = Invoke-RestMethod -Method Post -Uri "https://gitee.com/api/v5/user/repos?access_token=$Token" -Body $body -ContentType 'application/json; charset=utf-8' -TimeoutSec 30
            Write-Host "已创建 Gitee 仓库: $($created.full_name)（private=$($created.private)）"
        } catch {
            Write-Warning "创建失败（可能已存在或令牌权限不足）：$($_.Exception.Message)"
        }
    }
}

Write-Host ''
Write-Host '=== [3/5] 推送 main + 标签 ==='
Invoke-Git @('push', $pushUrl, "$current`:$current", '--force')
Invoke-Git @('push', $pushUrl, '--tags')
if ($MirrorAll) {
    Write-Host '[mirror] 推送全部本地分支 ...'
    Invoke-Git @('push', $pushUrl, '--all', '--force')
} else {
    Write-Host '（仅 main+标签；如需全部 111 个本地分支加 -MirrorAll）'
}

Write-Host ''
Write-Host '=== [4/5] 重配远端 ==='
if ($hasOrigin -and $originUrl -match 'github\.com') {
    if (-not $hasGithub) { Invoke-Git @('remote', 'rename', 'origin', 'github') }
    else { Invoke-Git @('remote', 'set-url', 'github', $originUrl) }
    Write-Host 'github 远端已保留为: github'
}
$existingRemotes = (& git remote)
if ($existingRemotes -contains 'origin') { Invoke-Git @('remote', 'set-url', 'origin', $GiteeUrl) }
else { Invoke-Git @('remote', 'add', 'origin', $GiteeUrl) }
Invoke-Git @('fetch', 'origin', '--prune')
Invoke-Git @('branch', "--set-upstream-to=origin/$current", $current)
if ($RemoveGitHub) {
    if ((& git remote) -contains 'github') { Invoke-Git @('remote', 'remove', 'github') }
}

Write-Host ''
Write-Host '=== [5/5] 校验 ==='
if ($DryRun) {
    Write-Host '[dry-run] 跳过远端校验'
} else {
    $heads = (& git ls-remote --heads origin)
    $tags = (& git ls-remote --tags origin)
    Write-Host "Gitee 远端分支数: $($heads.Count)，标签数: $($tags.Count)"
    Write-Host "origin 现在指向: $((& git remote get-url origin).Trim())"
}

Write-Host ''
Write-Host '=== 完成 ==='
Write-Host '后续手工项：'
Write-Host '  1) 更新 README/DEVELOPMENT 中的仓库地址（github.com → gitee.com）'
Write-Host '  2) 需要时把 GitHub open issues 摘要迁到 Gitee（本脚本不迁移 issue）'
Write-Host '  3) 如不再使用 GitHub，可执行: git remote remove github'

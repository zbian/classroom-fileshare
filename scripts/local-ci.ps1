#Requires -Version 5.1
<#
.SYNOPSIS
  classroom-fileshare 本机 CI 门禁（GHA 停用后的权威实现）。

.DESCRIPTION
  2026-11-17 用户指令：停用 GitHub Actions，改用本机 CI。本脚本串联 6 道门禁，
  任一步失败立即停止并以非 0 退出；每步打印 PASS/FAIL 与耗时，最后给出汇总与
  总耗时。门禁口径与 qa 的 docs/acceptance/local-ci.md 保持一致。

  门禁（按依赖顺序；前端构建须先于 GUI release clippy，后者需要 ui/dist）：
    1. PS BOM 守卫   —— 全仓受版本控制 .ps1 必须 UTF-8 BOM；.json 数据不得带 BOM
                        （双面规则；Windows PowerShell 5.1 对无 BOM 脚本按 ANSI/GBK
                         误读中文——已三次踩坑，见 install/uninstall/register-context-menu）
    2. cargo fmt --all --check
    3. cargo clippy --workspace --all-targets -- -D warnings
    4. cargo test --workspace
    5. pnpm install --frozen-lockfile + pnpm build（icon/main/tips 三入口）
    6. GUI release clippy —— cargo clippy --release -p classroom-gui
                            --features custom-protocol -- -D warnings
       （debug clippy 看不到 #[cfg(not(debug_assertions))] 分支的盲区守卫，
         #77 终审 / #79 引入；要求 ui/dist 存在，缺失时用临时占位并自动清理）

.PARAMETER SkipBom
  跳过门禁 1（PS BOM / JSON 双面规则）
.PARAMETER SkipFmt
  跳过门禁 2（fmt）
.PARAMETER SkipClippy
  跳过门禁 3（debug clippy；不影响门禁 6）
.PARAMETER SkipTest
  跳过门禁 4（workspace 测试）
.PARAMETER SkipWeb
  跳过门禁 5（pnpm install/build）。同时跳过门禁 6（release clippy 需要真实
  dist；需要单独跑的场合用 -SkipWeb -ForceReleaseClippy）
.PARAMETER SkipReleaseClippy
  跳过门禁 6
.PARAMETER ForceReleaseClippy
  与 -SkipWeb 同用时仍执行门禁 6（用临时占位 ui/dist）
.PARAMETER Quick
  快捷迭代：BOM + fmt（秒级）

.EXAMPLE
  .\scripts\local-ci.ps1                 # 全量 6 道门禁（PR 前必跑）
  .\scripts\local-ci.ps1 -Quick          # 快速：BOM + fmt
  .\scripts\local-ci.ps1 -SkipWeb        # 后端全量（跳过前端与 release clippy）
  .\scripts\local-ci.ps1 -SkipTest -SkipWeb   # 只 lint
#>
[CmdletBinding()]
param(
    [switch]$SkipBom,
    [switch]$SkipFmt,
    [switch]$SkipClippy,
    [switch]$SkipTest,
    [switch]$SkipWeb,
    [switch]$SkipReleaseClippy,
    [switch]$ForceReleaseClippy,
    [switch]$Quick
)
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path $PSScriptRoot -Parent
Set-Location -LiteralPath $repoRoot
$uiDir = Join-Path $repoRoot 'apps\classroom-gui\ui'

# ---- Quick 预设 ------------------------------------------------------------
if ($Quick) {
    $SkipClippy = $true
    $SkipTest = $true
    $SkipWeb = $true
    $SkipReleaseClippy = $true
}

# ---- 工具链定位 ------------------------------------------------------------
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path -LiteralPath $cargoBin) { $env:Path += ";$cargoBin" }
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo not found. Install Rust or add %USERPROFILE%\.cargo\bin to PATH.'
}
$needWeb = -not $SkipWeb
$needReleaseClippy = -not $SkipReleaseClippy -and (-not $SkipWeb -or $ForceReleaseClippy)
if ($needWeb -and -not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    throw 'pnpm not found（门禁 5 需要 Node 24 + pnpm 11；或用 -SkipWeb 跳过）。'
}

# ---- 执行框架 --------------------------------------------------------------
# 关键踩点：PowerShell 5.1 在 $ErrorActionPreference='Stop' 下，原生命令写
# stderr（cargo/pnpm 的进度行）会变成 NativeCommandError 终止脚本。Invoke-Step
# 内把 EAP 降为 Continue，失败判定只依赖 Assert-ExitCode 显式 throw。
$results = New-Object System.Collections.ArrayList
$totalSw = [Diagnostics.Stopwatch]::StartNew()
$placeholderDist = $null

function Assert-ExitCode {
    param([Parameter(Mandatory = $true)][string]$What)
    if ($LASTEXITCODE -ne 0) { throw "$What 失败 (exit $LASTEXITCODE)" }
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][scriptblock]$Action
    )
    Write-Host ''
    Write-Host "==> $Name" -ForegroundColor Cyan
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $ok = $false
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        # *>&1 合并全部流后逐行字符串化输出：PS 5.1 会把原生命令（cargo/pnpm）
        # 的 stderr 进度行包装成 ErrorRecord，直接 Out-Host 会显示为红色错误噪音。
        $stepOutput = & $Action *>&1
        foreach ($line in $stepOutput) { Write-Host ([string]$line) }
        $ok = $true
    }
    catch {
        Write-Host "    失败: $($_.Exception.Message)" -ForegroundColor Red
        $ok = $false
    }
    finally {
        $ErrorActionPreference = $prevEap
        $sw.Stop()
    }
    $status = if ($ok) { 'PASS' } else { 'FAIL' }
    $color = if ($ok) { 'Green' } else { 'Red' }
    Write-Host ("    {0}  ({1:N1}s)" -f $status, $sw.Elapsed.TotalSeconds) -ForegroundColor $color
    [void]$results.Add([pscustomobject]@{
            Step    = $Name
            Status  = $status
            Seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        })
    return $ok
}

function Exit-WithSummary {
    param([string]$Reason)
    $totalSw.Stop()
    if ($placeholderDist -and (Test-Path -LiteralPath $placeholderDist)) {
        Remove-Item -Recurse -Force $placeholderDist -ErrorAction SilentlyContinue
    }
    Write-Host ''
    Write-Host '================ 本机 CI 汇总 ================' -ForegroundColor Cyan
    foreach ($r in $results) {
        $color = if ($r.Status -eq 'PASS') { 'Green' } else { 'Red' }
        Write-Host ("  {0,-56} {1,-5} {2,6:N1}s" -f $r.Step, $r.Status, $r.Seconds) -ForegroundColor $color
    }
    $skipped = @()
    if ($SkipBom) { $skipped += 'BOM' }
    if ($SkipFmt) { $skipped += 'fmt' }
    if ($SkipClippy) { $skipped += 'debug-clippy' }
    if ($SkipTest) { $skipped += 'test' }
    if ($SkipWeb) { $skipped += 'web' }
    if (-not $needReleaseClippy) { $skipped += 'release-clippy' }
    if ($skipped.Count -gt 0) { Write-Host ("  已跳过: {0}" -f ($skipped -join ', ')) -ForegroundColor DarkYellow }
    Write-Host ("  总耗时: {0:N1}s" -f $totalSw.Elapsed.TotalSeconds)
    if ($Reason) {
        Write-Host ("  结果: FAILED —— {0}" -f $Reason) -ForegroundColor Red
        exit 1
    }
    Write-Host '  结果: ALL PASS' -ForegroundColor Green
    exit 0
}

# ---- 门禁 1：PS BOM 守卫（双面规则）----------------------------------------
if (-not $SkipBom) {
    $ok = Invoke-Step 'PS BOM 守卫（.ps1 必须有 BOM / .json 不得有 BOM）' {
        $root = $repoRoot
        $psFiles = @(git ls-files -- '*.ps1')
        $jsonFiles = @(git ls-files -- '*.json')
        $bad = @()
        foreach ($rel in $psFiles) {
            $p = Join-Path $root $rel
            if (-not (Test-Path -LiteralPath $p)) {
                Write-Host "    跟踪文件缺失: $rel" -ForegroundColor Red
                $bad += $rel
                continue
            }
            $b = [IO.File]::ReadAllBytes($p)
            $hasBom = ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF)
            if (-not $hasBom) {
                Write-Host "    PS 脚本缺 UTF-8 BOM（PS 5.1 会按 ANSI 误读中文）: $rel" -ForegroundColor Red
                $bad += $rel
            }
        }
        foreach ($rel in $jsonFiles) {
            $p = Join-Path $root $rel
            if (-not (Test-Path -LiteralPath $p)) { continue }
            $b = [IO.File]::ReadAllBytes($p)
            if ($b.Length -ge 3 -and $b[0] -eq 0xEF -and $b[1] -eq 0xBB -and $b[2] -eq 0xBF) {
                Write-Host "    JSON 数据带 BOM（应去除，serde_json 解析失败）: $rel" -ForegroundColor Red
                $bad += $rel
            }
        }
        Write-Host "    受版本控制 .ps1 $($psFiles.Count) 个 / .json $($jsonFiles.Count) 个"
        if ($bad.Count -gt 0) { throw "BOM 守卫发现 $($bad.Count) 个违规文件" }
    }
    if (-not $ok) { Exit-WithSummary 'PS BOM 守卫失败' }
}

# ---- 门禁 2：fmt -----------------------------------------------------------
if (-not $SkipFmt) {
    $ok = Invoke-Step 'cargo fmt --all --check' {
        cargo fmt --all --check
        Assert-ExitCode 'cargo fmt --all --check（修复：cargo fmt --all）'
    }
    if (-not $ok) { Exit-WithSummary 'fmt 门禁失败' }
}

# ---- 门禁 3：debug clippy --------------------------------------------------
if (-not $SkipClippy) {
    $ok = Invoke-Step 'cargo clippy --workspace --all-targets -- -D warnings' {
        cargo clippy --workspace --all-targets -- -D warnings
        Assert-ExitCode 'cargo clippy（debug）'
    }
    if (-not $ok) { Exit-WithSummary 'debug clippy 门禁失败' }
}

# ---- 门禁 4：workspace 测试 ------------------------------------------------
if (-not $SkipTest) {
    $ok = Invoke-Step 'cargo test --workspace' {
        $log = Join-Path ([IO.Path]::GetTempPath()) ("local-ci-test-{0}.log" -f [guid]::NewGuid())
        # 输出写文件而非管道：PS 5.1 对原生命令 stderr 的 NativeCommandError
        # 双写会污染控制台；失败时打印日志尾部、成功时只报合计（贴 PR 友好）
        cargo test --workspace *> $log
        if ($LASTEXITCODE -ne 0) {
            Get-Content -LiteralPath $log -Tail 120 | ForEach-Object { Write-Host ([string]$_) }
            throw "cargo test 失败 (exit $LASTEXITCODE)；完整日志: $log"
        }
        $passed = 0; $failed = 0
        foreach ($m in [regex]::Matches((Get-Content -LiteralPath $log -Raw), 'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed')) {
            $passed += [int]$m.Groups[1].Value
            $failed += [int]$m.Groups[2].Value
        }
        Write-Host "    合计通过 $passed / 失败 $failed"
        if ($failed -ne 0) { throw "存在 $failed 项失败用例（日志: $log）" }
    }
    if (-not $ok) { Exit-WithSummary '测试门禁失败' }
}

# ---- 门禁 5：前端安装与构建（三入口）---------------------------------------
if ($needWeb) {
    $ok = Invoke-Step 'pnpm install --frozen-lockfile + pnpm build（icon/main/tips）' {
        Push-Location -LiteralPath $uiDir
        try {
            pnpm install --frozen-lockfile
            Assert-ExitCode 'pnpm install --frozen-lockfile'
            pnpm build
            Assert-ExitCode 'pnpm build'
            $dist = Join-Path $uiDir 'dist'
            if (-not (Test-Path -LiteralPath $dist)) { throw "构建产物缺失: $dist" }
        }
        finally { Pop-Location }
    }
    if (-not $ok) { Exit-WithSummary '前端构建门禁失败' }
}

# ---- 门禁 6：GUI release clippy（debug 盲区守卫）---------------------------
if ($needReleaseClippy) {
    $ok = Invoke-Step 'GUI release clippy（custom-protocol，-D warnings）' {
        $dist = Join-Path $uiDir 'dist'
        $createdPlaceholder = $false
        if (-not (Test-Path -LiteralPath $dist)) {
            # tauri generate_context! 要求 frontendDist 目录存在；本门禁只做
            # lint、不产出 GUI 产物，故占位即可（构建门禁未跑时的快速路径）
            New-Item -ItemType Directory -Force -Path $dist | Out-Null
            Set-Content -LiteralPath (Join-Path $dist 'index.html') -Value '<!doctype html><title>local-ci placeholder</title>'
            $script:placeholderDist = $dist
            $createdPlaceholder = $true
            Write-Host '    ui/dist 不存在，已创建临时占位（结束自动清理）'
        }
        try {
            cargo clippy --release -p classroom-gui --features custom-protocol -- -D warnings
            Assert-ExitCode 'cargo clippy（release / custom-protocol）'
        }
        finally {
            if ($createdPlaceholder -and (Test-Path -LiteralPath $dist)) {
                Remove-Item -Recurse -Force $dist -ErrorAction SilentlyContinue
                Write-Host '    临时占位 ui/dist 已清理'
            }
        }
    }
    if (-not $ok) { Exit-WithSummary 'release clippy 门禁失败' }
}

Exit-WithSummary ''

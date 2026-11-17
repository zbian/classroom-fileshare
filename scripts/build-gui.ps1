#Requires -Version 5.1
<#
.SYNOPSIS
  Classroom Share GUI production build (embeds frontend assets).

.DESCRIPTION
  Tauri decides "dev mode" from whether the `tauri` crate was compiled with the
  `custom-protocol` feature (tauri-build `is_dev()` reads DEP_TAURI_DEV). That is
  independent of the cargo profile, so a bare `cargo build --release` still
  compiles in dev mode: tauri-codegen then skips asset embedding and the binary
  loads `build.devUrl` (http://localhost:1517) at runtime. Without a dev server
  that shows a blank window -- this is M1 acceptance finding O-1.

  This script is the correct production entry point:
    1) pnpm build  (vite -> apps/classroom-gui/ui/dist)
    2) cargo build --release -p classroom-gui --features custom-protocol

.PARAMETER SkipFrontend
  Reuse the existing ui/dist instead of rebuilding it.

.EXAMPLE
  .\scripts\build-gui.ps1
  .\scripts\build-gui.ps1 -SkipFrontend
#>
[CmdletBinding()]
param(
    [switch]$SkipFrontend
)
$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path $PSScriptRoot -Parent
$uiDir = Join-Path $repoRoot 'apps\classroom-gui\ui'

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path -LiteralPath $cargoBin) { $env:Path += ";$cargoBin" }
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo not found. Install Rust or add %USERPROFILE%\.cargo\bin to PATH.'
}

if (-not $SkipFrontend) {
    Write-Host '[1/2] pnpm build (frontend -> ui/dist)'
    if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
        throw 'pnpm not found. Install Node 24 + pnpm 11 (see README).'
    }
    Push-Location -LiteralPath $uiDir
    try {
        pnpm build
        if ($LASTEXITCODE -ne 0) { throw "pnpm build failed (exit $LASTEXITCODE)" }
    }
    finally { Pop-Location }
}
else {
    Write-Host '[1/2] skipped frontend build (-SkipFrontend)'
}

$dist = Join-Path $uiDir 'dist'
if (-not (Test-Path -LiteralPath $dist)) {
    throw "frontend dist not found: $dist -- run without -SkipFrontend"
}

Write-Host '[2/2] cargo build --release --features custom-protocol'
Push-Location -LiteralPath $repoRoot
try {
    cargo build --release -p classroom-gui --features custom-protocol
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed (exit $LASTEXITCODE)" }
}
finally { Pop-Location }

$exe = Join-Path $repoRoot 'target\release\classroom-gui.exe'
if (-not (Test-Path -LiteralPath $exe)) { throw "build output missing: $exe" }
Write-Host ''
Write-Host "OK: $exe"
Write-Host 'This binary embeds ui/dist and does not need a dev server.'

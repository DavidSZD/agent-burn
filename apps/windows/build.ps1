<#
.SYNOPSIS
    Script de compilation automatisé pour Agent Burn Windows.
#>
param (
    [switch]$Release
)

$ErrorActionPreference = "Stop"

Write-Host "==> Vérification des prérequis de build Windows..." -ForegroundColor Cyan

# 1. Vérification de Rust / Cargo
$cargoBin = Join-Path $HOME ".cargo\bin"
if ((Test-Path (Join-Path $cargoBin "cargo.exe")) -and ($env:PATH -notlike "*$cargoBin*")) {
    $env:PATH = "$cargoBin;" + $env:PATH
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Warning "Cargo n'est pas installé ou non présent dans le PATH."
    Write-Host "Pour installer Rust sous Windows : winget install Rustlang.Rustup" -ForegroundColor Yellow
    exit 1
}

# 2. Vérification de WebView2
$wv2 = Get-ItemProperty -Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
if (-not $wv2) {
    Write-Host "Microsoft Edge WebView2 est requis." -ForegroundColor Yellow
}

# 3. Compilation du binaire CLI si manquant
$rootDir = (Resolve-Path "$PSScriptRoot\..\..").Path
$cliBin = Join-Path $rootDir "rust\target\release\agent-burn.exe"
if (-not (Test-Path $cliBin)) {
    Write-Host "==> Compilation du binaire natif agent-burn..." -ForegroundColor Cyan
    cargo build --manifest-path (Join-Path $rootDir "rust\Cargo.toml") --release --bin agent-burn
}

# 4. Compilation de l'application Tauri
Write-Host "==> Lancement de l'application Tauri Windows..." -ForegroundColor Cyan
Push-Location $PSScriptRoot
try {
    if ($Release) {
        npx --yes @tauri-apps/cli build
    } else {
        npx --yes @tauri-apps/cli dev
    }
} finally {
    Pop-Location
}

<#
.SYNOPSIS
    Non-destructive validation helper for the Windows installer update path.

.DESCRIPTION
    The script snapshots the Agent Burn data directory, runs a supplied
    production installer with /UPDATE, and verifies that settings, report
    cache, and quota history still exist afterwards. It never deletes the
    original data directory; the backup is kept for manual comparison.
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$InstallerPath,
    [string]$DataDir = (Join-Path $env:LOCALAPPDATA "Agent Burn"),
    [int]$TimeoutSec = 120
)

$ErrorActionPreference = "Stop"
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$data = (Resolve-Path -LiteralPath $DataDir -ErrorAction SilentlyContinue)
if (-not $data) {
    throw "Agent Burn data directory not found: $DataDir"
}

$backup = Join-Path ([IO.Path]::GetTempPath()) ("agent-burn-update-{0}" -f (Get-Date -Format "yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Path $backup | Out-Null
Copy-Item -LiteralPath $data.Path -Destination (Join-Path $backup "Agent Burn") -Recurse

$beforeFiles = @(
    "settings.json",
    "report-cache.json",
    "quota-archive.json",
    "usage-journal.json",
    "metrics-history.json",
    "antigravity-plan.json"
) | ForEach-Object {
    $path = Join-Path $data.Path $_
    $exists = Test-Path -LiteralPath $path
    [pscustomobject]@{
        Name = $_
        Exists = $exists
        Length = if ($exists) { (Get-Item -LiteralPath $path).Length } else { 0 }
        Hash = if ($exists) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { $null }
    }
}

Write-Host "Running $installer /UPDATE ..." -ForegroundColor Cyan
$process = Start-Process -FilePath $installer -ArgumentList "/UPDATE" -PassThru
if (-not $process.WaitForExit($TimeoutSec * 1000)) {
    try { $process.Kill() } catch { }
    throw "Installer timed out after $TimeoutSec seconds. Backup: $backup"
}
if ($process.ExitCode -ne 0) {
    throw "Installer returned exit code $($process.ExitCode). Backup: $backup"
}

foreach ($file in $beforeFiles | Where-Object Exists) {
    $afterPath = Join-Path $data.Path $file.Name
    if (-not (Test-Path -LiteralPath $afterPath)) {
        throw "Update removed $($file.Name). Backup: $backup"
    }
    if ((Get-Item -LiteralPath $afterPath).Length -eq 0 -and $file.Length -gt 0) {
        throw "Update truncated $($file.Name). Backup: $backup"
    }
    if ($file.Name -eq "settings.json") {
        $afterHash = (Get-FileHash -LiteralPath $afterPath -Algorithm SHA256).Hash
        if ($afterHash -ne $file.Hash) {
            throw "Update changed settings.json. Backup: $backup"
        }
    }
    try {
        Get-Content -LiteralPath $afterPath -Raw | ConvertFrom-Json | Out-Null
    } catch {
        throw "Update left invalid JSON in $($file.Name). Backup: $backup"
    }
}

Write-Host "UPDATE validation passed. Data preserved. Backup: $backup" -ForegroundColor Green

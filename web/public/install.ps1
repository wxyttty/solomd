# SoloMD installer for Windows — https://solomd.app
#
# Usage (PowerShell):
#   irm https://solomd.app/install.ps1 | iex
#
# Downloads the latest .msi installer and runs it interactively.
# For silent install: set $env:SOLOMD_SILENT=1 before running.

$ErrorActionPreference = 'Stop'
$repo = "zhitongblog/solomd"

function Write-Step($msg) {
    Write-Host "==> " -ForegroundColor Yellow -NoNewline
    Write-Host $msg
}

Write-Step "Fetching latest SoloMD release from GitHub..."
try {
    $latest = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -ErrorAction Stop
} catch {
    Write-Host "Error: failed to reach GitHub API. Check your internet connection." -ForegroundColor Red
    exit 1
}
$tag = $latest.tag_name
$version = $tag -replace '^v', ''
Write-Host "Latest version: $tag"

# Pick the architecture. OSArchitecture is the honest answer: on Windows on
# Arm, PROCESSOR_ARCHITECTURE reads AMD64 whenever PowerShell itself is running
# emulated, so trusting it hands an Arm machine the x64 build.
$arch = 'x64'
try {
    if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { $arch = 'arm64' }
} catch {
    # Very old PowerShell: fall back to the environment, checking the
    # WOW64 variable first since it survives emulation.
    $env_arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($env_arch -eq 'ARM64') { $arch = 'arm64' }
}
Write-Host "Architecture: $arch"

# MSI is the only supported installed Windows channel.
$asset = $latest.assets | Where-Object { $_.name -like "SoloMD_*_${arch}_en-US.msi" } | Select-Object -First 1
if (-not $asset -and $arch -eq 'arm64') {
    # An Arm machine can run the x64 build under emulation, so a release
    # without an arm64 MSI is a reason to fall back, not to fail.
    Write-Host "No arm64 installer in this release; falling back to x64 (runs emulated)." -ForegroundColor Yellow
    $asset = $latest.assets | Where-Object { $_.name -like "SoloMD_*_x64_en-US.msi" } | Select-Object -First 1
}
if (-not $asset) {
    Write-Host "Error: no Windows MSI installer found in latest release" -ForegroundColor Red
    exit 1
}

$out = Join-Path $env:TEMP $asset.name
Write-Step "Downloading $($asset.name) to $out..."
try {
    # Progress bar shows bytes downloaded
    $ProgressPreference = 'SilentlyContinue'  # cleaner output
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $out -UseBasicParsing
} catch {
    Write-Host "Error: download failed: $_" -ForegroundColor Red
    exit 1
}

Write-Step "Launching installer..."
if ($env:SOLOMD_SILENT -eq '1') {
    Start-Process -FilePath 'msiexec.exe' -ArgumentList '/i', "`"$out`"", '/qn' -Wait
} else {
    Start-Process -FilePath $out -Wait
}

Remove-Item $out -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "[OK] SoloMD installed. Launch from the Start Menu." -ForegroundColor Green
Write-Host "Docs: https://solomd.app"
Write-Host ""

# Build PinSight release artefacts (dev helper).
#
#   powershell -ExecutionPolicy Bypass -File scripts\package.ps1
#
# Produces:
#   dist\PinSight-<version>-portable.zip   single-folder portable build
#   dist\PinSight-<version>-setup.exe      installer (needs Inno Setup 6)
#
# Pass -SkipInstaller when Inno Setup (`iscc`) is not on PATH.
# Pass -Upx to UPX-compress the exe (~7.2 MB -> ~2.5 MB). Off by default: some
# antivirus scanners flag packed binaries, so it is an explicit choice. The
# compressed exe is used by BOTH the portable zip and the installer.

param(
    [switch]$SkipInstaller,
    [switch]$Upx
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$dist = Join-Path $root "dist"
$stage = Join-Path $dist "portable\PinSight"

Write-Host "==> building release"
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

Write-Host "==> staging portable folder"
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Path $stage -Force | Out-Null

Copy-Item "target\release\pinsight.exe" $stage
Copy-Item "README.md" $stage
Copy-Item "LICENSE" $stage
Copy-Item "config.example.json" $stage
Copy-Item "assets" $stage -Recurse

# The marker is what makes the binary switch to portable mode on next start.
Set-Content -Path (Join-Path $stage "pinsight.portable") -Value "delete this file to install" -NoNewline
New-Item -ItemType Directory -Path (Join-Path $stage "captures") -Force | Out-Null

if ($Upx) {
    $upxCmd = Get-Command upx -ErrorAction SilentlyContinue
    if ($null -eq $upxCmd) {
        Write-Warning "upx not found on PATH — skipping compression (install UPX or drop -Upx)"
    }
    else {
        Write-Host "==> UPX-compressing exe"
        & $upxCmd.Source --best --lzma (Join-Path $stage "pinsight.exe")
        if ($LASTEXITCODE -ne 0) { throw "UPX failed" }
    }
}

if (-not (Test-Path $dist)) { New-Item -ItemType Directory -Path $dist | Out-Null }
$zip = Join-Path $dist "PinSight-$version-portable.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip
Write-Host "==> $zip"

if (-not $SkipInstaller) {
    $iscc = Get-Command iscc -ErrorAction SilentlyContinue
    if ($null -eq $iscc) {
        Write-Warning "iscc not found on PATH — install Inno Setup 6, or pass -SkipInstaller"
    }
    else {
        Write-Host "==> building installer"
        # Bundle the *staged* exe (which is UPX-compressed when -Upx was passed)
        # so the installed binary on disk is small too. Inno's own lzma2
        # compression only shrinks the setup file, not the installed exe.
        & $iscc.Source "/DPinsightExe=..\..\dist\portable\PinSight\pinsight.exe" "packaging\windows\pinsight.iss"
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed" }
    }
}

Write-Host "==> done"

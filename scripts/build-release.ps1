# Build nidavellir-service sidecar for Tauri bundle (externalBin naming convention).
# Run from repo root, or via npm beforeBuildCommand (cwd = apps/ui).
$ErrorActionPreference = "Stop"
# npm/cmd can pass PowerShell 7's module path into Windows PowerShell 5.1.
# Load this host's utility manifest so Get-FileHash resolves to a compatible module.
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

Write-Host "[build-release] Repo root: $RepoRoot"

cargo build --release -p nidavellir-service
if ($LASTEXITCODE -ne 0) { throw 'Service release build failed; existing sidecar was not replaced' }

$rustcOut = rustc -vV
if ($LASTEXITCODE -ne 0) { throw 'Could not read rustc host identity' }
$triple = ($rustcOut | Select-String "^host: ").ToString().Replace("host:", "").Trim()
if (-not $triple) {
    throw "Could not parse rustc host triple"
}
Write-Host "[build-release] Host triple: $triple"

$srcExe = Join-Path $RepoRoot "target\release\nidavellir-service.exe"
if (-not (Test-Path $srcExe)) {
    throw "Missing $srcExe - cargo build failed?"
}

$binDir = Join-Path $RepoRoot "apps\ui\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

$dstName = "nidavellir-service-$triple.exe"
$dstExe = Join-Path $binDir $dstName
Copy-Item -Force $srcExe $dstExe
if ((Get-FileHash -LiteralPath $srcExe -Algorithm SHA256).Hash -ne
    (Get-FileHash -LiteralPath $dstExe -Algorithm SHA256).Hash) {
    throw 'Copied sidecar does not match the built service'
}
Write-Host "[build-release] Copied sidecar to $dstExe"

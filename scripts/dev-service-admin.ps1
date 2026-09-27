# Start the local Core Service elevated; no installer and no automatic rebuild/restart watcher.
param([switch]$Release, [switch]$DevelopmentValidation)
$ErrorActionPreference = "Stop"
if (Get-Process -Name nidavellir-service -ErrorAction SilentlyContinue) {
    throw 'A Core Service process is already running. Close it normally before starting another console session.'
}
if (Get-Service -Name NidavellirCore -ErrorAction SilentlyContinue) {
    throw 'An installed Core Service is registered. Use one execution workflow at a time; do not start a competing development service.'
}
$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ((Split-Path -Leaf $RepoRoot) -eq "scripts") {
    $RepoRoot = Split-Path -Parent $RepoRoot
}

$buildArgs = @('build', '-p', 'nidavellir-service')
if ($Release) { $buildArgs += '--release' }
$serviceArgs = @('console')
if ($DevelopmentValidation) { $serviceArgs += '--development-validation' }
$serviceProfile = if ($Release) { 'release' } else { 'debug' }
Write-Host '[dev-service-admin] Building nidavellir-service...' -ForegroundColor Cyan
Push-Location $RepoRoot
try {
    cargo @buildArgs
    if ($LASTEXITCODE -ne 0) { throw 'Core Service build failed; no process was started.' }
} finally {
    Pop-Location
}
if (Get-Process -Name nidavellir-service -ErrorAction SilentlyContinue) {
    throw 'Another Core Service started during the build; this launch was canceled.'
}
$serviceExe = Join-Path $RepoRoot "target\$serviceProfile\nidavellir-service.exe"
if (-not (Test-Path -LiteralPath $serviceExe -PathType Leaf)) {
    throw "Core Service executable was not found: $serviceExe"
}

# Launch only the Core: no cargo/PowerShell parent retaining the console after native exit.
Write-Host '[dev-service-admin] Starting Core Service (console)...' -ForegroundColor Green
Start-Process -FilePath $serviceExe -Verb RunAs -WorkingDirectory $RepoRoot -ArgumentList $serviceArgs

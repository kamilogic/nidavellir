# Register the locally built Core as the NidavellirCore Windows service (LocalSystem, Automatic),
# like the installer does: it starts before login and SCM restarts it after failures, so a Forge
# with auto-resume continues through a reboot or a driver-only GPU reset with nobody logged in.
# Run from an elevated PowerShell. Install also updates; never update during a run (Resume
# requires the same build). Uninstall keeps all Nidavellir data in ProgramData.
param([Parameter(Mandatory)][ValidateSet('Install', 'Uninstall')][string]$Action)
$ErrorActionPreference = 'Stop'
$RepoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$lifecycle = Join-Path $RepoRoot 'apps\ui\src-tauri\windows\service-lifecycle.ps1'
# Outside the repo: a LocalSystem service must not run a binary that any user process can replace.
$installDir = Join-Path $env:ProgramFiles 'Nidavellir Dev'

$principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this script from an elevated PowerShell.'
}
$registered = Get-CimInstance -ClassName Win32_Service -Filter "Name='NidavellirCore'"
if ($registered -and $registered.PathName -notlike "*$installDir\*") {
    throw "NidavellirCore belongs to another install ($($registered.PathName)). Uninstall it first."
}
if ($Action -eq 'Uninstall') {
    & $lifecycle -Action Uninstall -InstallDir $installDir
    return
}
if (Get-Process -Name nidavellir-service -ErrorAction SilentlyContinue | Where-Object { $_.Path -notlike "$installDir\*" }) {
    throw 'A console Core Service is running. Close it before registering the service.'
}
Push-Location $RepoRoot
try {
    cargo build -p nidavellir-service --release
    if ($LASTEXITCODE -ne 0) { throw 'Core Service build failed; the service was not changed.' }
} finally {
    Pop-Location
}
& $lifecycle -Action Prepare -InstallDir $installDir
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
Copy-Item -LiteralPath (Join-Path $RepoRoot 'target\release\nidavellir-service.exe') -Destination $installDir -Force
& $lifecycle -Action Install -InstallDir $installDir

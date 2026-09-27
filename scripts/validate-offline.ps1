# Software-only gate. Never starts the Core Service or a GPU workload.
param([switch]$Browser)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Push-Location $repo
try {
    & (Join-Path $PSScriptRoot 'test-installer-lifecycle.ps1')
    if (-not (Test-Path (Join-Path $repo 'apps\ui\src-tauri\binaries\nidavellir-service-x86_64-pc-windows-msvc.exe'))) {
        & (Join-Path $PSScriptRoot 'build-release.ps1')
    }
    cargo test --workspace --quiet
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed' }
    Push-Location (Join-Path $repo 'apps\ui')
    try {
        npm.cmd test
        if ($LASTEXITCODE -ne 0) { throw 'UI journey tests failed' }
        npm.cmd run build
        if ($LASTEXITCODE -ne 0) { throw 'UI production build failed' }
        if ($Browser) {
            npm.cmd run test:e2e
            if ($LASTEXITCODE -ne 0) { throw 'Browser journeys failed' }
        }
    } finally { Pop-Location }
    git -c core.safecrlf=false diff --check
    if ($LASTEXITCODE -ne 0) { throw 'Diff whitespace check failed' }
    Write-Host 'Offline gates passed. Hardware and installer acceptance are separate.'
} finally { Pop-Location }

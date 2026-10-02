# Full release: sidecar + frontend + NSIS installer (run from repo root)
$ErrorActionPreference = "Stop"
# Also support invocation through npm/cmd with another PowerShell host's module path.
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Split-Path -Parent $ScriptDir
Set-Location $RepoRoot

function Get-SourceSnapshot {
  $paths = @(git -c core.quotepath=false ls-files --cached --others --exclude-standard -- crates apps/ui/src apps/ui/src-tauri apps/ui/tests scripts .github/workflows Cargo.toml Cargo.lock apps/ui/package.json apps/ui/package-lock.json apps/ui/vite.config.js apps/ui/playwright.config.js)
  if ($LASTEXITCODE -ne 0) { throw 'Cannot inventory release sources' }
  $files = @($paths | Sort-Object -Unique | ForEach-Object {
    $path = Join-Path $RepoRoot $_
    [pscustomobject]@{ path = $_; sha256 = $(if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } else { $null }) }
  })
  $payload = [Text.Encoding]::UTF8.GetBytes(($files | ConvertTo-Json -Compress))
  $hasher = [Security.Cryptography.SHA256]::Create()
  try { $hash = [BitConverter]::ToString($hasher.ComputeHash($payload)).Replace('-', '') }
  finally { $hasher.Dispose() }
  [pscustomobject]@{ sha256 = $hash; files = $files }
}

# Release installers carry the updater signature; an installer with no public key could never
# verify a later update. See docs/releasing.md for the one-time key setup.
$updater = (Get-Content -LiteralPath 'apps\ui\src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json).plugins.updater
if (-not $updater.pubkey) { throw 'plugins.updater.pubkey is empty in tauri.conf.json; see docs/releasing.md' }
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) { throw 'Set TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD to sign the installer; see docs/releasing.md' }

$sourceBefore = Get-SourceSnapshot
$buildStartedUtc = [DateTime]::UtcNow
Set-Location (Join-Path $RepoRoot "apps\ui")
if (-not (Test-Path "node_modules")) {
  npm.cmd ci
  if ($LASTEXITCODE -ne 0) { throw 'UI dependency installation failed' }
}
# Tauri beforeBuildCommand owns sidecar + UI build exactly once.
npm.cmd run tauri build
if ($LASTEXITCODE -ne 0) { throw 'Installer build failed; no release was produced' }

Set-Location $RepoRoot
$sourceAfter = Get-SourceSnapshot
if ($sourceBefore.sha256 -ne $sourceAfter.sha256) { throw 'Source files changed during packaging; freeze changes and rebuild before release' }
$commit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify release commit' }
$config = Get-Content -LiteralPath 'apps\ui\src-tauri\tauri.conf.json' -Raw | ConvertFrom-Json
$artifacts = @('target\release\nidavellir-service.exe', 'apps\ui\src-tauri\binaries\nidavellir-service-x86_64-pc-windows-msvc.exe', 'target\release\nidavellir-ui.exe', "target\release\bundle\nsis\Nidavellir_$($config.version)_x64-setup.exe", "target\release\bundle\nsis\Nidavellir_$($config.version)_x64-setup.exe.sig")
$identity = @($artifacts | ForEach-Object {
  $item = Get-Item -LiteralPath (Join-Path $RepoRoot $_)
  [pscustomobject]@{ path = $_; bytes = $item.Length; sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash }
})
if ($identity[0].sha256 -ne $identity[1].sha256) { throw 'Packaged sidecar source does not match the release service' }
$manifest = [ordered]@{
  builtUtc = [DateTime]::UtcNow.ToString('o'); buildStartedUtc = $buildStartedUtc.ToString('o')
  gitCommit = $commit; source = $sourceAfter; artifacts = $identity
  acceptance = 'Packaging evidence only. Hardware and Windows installer lifecycle acceptance remain required.'
}
$manifestPath = Join-Path $RepoRoot 'target\release\release-manifest.json'
[IO.File]::WriteAllText($manifestPath, ($manifest | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
Write-Host "Installer and source/artifact hashes: $manifestPath"

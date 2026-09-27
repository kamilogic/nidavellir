# Read-only inventory for D3. Never acknowledges incidents, starts a service or loads the GPU.
param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$dataDir = Join-Path $env:ProgramData 'Nidavellir'
$blockers = [System.Collections.Generic.List[string]]::new()

function Read-StateJson([string]$Name) {
    $path = Join-Path $dataDir $Name
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    try { return Get-Content -LiteralPath $path -Raw | ConvertFrom-Json }
    catch { $blockers.Add("Unreadable ${Name}: $($_.Exception.Message)"); return $null }
}

$safe = Read-StateJson 'safe_loop.json'
$applied = Read-StateJson 'gpu_applied.json'
$checkpoint = Read-StateJson 'forge_state.json'
$bootFlagPresent = Test-Path -LiteralPath (Join-Path $dataDir 'boot_flag.json')
if ($bootFlagPresent) { $blockers.Add('Boot flag is present; stock recovery must be verified by the service') }
if ($safe.pending_forge_incident) { $blockers.Add('Incident acknowledgement is pending') }
if ($safe.safe_mode) { $blockers.Add('Safe Mode is active') }
if ($safe.state -eq 'unstable') { $blockers.Add('Safe Loop reports unstable state') }
$events = @()
$ledgerPath = Join-Path $dataDir 'condemnation_ledger.jsonl'
if (Test-Path -LiteralPath $ledgerPath) {
    try {
        $events = @(Get-Content -LiteralPath $ledgerPath | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json })
    } catch { $blockers.Add("Condemnation ledger is unreadable: $($_.Exception.Message)") }
}
$gpu = @()
$smi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
if ($smi) {
    $gpu = @(& $smi.Source --query-gpu=uuid,name,driver_version --format=csv,noheader)
    if ($LASTEXITCODE -ne 0) { $blockers.Add('NVIDIA driver inventory failed') }
} else { $blockers.Add('nvidia-smi is unavailable; GPU/driver identity remains unverified') }
$binaries = @()
foreach ($relative in @('target\debug\nidavellir-service.exe', 'target\release\nidavellir-service.exe', 'apps\ui\src-tauri\binaries\nidavellir-service-x86_64-pc-windows-msvc.exe')) {
    $path = Join-Path $repo $relative
    if (Test-Path -LiteralPath $path) {
        $item = Get-Item -LiteralPath $path
        $binaries += [pscustomobject]@{ path = $relative; bytes = $item.Length; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash; modifiedUtc = $item.LastWriteTimeUtc.ToString('o') }
    }
}
$backendReadiness = $null
$serviceExe = Join-Path $repo 'target\release\nidavellir-service.exe'
if (Test-Path -LiteralPath $serviceExe) {
    # The explicit read-only command shares the backend's effective crash-budget rules.
    try {
        $raw = & $serviceExe acceptance-preflight
        if ($LASTEXITCODE -ne 0) { throw 'Backend readiness command failed' }
        $backendReadiness = $raw | ConvertFrom-Json
        if ($backendReadiness.read_only -ne $true) { throw 'Backend did not confirm read-only preflight' }
        foreach ($reason in $backendReadiness.blockers) { if (-not $blockers.Contains($reason)) { $blockers.Add($reason) } }
    } catch { $blockers.Add("Backend readiness unavailable: $($_.Exception.Message)") }
}
$report = [ordered]@{
    capturedUtc = [DateTime]::UtcNow.ToString('o')
    scope = 'Read-only preflight; this is not hardware acceptance or authorization to tune'
    gate = $(if ($blockers.Count) { 'blocked' } else { 'requires_live_service_verification' })
    blockers = @($blockers.ToArray())
    gpuDriverInventory = $gpu
    binaries = $binaries
    service = @(Get-Service -Name NidavellirCore -ErrorAction SilentlyContinue | Select-Object Name,Status)
    process = @(Get-Process nidavellir-service -ErrorAction SilentlyContinue | Select-Object Id,StartTime)
    safeLoopState = $safe.state
    pendingIncident = $safe.pending_forge_incident
    bootFlagPresent = $bootFlagPresent
    checkpointPresent = ($null -ne $checkpoint)
    appliedDescriptorPresent = ($null -ne $applied)
    candidateCrashRows = @($events | Where-Object { $_.kind -eq 'candidate-crash' } | Select-Object gpu_key,run_id,target_mhz,vf_bin_mv,qualification_contract_version,rehabilitated)
    backendReadiness = $backendReadiness
    eligibilityNote = 'Rows are historical inventory. backendReadiness uses the service effective crash-budget policy; no ready result substitutes live service/hardware acceptance.'
    remainingEvidence = @('Installed application lifecycle', 'Exact qualified Apply', 'Measured power/performance', 'Real-use observation', 'Restart/reapply', 'Verified stock restoration')
}
$json = $report | ConvertTo-Json -Depth 10
if ($OutputPath) {
    $path = [IO.Path]::GetFullPath($OutputPath)
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
    [IO.File]::WriteAllText($path, $json, [Text.UTF8Encoding]::new($false))
}
$json

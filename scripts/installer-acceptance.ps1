# D5 package + guarded installer lifecycle. Inspect never installs or starts a service.
param(
    [ValidateSet('Prepare', 'Inspect', 'Run')][string]$Action = 'Inspect',
    [string]$BundleDir,
    [ValidateSet('DisposableVm', 'ExistingPc')][string]$Environment = 'DisposableVm',
    [switch]$ConfirmDisposableVm,
    [switch]$ConfirmHostLifecycle
)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1') -ErrorAction Stop
if (-not $BundleDir) {
    $BundleDir = if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'release-manifest.json')) {
        $PSScriptRoot
    } else { Join-Path (Split-Path -Parent $PSScriptRoot) 'target\beta\installer-acceptance' }
}
$BundleDir = [IO.Path]::GetFullPath($BundleDir)
$manifestPath = Join-Path $BundleDir 'release-manifest.json'
$reportPath = Join-Path $BundleDir 'acceptance-report.json'

function Assert-RunnerIdentity($Manifest) {
    $entry = @($Manifest.source.files | Where-Object { $_.path -eq 'scripts/installer-acceptance.ps1' })
    if ($entry.Count -ne 1 -or (Get-FileHash -LiteralPath $PSCommandPath).Hash -ne $entry[0].sha256) {
        throw 'Acceptance runner differs from the release manifest. Rebuild the package before preparing or running this kit.'
    }
}

if ($Action -eq 'Prepare') {
    $repo = Split-Path -Parent $PSScriptRoot
    $manifest = Get-Content -LiteralPath (Join-Path $repo 'target\release\release-manifest.json') -Raw | ConvertFrom-Json
    Assert-RunnerIdentity $manifest
    $installer = @($manifest.artifacts | Where-Object { $_.path -match '\\bundle\\nsis\\[^\\]+-setup\.exe$' })
    if ($installer.Count -ne 1) { throw 'Manifest must identify exactly one NSIS installer' }
    $source = Join-Path $repo $installer[0].path
    if ((Get-FileHash -LiteralPath $source).Hash -ne $installer[0].sha256) { throw 'Installer hash differs from the release manifest' }
    if (Test-Path -LiteralPath $reportPath) { throw 'This bundle already contains acceptance evidence. Preserve it and choose a new empty BundleDir.' }
    [IO.Directory]::CreateDirectory($BundleDir) | Out-Null
    Copy-Item -LiteralPath $source -Destination $BundleDir -Force
    Copy-Item -LiteralPath (Join-Path $repo 'target\release\release-manifest.json') -Destination $manifestPath -Force
    Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $BundleDir 'installer-acceptance.ps1') -Force
    Copy-Item -LiteralPath (Join-Path $repo 'docs\installer-acceptance.md') -Destination $BundleDir -Force
    Write-Output "Prepared: $BundleDir. Inspect on the selected Windows test machine before Run. Nothing was installed."
    return
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
Assert-RunnerIdentity $manifest
$artifact = @($manifest.artifacts | Where-Object { $_.path -match '\\bundle\\nsis\\[^\\]+-setup\.exe$' })
$serviceArtifact = @($manifest.artifacts | Where-Object { $_.path -eq 'target\release\nidavellir-service.exe' })
if ($artifact.Count -ne 1 -or $serviceArtifact.Count -ne 1) { throw 'Manifest lacks unique installer/service identity' }
$installerPath = Join-Path $BundleDir ([IO.Path]::GetFileName($artifact[0].path))
if ((Get-FileHash -LiteralPath $installerPath).Hash -ne $artifact[0].sha256) { throw 'Installer hash differs from the release manifest' }
$machine = Get-CimInstance Win32_ComputerSystem
$os = Get-CimInstance Win32_OperatingSystem
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$elevated = ([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$vm = ($machine.Manufacturer -eq 'Microsoft Corporation' -and $machine.Model -eq 'Virtual Machine') -or
      ($machine.Manufacturer -match '^VMware' -and $machine.Model -match '^VMware') -or
      ($machine.Model -eq 'VirtualBox')
$dataDir = Join-Path $env:ProgramData 'Nidavellir'
$installDir = Join-Path $env:ProgramFiles 'Nidavellir Acceptance'
$serviceExe = Join-Path $installDir 'nidavellir-service.exe'
$blockers = [Collections.Generic.List[string]]::new()
$hostLifecycle = $Environment -eq 'ExistingPc'
if (-not $hostLifecycle -and -not $vm) { $blockers.Add('A supported disposable VM is required for DisposableVm mode; HypervisorPresent alone does not identify a guest.') }
if (-not $elevated) { $blockers.Add('Run the lifecycle from an elevated PowerShell on the selected test machine.') }
if (-not $hostLifecycle -and (Get-CimInstance Win32_VideoController | Where-Object { $_.PNPDeviceID -match 'VEN_10DE' })) {
    $blockers.Add('NVIDIA passthrough hardware is present. This installer test must not reach a physical GPU.')
}
if (Get-Service NidavellirCore -ErrorAction SilentlyContinue) { $blockers.Add('NidavellirCore already exists. Review its installation before testing; this runner does not replace an existing installation.') }
if (Get-Process nidavellir-service,nidavellir-ui -ErrorAction SilentlyContinue) { $blockers.Add('A Nidavellir process is already running.') }
if ((-not $hostLifecycle -and (Test-Path -LiteralPath $dataDir)) -or (Test-Path -LiteralPath $installDir)) { $blockers.Add('The test install path must be absent; DisposableVm also requires absent data.') }
foreach ($registryRoot in @('HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall', 'HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall')) {
    if (Test-Path -LiteralPath $registryRoot) {
        if (Get-ChildItem -LiteralPath $registryRoot | Get-ItemProperty | Where-Object { $_.DisplayName -like '*Nidavellir*' }) {
            $blockers.Add('An existing Nidavellir uninstall registration must be reviewed before testing.')
        }
    }
}
if ($hostLifecycle) {
    # Never move real safety state aside to make an installation look clean.
    foreach ($name in @('boot_flag.json', 'gpu_applied.json', 'forge_state.json')) {
        if (Test-Path -LiteralPath (Join-Path $dataDir $name)) { $blockers.Add("ExistingPc requires no active/reapply state: $name") }
    }
    if (Test-Path -LiteralPath $dataDir) {
        if (Get-ChildItem -LiteralPath $dataDir -Force -Filter '.boot_flag.json.clear-*') { $blockers.Add('An unresolved BootFlag clear claim is present.') }
        if (Get-ChildItem -LiteralPath $dataDir -Force -Recurse | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) { $blockers.Add('The data backup must not traverse reparse points.') }
    }
    $safePath = Join-Path $dataDir 'safe_loop.json'
    $historyPath = Join-Path $dataDir 'condemnation_ledger.jsonl'
    try {
        $safeBefore = Get-Content -LiteralPath $safePath -Raw | ConvertFrom-Json
        if ($null -ne $safeBefore.last_validated) { $blockers.Add('A last-validated tuning point requires a separate recovery review before installer testing.') }
        Get-Content -LiteralPath $historyPath | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json | Out-Null }
        $historyHash = (Get-FileHash -LiteralPath $historyPath).Hash
    } catch { $blockers.Add("ExistingPc requires readable safety history: $($_.Exception.Message)") }
}
if (Test-Path -LiteralPath $reportPath) { $blockers.Add('A prior acceptance report exists. Preserve it and use a fresh copy after restoring the VM.') }
foreach ($path in @($BundleDir, $env:ProgramData, $env:ProgramFiles, $dataDir) | Where-Object { Test-Path -LiteralPath $_ }) {
    $directory = Get-Item -LiteralPath $path -Force
    while ($directory) {
        if ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) { $blockers.Add("Reparse/mapped directory is not accepted: $($directory.FullName)"); break }
        $directory = $directory.Parent
    }
    $drive = [IO.DriveInfo]::new([IO.Path]::GetPathRoot($path))
    if ($drive.DriveType -ne [IO.DriveType]::Fixed) { $blockers.Add("Use a local fixed disk: $path") }
}
$report = [ordered]@{
    capturedUtc = [DateTime]::UtcNow.ToString('o'); mode = $Action; environment = $Environment
    status = $(if ($blockers.Count) { 'blocked' } else { 'ready_for_installer_lifecycle' })
    machine = @{ manufacturer=$machine.Manufacturer; model=$machine.Model; os=$os.Caption; build=$os.BuildNumber; elevated=$elevated }
    installerSha256 = $artifact[0].sha256; sourceSha256 = $manifest.source.sha256
    blockers = @($blockers.ToArray()); steps = @()
    backup = $null
    remainingEvidence = @('Upgrade from a different released version', 'Failure injection against installed SCM', 'Unelevated desktop UI', 'GPU acceptance')
}
if ($Action -eq 'Inspect') { $report | ConvertTo-Json -Depth 7; return }
if ($blockers.Count -or ($hostLifecycle -and -not $ConfirmHostLifecycle) -or (-not $hostLifecycle -and -not $ConfirmDisposableVm)) {
    $report | ConvertTo-Json -Depth 7
    throw 'Lifecycle refused. Resolve preflight blockers and explicitly confirm the selected environment; no installation was attempted.'
}

function Save-Report { [IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 7), [Text.UTF8Encoding]::new($false)) }
function Step([string]$Name, [scriptblock]$Body) {
    $report.steps += [ordered]@{name=$Name;status='running';startedUtc=[DateTime]::UtcNow.ToString('o')}
    Save-Report
    try { & $Body; $report.steps[-1].status = 'passed' }
    catch { $report.steps[-1].status = 'failed'; $report.steps[-1].error = $_.Exception.Message; throw }
    finally { Save-Report }
}
function Run-Package([string]$Path, [string]$Arguments) {
    $process = Start-Process -FilePath $Path -ArgumentList $Arguments -PassThru -WindowStyle Hidden
    try {
        if (-not $process.WaitForExit(180000)) { throw 'Installer timeout. Preserve the report and inspect the process/service; do not retry against an unfinished process.' }
        if ($process.ExitCode -ne 0) { throw "Installer exited with code $($process.ExitCode)" }
    } finally { $process.Dispose() }
}
function Wait-Core([string]$Expected) {
    $until = [DateTime]::UtcNow.AddSeconds(35)
    do {
        $service = Get-CimInstance Win32_Service -Filter "Name='NidavellirCore'" -OperationTimeoutSec 5
        if ($Expected -eq 'Absent' -and $null -eq $service) { return }
        if ($service -and $service.State -eq $Expected) {
            if ($service.ExitCode -ne 0) { throw "Service reports an error: $($service.ExitCode)/$($service.ServiceSpecificExitCode)" }
            return $service
        }
        Start-Sleep -Milliseconds 200
    } while ([DateTime]::UtcNow -lt $until)
    throw "Service did not reach $Expected"
}
function Check-Installed {
    $service = Wait-Core 'Running'
    if ($service.PathName -ne ('"' + $serviceExe + '"')) { throw "Wrong SCM executable path: $($service.PathName)" }
    if ((Get-FileHash -LiteralPath $serviceExe).Hash -ne $serviceArtifact[0].sha256) { throw 'Installed service hash mismatch' }
    $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', 'NidavellirCore', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
    try {
        $pipe.Connect(5000)
        $request = [Text.Encoding]::UTF8.GetBytes('{"method":"Ping"}' + "`n")
        $write = $pipe.WriteAsync($request, 0, $request.Length)
        if (-not $write.Wait(5000)) { throw 'Ping write timed out' }
        $reader = [IO.StreamReader]::new($pipe)
        $read = $reader.ReadLineAsync()
        if (-not $read.Wait(5000)) { throw 'Ping reply timed out' }
        $reply = $read.Result | ConvertFrom-Json
        if ($reply.ok -ne $true -or $reply.data.type -ne 'Pong') { throw 'Invalid Ping reply' }
    } finally { $pipe.Dispose() }
}
function Check-History {
    if ((Get-FileHash -LiteralPath $historyPath).Hash -ne $historyHash) { throw 'Safety history changed or was removed' }
    if ($hostLifecycle) {
        $safeAfter = Get-Content -LiteralPath $safePath -Raw | ConvertFrom-Json
        foreach ($field in @('pending_forge_incident', 'forge_incidents', 'blacklist', 'crash_log', 'consecutive_crashes', 'safe_mode')) {
            if ((ConvertTo-Json -InputObject $safeBefore.$field -Depth 15 -Compress) -ne (ConvertTo-Json -InputObject $safeAfter.$field -Depth 15 -Compress)) { throw "Safety evidence changed: $field" }
        }
        foreach ($name in @('boot_flag.json', 'gpu_applied.json', 'forge_state.json')) {
            if (Test-Path -LiteralPath (Join-Path $dataDir $name)) { throw "Unexpected tuning state appeared: $name" }
        }
    }
}

$report.status = 'running'
Save-Report
try {
    if ($hostLifecycle) {
        Step 'backup-existing-data' {
            $backupDir = Join-Path $BundleDir 'data-before-install'
            if (Test-Path -LiteralPath $backupDir) { throw 'Backup path already exists; preserve it and use a fresh bundle.' }
            $inventory = @(Get-ChildItem -LiteralPath $dataDir -File -Recurse -Force | ForEach-Object {
                [pscustomobject]@{path=$_.FullName.Substring($dataDir.Length + 1); sha256=(Get-FileHash -LiteralPath $_.FullName).Hash}
            })
            Copy-Item -LiteralPath $dataDir -Destination $backupDir -Recurse
            foreach ($file in $inventory) {
                if ((Get-FileHash -LiteralPath (Join-Path $backupDir $file.path)).Hash -ne $file.sha256 -or
                    (Get-FileHash -LiteralPath (Join-Path $dataDir $file.path)).Hash -ne $file.sha256) { throw "Backup/source verification failed: $($file.path)" }
            }
            $report.backup = @{path=$backupDir; files=$inventory; verifiedUtc=[DateTime]::UtcNow.ToString('o')}
            Check-History
        }
    }
    $installStep = if ($hostLifecycle) { 'install-with-existing-history-and-ping' } else { 'clean-install-and-ping' }
    Step $installStep { Run-Package $installerPath ("/S /D=" + $installDir); Check-Installed; if ($hostLifecycle) { Check-History } }
    if (-not $hostLifecycle) {
    # Synthetic, clearly identified history on a pristine VM; never copy real host ProgramData.
    $historyPath = Join-Path $dataDir 'condemnation_ledger.jsonl'
    if (Test-Path -LiteralPath $historyPath) { throw 'Unexpected existing ledger; preserve VM state for review' }
    $fixture = @{timestamp=[DateTime]::UtcNow.ToString('o');gpu_key='installer-fixture-no-hardware';severity='rigid';kind='candidate-crash';target_mhz=1800;vf_bin_mv=900;run_id='installer-fixture';qualification_contract_version=29;note='Synthetic D5 retention fixture; no GPU measurement';rehabilitated=$false}
    [IO.Directory]::CreateDirectory($dataDir) | Out-Null
    [IO.File]::WriteAllText($historyPath, (($fixture | ConvertTo-Json -Compress) + "`n"), [Text.UTF8Encoding]::new($false))
    $historyHash = (Get-FileHash -LiteralPath $historyPath).Hash
    }
    Step 'reinstall-while-running' { Run-Package $installerPath ("/S /D=" + $installDir); Check-Installed; Check-History }
    Step 'cooperative-scm-stop' {
        $service = Wait-Core 'Running'
        $result = Invoke-CimMethod -InputObject $service -MethodName StopService -OperationTimeoutSec 10
        if ($result.ReturnValue -ne 0) { throw "StopService failed: $($result.ReturnValue)" }
        Wait-Core 'Stopped' | Out-Null
        Check-History
    }
    Step 'reinstall-while-stopped' { Run-Package $installerPath ("/S /D=" + $installDir); Check-Installed; Check-History }
    Step 'uninstall-and-retain-history' {
        Run-Package (Join-Path $installDir 'uninstall.exe') ("/S _?=" + $installDir)
        Wait-Core 'Absent' | Out-Null
        if (Test-Path -LiteralPath $serviceExe) { throw 'Service binary remains after uninstall' }
        if (Test-Path -LiteralPath (Join-Path $installDir 'nidavellir-ui.exe')) { throw 'UI binary remains after uninstall' }
        Check-History
    }
    $report.status = 'lifecycle_subset_passed'
} catch {
    $report.status = 'failed'
    $report.error = $_.Exception.Message
    throw
} finally { Save-Report }
Write-Output "Lifecycle subset passed. Preserve $reportPath and its backup/snapshot; remaining evidence is listed in the report."

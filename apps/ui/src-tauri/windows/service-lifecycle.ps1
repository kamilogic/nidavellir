# Invoked by NSIS with elevation. No tuning, state deletion, or process killing here.
param(
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Install', 'Uninstall')][string]$Action,
    [Parameter(Mandatory)][string]$InstallDir,
    [ValidateRange(0, 60)][int]$TimeoutSeconds = 30
)
$ErrorActionPreference = 'Stop'
$serviceName = 'NidavellirCore'
# Tauri externalBin removes the build-target suffix when installing the sidecar.
$binary = Join-Path ([IO.Path]::GetFullPath($InstallDir)) 'nidavellir-service.exe'

# The passive installer closes by itself; this log keeps what a stalled update needs.
function Write-InstallLog([string]$Message) {
    Add-Content -LiteralPath (Join-Path $env:ProgramData 'Nidavellir\installer.log') -Value "$([DateTime]::UtcNow.ToString('o')) $Action $Message" -ErrorAction SilentlyContinue
}

function Read-CoreService {
    Get-CimInstance -ClassName Win32_Service -Filter "Name='NidavellirCore'" -OperationTimeoutSec 10 -ErrorAction Stop
}

function Invoke-CoreMethod($Service, [string]$Method, [hashtable]$Arguments = @{}) {
    $result = Invoke-CimMethod -InputObject $Service -MethodName $Method -Arguments $Arguments -OperationTimeoutSec 10 -ErrorAction Stop
    if ($result.ReturnValue -ne 0) { throw "$Method failed for ${serviceName}: Windows service error $($result.ReturnValue)" }
}

function Wait-CoreState([string]$Expected) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        $service = Read-CoreService
        if ($Expected -eq 'Absent' -and $null -eq $service) { return }
        if ($service -and $service.State -eq $Expected) {
            if ($Expected -eq 'Stopped' -and $service.ExitCode -and $service.ExitCode -ne 0) {
                throw "Core Service stopped with an error ($($service.ExitCode)/$($service.ServiceSpecificExitCode)); clean shutdown was not confirmed. Recovery evidence is preserved. Restart Windows before replacing the service."
            }
            return
        }
        if ([DateTime]::UtcNow -ge $deadline) { throw "$serviceName did not reach $Expected within ${TimeoutSeconds}s. Current state: $($service.State). Restart Windows and retry the installer." }
        Start-Sleep -Milliseconds 250
    } while ($true)
}

# SCM reports Stopped before Windows releases the executable (the GPU driver may still be tearing
# the process down), so replacing the binary waits for the process itself. One that lingers (0.5.3
# update: >30 s) gets its binary moved aside: a running image can be renamed, not overwritten.
function Wait-CoreProcessExit([int]$ProcessId) {
    if ($ProcessId -le 0) { return }
    $started = [DateTime]::UtcNow
    $deadline = $started.AddSeconds($TimeoutSeconds)
    do {
        $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -eq 'nidavellir-service' }
        if (-not $process) {
            Write-InstallLog ("Core process {0} exited {1:n1}s after stopping" -f $ProcessId, ([DateTime]::UtcNow - $started).TotalSeconds)
            return
        }
        if ([DateTime]::UtcNow -ge $deadline) { break }
        Start-Sleep -Milliseconds 250
    } while ($true)
    # WaitReason throws for a thread that is not waiting; diagnostics never fail the install.
    $waits = try {
        ($process.Threads | ForEach-Object { if ("$($_.ThreadState)" -eq 'Wait') { "Wait/$($_.WaitReason)" } else { "$($_.ThreadState)" } } |
            Group-Object | ForEach-Object { "$($_.Name) x$($_.Count)" }) -join '; '
    } catch { 'unavailable' }
    Write-InstallLog "Core process $ProcessId still running ${TimeoutSeconds}s after stopping (threads: $waits)"
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { return }
    try { Rename-Item -LiteralPath $binary -NewName "nidavellir-service.$ProcessId.old" -ErrorAction Stop }
    catch { throw "The Core process ($ProcessId) did not exit within ${TimeoutSeconds}s after stopping, and its binary could not be moved aside. Restart Windows and retry the installer." }
    Write-InstallLog "old Core binary moved aside as nidavellir-service.$ProcessId.old"
}

function Stop-CoreService($Service) {
    $processId = [int]$Service.ProcessId
    if ($Service.State -ne 'Stopped') {
        if ($Service.State -ne 'Stop Pending') { Invoke-CoreMethod $Service 'StopService' }
    }
    # Also inspect an already stopped service: retrying must not erase a failed shutdown.
    Wait-CoreState 'Stopped'
    Wait-CoreProcessExit $processId
}

$service = Read-CoreService
if ($Action -eq 'Prepare' -or $Action -eq 'Uninstall') {
    if ($service) {
        Stop-CoreService $service
        if ($Action -eq 'Uninstall') {
            Invoke-CoreMethod (Read-CoreService) 'Delete'
            Wait-CoreState 'Absent'
        }
    }
    # Prepare deliberately retains registration until the new binary is in place.
    Write-Output "$Action completed for $serviceName. Safety history is preserved."
    return
}

if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Installed Core Service binary is missing: $binary. Repair the installation." }
# Binaries moved aside by an earlier update are free once their process is gone.
Remove-Item -Path (Join-Path ([IO.Path]::GetFullPath($InstallDir)) 'nidavellir-service.*.old') -Force -ErrorAction SilentlyContinue
$quotedBinary = '"' + $binary + '"'
if ($service) {
    Stop-CoreService $service
    Invoke-CoreMethod (Read-CoreService) 'Change' @{ PathName = $quotedBinary; StartMode = 'Automatic'; DisplayName = 'Nidavellir Core Service' }
} else {
    $result = Invoke-CimMethod -ClassName Win32_Service -MethodName Create -Arguments @{
        Name = $serviceName; DisplayName = 'Nidavellir Core Service'; PathName = $quotedBinary
        ServiceType = [byte]16; ErrorControl = [byte]1; StartMode = 'Automatic'; StartName = 'LocalSystem'
    } -OperationTimeoutSec 10 -ErrorAction Stop
    if ($result.ReturnValue -ne 0) { throw "Create failed for ${serviceName}: Windows service error $($result.ReturnValue)" }
}
# SCM restarts the Core after any failure, including the deliberate non-zero exit of the
# driver-only GPU reset (Forge auto-resume). Startup recovery owns crash accounting.
& sc.exe failure $serviceName reset= 86400 actions= restart/5000/restart/5000/restart/5000 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Recovery actions could not be set for ${serviceName} (sc.exe exit $LASTEXITCODE)" }
& sc.exe failureflag $serviceName 1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Recovery flag could not be set for ${serviceName} (sc.exe exit $LASTEXITCODE)" }
# The program starts the Core when it opens; the Core stops itself when the program exits.
# Interactive users get the default service rights plus start (RP), nothing more.
& sc.exe sdset $serviceName 'D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWRPLOCRRC;;;IU)(A;;CCLCSWLOCRRC;;;SU)' | Out-Null
if ($LASTEXITCODE -ne 0) { throw "Start permission could not be set for ${serviceName} (sc.exe exit $LASTEXITCODE)" }
Invoke-CoreMethod (Read-CoreService) 'StartService'
Wait-CoreState 'Running'
Write-Output "Core Service is running. Installed binary: $binary"

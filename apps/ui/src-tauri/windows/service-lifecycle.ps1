# Invoked by NSIS with elevation. No tuning, state deletion, or process killing here.
param(
    [Parameter(Mandatory)][ValidateSet('Prepare', 'Install', 'Uninstall')][string]$Action,
    [Parameter(Mandatory)][string]$InstallDir,
    [ValidateRange(0, 60)][int]$TimeoutSeconds = 30
)
$ErrorActionPreference = 'Stop'
$serviceName = 'NidavellirCore'

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

function Stop-CoreService($Service) {
    if ($Service.State -ne 'Stopped') {
        if ($Service.State -ne 'Stop Pending') { Invoke-CoreMethod $Service 'StopService' }
    }
    # Also inspect an already stopped service: retrying must not erase a failed shutdown.
    Wait-CoreState 'Stopped'
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

# Tauri externalBin removes the build-target suffix when installing the sidecar.
$binary = Join-Path ([IO.Path]::GetFullPath($InstallDir)) 'nidavellir-service.exe'
if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw "Installed Core Service binary is missing: $binary. Repair the installation." }
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

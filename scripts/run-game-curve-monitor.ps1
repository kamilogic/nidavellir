param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(300, 3000)]
    [int]$TargetMhz,

    [Parameter(Mandatory = $true)]
    [ValidateRange(500, 1250)]
    [int]$VoltageMv,

    [Parameter(Mandatory = $true)]
    [string]$OutPath,

    [ValidateRange(1, 10)]
    [int]$PollSeconds = 2
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Write-MonitorEvent {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Event,
        [hashtable]$Fields = @{}
    )

    $row = [ordered]@{
        timestamp = [DateTimeOffset]::UtcNow.ToString("o")
        event = $Event
    }
    foreach ($entry in $Fields.GetEnumerator()) {
        $row[$entry.Key] = $entry.Value
    }
    $line = $row | ConvertTo-Json -Compress -Depth 20
    [System.IO.File]::AppendAllText($OutPath, $line + [Environment]::NewLine)
    Write-Output $line
}

function Invoke-Nidavellir {
    param([Parameter(Mandatory = $true)][hashtable]$Request)

    $writer.WriteLine(($Request | ConvertTo-Json -Compress -Depth 10))
    $line = $reader.ReadLine()
    if ($null -eq $line) {
        throw "Nidavellir IPC pipe closed"
    }
    $response = $line | ConvertFrom-Json
    if (-not $response.ok) {
        throw [string]$response.error
    }
    return $response
}

function Get-LatestTdrEvent {
    try {
        $event = Get-WinEvent -FilterHashtable @{
            LogName = "System"
            ProviderName = "nvlddmkm"
            Id = 153
        } -MaxEvents 1 -ErrorAction Stop
        return [pscustomobject]@{
            record_id = [long]$event.RecordId
            time_created = $event.TimeCreated.ToUniversalTime().ToString("o")
        }
    }
    catch {
        return $null
    }
}

$parent = Split-Path -Parent $OutPath
if ($parent) {
    [System.IO.Directory]::CreateDirectory($parent) | Out-Null
}
if ([System.IO.File]::Exists($OutPath)) {
    throw "Output already exists: $OutPath"
}

$pipe = [System.IO.Pipes.NamedPipeClientStream]::new(
    ".",
    "NidavellirCore",
    [System.IO.Pipes.PipeDirection]::InOut
)
$writer = $null
$reader = $null
$traceStarted = $false
$curveApplied = $false
$exitCode = 0

try {
    $pipe.Connect(5000)
    $writer = [System.IO.StreamWriter]::new($pipe)
    $writer.AutoFlush = $true
    $reader = [System.IO.StreamReader]::new($pipe)

    $safe = Invoke-Nidavellir @{ method = "GetSafeLoopStatus" }
    $manual = Invoke-Nidavellir @{ method = "GetManualDiagnosticPointStatus" }
    $lab = Invoke-Nidavellir @{ method = "GetDetectorLabStatus" }
    $trace = Invoke-Nidavellir @{ method = "GetGameTraceStatus" }
    if ($safe.data.safe_mode -or $safe.data.boot_flag_armed -or $safe.data.recovery_pending_ack -or $safe.data.gpu_reboot_required) {
        throw "Safe Loop is not ready for a game diagnostic"
    }
    if ($manual.data.active -or $lab.data.running -or $trace.data.running) {
        throw "A manual point, Detector Lab, or Game Trace session is already active"
    }

    $tdrBaseline = Get-LatestTdrEvent
    $apply = Invoke-Nidavellir @{
        method = "ApplyManualDiagnosticCurvePoint"
        params = @{
            target_mhz = $TargetMhz
            voltage_mv = $VoltageMv
        }
    }
    $curveApplied = $true
    $trace = Invoke-Nidavellir @{ method = "StartGameTrace" }
    $traceStarted = $true
    $startedAt = [DateTimeOffset]::UtcNow
    $lastReport = [DateTimeOffset]::MinValue
    Write-MonitorEvent "monitor_started" @{
        target_mhz = $TargetMhz
        requested_voltage_mv = $VoltageMv
        resolved_voltage_mv = $apply.data.resolved_voltage_mv
        application_mode = "anchored_curve"
        trace_path = $trace.data.out_path
        tdr_baseline = $tdrBaseline
    }

    while ($true) {
        Start-Sleep -Seconds $PollSeconds
        $trace = Invoke-Nidavellir @{ method = "GetGameTraceStatus" }
        $safe = Invoke-Nidavellir @{ method = "GetSafeLoopStatus" }
        $manual = Invoke-Nidavellir @{ method = "GetManualDiagnosticPointStatus" }
        $latestTdr = Get-LatestTdrEvent
        $newTdr = $null -ne $latestTdr -and (
            $null -eq $tdrBaseline -or $latestTdr.record_id -ne $tdrBaseline.record_id
        )
    $diagnosticReset = -not $safe.data.boot_flag_armed -and -not $safe.data.gpu_reboot_required

        if ($newTdr -or $diagnosticReset -or -not $trace.data.running) {
            $sentinelStatusPath = "C:\ProgramData\Nidavellir\sentinel_status.json"
            $sentinelStatus = if ([System.IO.File]::Exists($sentinelStatusPath)) {
                [System.IO.File]::ReadAllText($sentinelStatusPath)
            } else {
                $null
            }
            $reason = if ($newTdr) {
                "tdr"
            } elseif ($diagnosticReset) {
                "sentinel-reset-or-disarm"
            } else {
                "game-trace-stopped"
            }
            Write-MonitorEvent "failure_or_stop_detected" @{
                reason = $reason
                latest_tdr = $latestTdr
                sentinel_status = $sentinelStatus
                trace_status = $trace.data
                safe_loop = $safe.data
                manual_status = $manual.data
            }
            $exitCode = if ($newTdr) { 21 } else { 20 }
            break
        }

        if (([DateTimeOffset]::UtcNow - $lastReport).TotalSeconds -ge 30) {
            Write-MonitorEvent "heartbeat" @{
                elapsed_s = [long]([DateTimeOffset]::UtcNow - $startedAt).TotalSeconds
                samples = $trace.data.samples
                power_w = $trace.data.last_power_w
                clock_mhz = $trace.data.last_core_mhz
                voltage_mv = $trace.data.last_volt_mv
                canary_sequence = $null
            }
            $lastReport = [DateTimeOffset]::UtcNow
        }
    }
}
catch {
    $exitCode = 1
    Write-MonitorEvent "monitor_error" @{ error = $_.Exception.Message }
}
finally {
    if ($null -ne $writer -and $null -ne $reader -and $pipe.IsConnected) {
        if ($traceStarted) {
            try {
                $trace = Invoke-Nidavellir @{ method = "StopGameTrace" }
                Write-MonitorEvent "trace_stopped" @{ status = $trace.data }
            }
            catch {
                Write-MonitorEvent "cleanup_error" @{ operation = "StopGameTrace"; error = $_.Exception.Message }
            }
        }
        if ($curveApplied) {
            try {
                $manual = Invoke-Nidavellir @{ method = "ResetManualDiagnosticPoint" }
                Write-MonitorEvent "point_reset" @{ status = $manual.data }
            }
            catch {
                Write-MonitorEvent "cleanup_error" @{ operation = "ResetManualDiagnosticPoint"; error = $_.Exception.Message }
            }
        }
    }
    if ($null -ne $writer) {
        try { $writer.Dispose() } catch {}
    }
    if ($null -ne $reader) {
        try { $reader.Dispose() } catch {}
    }
    try { $pipe.Dispose() } catch {}
}

exit $exitCode

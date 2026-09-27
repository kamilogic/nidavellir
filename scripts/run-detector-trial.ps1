param(
    [Parameter(Mandatory = $true)]
    [ValidateRange(300, 3000)]
    [int]$TargetMhz,

    [Parameter(Mandatory = $true)]
    [ValidateRange(500, 1250)]
    [int]$VoltageMv,

    [Parameter(Mandatory = $true)]
    [ValidateSet("matrix_v27", "matrix_v26", "dx11_resident", "control_v25", "curve_v25", "dense_v14")]
    [string]$Recipe,

    [Parameter(Mandatory = $true)]
    [ValidateRange(15, 600)]
    [int]$DurationS,

    [Parameter(Mandatory = $true)]
    [string]$OutPath,

    [ValidateRange(1, 10)]
    [int]$PollSeconds = 2,

    [ValidateRange(10, 120)]
    [int]$StopGraceSeconds = 45
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Write-TrialEvent {
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
$labRunning = $false

try {
    $pipe.Connect(5000)
    $writer = [System.IO.StreamWriter]::new($pipe)
    $writer.AutoFlush = $true
    $reader = [System.IO.StreamReader]::new($pipe)

    $safe = Invoke-Nidavellir @{ method = "GetSafeLoopStatus" }
    $manual = Invoke-Nidavellir @{ method = "GetManualDiagnosticPointStatus" }
    $lab = Invoke-Nidavellir @{ method = "GetDetectorLabStatus" }
    if ($safe.data.safe_mode -or $safe.data.boot_flag_armed -or $safe.data.recovery_pending_ack -or $safe.data.gpu_reboot_required) {
        throw "Safe Loop is not ready for a diagnostic trial"
    }
    if ($lab.data.running) {
        throw "Detector Lab is already running"
    }
    if ($manual.data.active) {
        Invoke-Nidavellir @{ method = "ResetManualDiagnosticPoint" } | Out-Null
    }

    Write-TrialEvent "trial_start" @{
        target_mhz = $TargetMhz
        requested_voltage_mv = $VoltageMv
        recipe = $Recipe
        duration_s = $DurationS
        safe_loop = @{
            state = $safe.data.state
            safe_mode = $safe.data.safe_mode
            consecutive_crashes = $safe.data.consecutive_crashes
        boot_flag_armed = $safe.data.boot_flag_armed
        recovery_pending_ack = $safe.data.recovery_pending_ack
        gpu_reboot_required = $safe.data.gpu_reboot_required
            blacklist_count = @($safe.data.blacklist).Count
        }
    }

    $apply = Invoke-Nidavellir @{
        method = "ApplyManualDiagnosticPoint"
        params = @{
            target_mhz = $TargetMhz
            voltage_mv = $VoltageMv
        }
    }
    Write-TrialEvent "point_applied" @{ status = $apply.data }

    $start = Invoke-Nidavellir @{
        method = "StartDetectorLab"
        params = @{
            recipe = $Recipe
            duration_s = $DurationS
        }
    }
    $labRunning = $true
    Write-TrialEvent "lab_started" @{ status = $start.data }

    $expectedLoadSeconds = if ($Recipe -in @("matrix_v27", "matrix_v26")) {
        180 + 420 + (2 * 120) + 300
    } elseif ($Recipe -in @("control_v25", "curve_v25")) {
        2 * $DurationS
    } else {
        $DurationS
    }
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds($expectedLoadSeconds + 90)

    do {
        Start-Sleep -Seconds $PollSeconds
        $lab = Invoke-Nidavellir @{ method = "GetDetectorLabStatus" }
        $sensors = Invoke-Nidavellir @{ method = "ReadSensors" }
        $gpu = @($sensors.data.gpu)[0]
        Write-TrialEvent "sample" @{
            running = $lab.data.running
            stage = $lab.data.stage
            segment = $lab.data.current_segment
            phase = $lab.data.current_phase
            elapsed_ms = $lab.data.elapsed_ms
            progress_pct = $lab.data.progress_pct
            result = $lab.data.result
            failure_phase = $lab.data.failure_phase
            clock_mhz = $gpu.core_clock_mhz
            voltage_mv = $gpu.voltage_mv
            power_w = $gpu.power_w
            temperature_c = $gpu.temperature_c
            utilization_pct = $gpu.utilization_pct
        }

        if ([DateTimeOffset]::UtcNow -gt $deadline -and $lab.data.running) {
            Write-TrialEvent "deadline_exceeded" @{ deadline = $deadline.ToString("o") }
            Invoke-Nidavellir @{ method = "StopDetectorLab" } | Out-Null
            $stopDeadline = [DateTimeOffset]::UtcNow.AddSeconds($StopGraceSeconds)
            do {
                Start-Sleep -Seconds 1
                $lab = Invoke-Nidavellir @{ method = "GetDetectorLabStatus" }
            } while ($lab.data.running -and [DateTimeOffset]::UtcNow -lt $stopDeadline)
            if ($lab.data.running) {
                Write-TrialEvent "hard_hang" @{
                    stage = $lab.data.stage
                    segment = $lab.data.current_segment
                    phase = $lab.data.current_phase
                    action = "leave_boot_flag_armed_and_require_reboot"
                }
                exit 42
            }
        }
    } while ($lab.data.running)

    $labRunning = $false
    Write-TrialEvent "lab_finished" @{ status = $lab.data }

    $manual = Invoke-Nidavellir @{ method = "GetManualDiagnosticPointStatus" }
    if ($manual.data.active) {
        $reset = Invoke-Nidavellir @{ method = "ResetManualDiagnosticPoint" }
        Write-TrialEvent "point_reset" @{ status = $reset.data }
    }
    $safe = Invoke-Nidavellir @{ method = "GetSafeLoopStatus" }
    $sensors = Invoke-Nidavellir @{ method = "ReadSensors" }
    Write-TrialEvent "trial_finished" @{
        safe_loop = @{
            state = $safe.data.state
            safe_mode = $safe.data.safe_mode
            consecutive_crashes = $safe.data.consecutive_crashes
        boot_flag_armed = $safe.data.boot_flag_armed
        recovery_pending_ack = $safe.data.recovery_pending_ack
        gpu_reboot_required = $safe.data.gpu_reboot_required
            blacklist_count = @($safe.data.blacklist).Count
        }
        gpu = @($sensors.data.gpu)[0]
    }
    if ([string]$lab.data.result -ne "stable") {
    if ([string]$lab.data.result -eq "tdr" -or $safe.data.boot_flag_armed -or $safe.data.gpu_reboot_required) {
            exit 21
        }
        exit 20
    }
} catch {
    Write-TrialEvent "runner_error" @{
        message = $_.Exception.Message
        lab_running = $labRunning
    }
    throw
} finally {
    if ($null -ne $writer) {
        $writer.Flush()
    }
    $pipe.Dispose()
}

# One explicit development authorization, never a Start/Resume/Apply request.
# Without -Authorize this only reads readiness. The service must already be running with
# console --development-validation. A timeout has an unknown outcome: inspect, never auto-retry.
param(
    [switch]$Authorize,
    [string]$Reason = '',
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
if ($Authorize -and [Text.Encoding]::UTF8.GetByteCount($Reason.Trim()) -notin 8..500) {
    throw 'Use -Reason with 8 to 500 UTF-8 bytes describing the reviewed change and this validation.'
}
if ($OutputPath -and (Test-Path -LiteralPath $OutputPath)) { throw 'Output already exists; preserve the earlier evidence.' }
$pipe = [IO.Pipes.NamedPipeClientStream]::new('.', 'NidavellirCore', [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
$reader = $null
$writer = $null
function Request([string]$Method, $Parameters = $null) {
    $request = @{method=$Method}
    if ($null -ne $Parameters) { $request.params = $Parameters }
    $write = $writer.WriteLineAsync(($request | ConvertTo-Json -Compress))
    if (-not $write.Wait(5000)) { throw 'IPC write timeout. Outcome unknown; no retry was sent.' }
    [void]$write.GetAwaiter().GetResult()
    $read = $reader.ReadLineAsync()
    if (-not $read.Wait(30000)) { throw 'IPC response timeout. Outcome unknown; inspect readiness and the development audit before another action.' }
    $line = $read.GetAwaiter().GetResult()
    if ($null -eq $line) { throw 'IPC closed before response. Outcome unknown; no retry was sent.' }
    $response = $line | ConvertFrom-Json
    if ($response.ok -ne $true) { throw "${Method}: $($response.error)" }
    return $response.data
}
try {
    $pipe.Connect(5000)
    $writer = [IO.StreamWriter]::new($pipe, [Text.UTF8Encoding]::new($false), 1024, $true)
    $writer.AutoFlush = $true
    $reader = [IO.StreamReader]::new($pipe, [Text.Encoding]::UTF8, $false, 1024, $true)
    $report = [ordered]@{checkedUtc=[DateTime]::UtcNow.ToString('o');authorizationRequested=[bool]$Authorize}
    $report.safeLoop = Request 'GetSafeLoopStatus'
    $report.progress = Request 'GetPowerSweepProgress'
    $report.applied = Request 'GetAppliedProfile'
    if ($Authorize) {
        # The backend repeats readiness checks under its worker lease and verifies stock before
        # committing authorization. These reads alone never authorize GPU work.
        $report.progress = Request 'AuthorizeDevelopmentValidation' @{reason=$Reason.Trim()}
        $report.note = 'Authorization recorded. Start Standard manually in the UI. No workload was started by this command.'
    } else {
        $report.note = 'Read-only inspection. Use -Authorize -Reason only after reviewing the incident report and intended validation.'
    }
    $json = $report | ConvertTo-Json -Depth 14
    if ($OutputPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), $json, [Text.UTF8Encoding]::new($false)) }
    $json
} finally {
    # Cancel owned pending asynchronous I/O before disposing wrappers (which may flush).
    $pipe.Dispose()
    if ($reader) { try { $reader.Dispose() } catch [ObjectDisposedException] {} }
    if ($writer) { try { $writer.Dispose() } catch [ObjectDisposedException] {} }
}

param(
    [Parameter(Mandatory = $true)]
    [string]$Sequence,

    [Parameter(Mandatory = $true)]
    [ValidateSet("matrix_v27", "matrix_v26", "dx11_resident", "control_v25", "curve_v25", "dense_v14")]
    [string]$Recipe,

    [Parameter(Mandatory = $true)]
    [ValidateRange(15, 600)]
    [int]$DurationS,

    [Parameter(Mandatory = $true)]
    [string]$OutDir,

    [string]$Prefix = "paired"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$runner = Join-Path $PSScriptRoot "run-detector-trial.ps1"
[System.IO.Directory]::CreateDirectory($OutDir) | Out-Null
$counts = @{}

foreach ($rawPoint in $Sequence.Split(',')) {
    $point = $rawPoint.Trim()
    if ($point -notmatch '^(?<mhz>\d+)@(?<mv>\d+)$') {
        throw "Invalid point '$point'; expected target_mhz@requested_voltage_mv"
    }
    $targetMhz = [int]$Matches.mhz
    $voltageMv = [int]$Matches.mv
    $key = "$targetMhz-$voltageMv"
    $counts[$key] = 1 + [int]$counts[$key]
    $outPath = Join-Path $OutDir (
        "$Prefix-$key-$Recipe-r$($counts[$key]).jsonl"
    )

    & "$PSHOME\powershell.exe" `
        -NoProfile `
        -ExecutionPolicy Bypass `
        -File $runner `
        -TargetMhz $targetMhz `
        -VoltageMv $voltageMv `
        -Recipe $Recipe `
        -DurationS $DurationS `
        -OutPath $outPath
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Campaign stopped after $point with exit code $LASTEXITCODE"
        exit $LASTEXITCODE
    }
}

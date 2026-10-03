# J11: execute the real helper against injected Windows service responses. No host service I/O.
$ErrorActionPreference = 'Stop'
$helper = Join-Path (Split-Path -Parent $PSScriptRoot) 'apps\ui\src-tauri\windows\service-lifecycle.ps1'
$installDir = Join-Path $env:TEMP 'Nidavellir Installer Test'
$global:NidavellirInstallerFixture = $null

function New-Fixture([bool]$Installed = $false) {
    $global:NidavellirInstallerFixture = @{
        Service = $(if ($Installed) { [pscustomobject]@{ State = 'Running'; ExitCode = 0; ServiceSpecificExitCode = 0 } } else { $null })
        Calls = [Collections.Generic.List[string]]::new(); Fail = ''; Pending = $false; FailedStop = $false; BinaryExists = $true; PathName = ''
    }
}
function Get-CimInstance {
    param($ClassName, $Filter, $OperationTimeoutSec, $ErrorAction)
    if ($ClassName -ne 'Win32_Service' -or $Filter -ne "Name='NidavellirCore'") { throw 'Unexpected service query' }
    if ($global:NidavellirInstallerFixture.Fail -eq 'Read') { throw 'Injected service query failure' }
    $global:NidavellirInstallerFixture.Service
}
function Invoke-CimMethod {
    param($InputObject, $MethodName, $Arguments, $ClassName, $OperationTimeoutSec, $ErrorAction)
    $fixture = $global:NidavellirInstallerFixture
    $fixture.Calls.Add($MethodName)
    if ($fixture.Fail -eq $MethodName) { return [pscustomobject]@{ ReturnValue = 2 } }
    switch ($MethodName) {
        'Create' { $fixture.Service = [pscustomobject]@{ State = 'Stopped'; ExitCode = 0; ServiceSpecificExitCode = 0 }; $fixture.PathName = $Arguments.PathName }
        'Change' { $fixture.PathName = $Arguments.PathName }
        'StartService' { $fixture.Service.State = 'Running' }
        'StopService' {
            $fixture.Service.State = $(if ($fixture.Pending) { 'Stop Pending' } else { 'Stopped' })
            if ($fixture.FailedStop) { $fixture.Service.ExitCode = 1066; $fixture.Service.ServiceSpecificExitCode = 1 }
        }
        'Delete' { $fixture.Service = $null }
        default { throw "Unexpected service method: $MethodName" }
    }
    [pscustomobject]@{ ReturnValue = 0 }
}
# Functions outrank applications, so the helper's sc.exe recovery calls never reach the host.
function sc.exe {
    $global:NidavellirInstallerFixture.Calls.Add("sc:$($args[0])")
    $global:LASTEXITCODE = $(if ($global:NidavellirInstallerFixture.Fail -eq "sc:$($args[0])") { 5 } else { 0 })
}
function Test-Path {
    param($LiteralPath, $PathType)
    if ($PathType -ne 'Leaf' -or -not $LiteralPath.EndsWith('\nidavellir-service.exe')) { throw "Unexpected binary path: $LiteralPath" }
    $global:NidavellirInstallerFixture.BinaryExists
}
function Assert-Calls([string]$Expected) {
    $actual = $global:NidavellirInstallerFixture.Calls -join ','
    if ($actual -ne $Expected) { throw "Expected $Expected; got $actual" }
}
function Assert-Failure([string]$Action, [string]$Message) {
    try { & $helper -Action $Action -InstallDir $installDir -TimeoutSeconds 0 | Out-Null }
    catch {
        if ($_.Exception.Message -notlike "*$Message*") { throw }
        return
    }
    throw "Expected $Action to fail: $Message"
}

try {
    New-Fixture
    & $helper -Action Install -InstallDir $installDir | Out-Null
    Assert-Calls 'Create,sc:failure,sc:failureflag,sc:sdset,StartService'
    # Resolve like the helper does: CI's TEMP is an 8.3 path (RUNNER~1) that GetFullPath expands.
    $expectedPath = '"' + (Join-Path ([IO.Path]::GetFullPath($installDir)) 'nidavellir-service.exe') + '"'
    if ($global:NidavellirInstallerFixture.PathName -ne $expectedPath) { throw "Service executable path must retain quotes: $($global:NidavellirInstallerFixture.PathName)" }

    New-Fixture $true
    & $helper -Action Prepare -InstallDir $installDir | Out-Null
    Assert-Calls 'StopService'
    & $helper -Action Install -InstallDir $installDir | Out-Null
    Assert-Calls 'StopService,Change,sc:failure,sc:failureflag,sc:sdset,StartService'
    & $helper -Action Uninstall -InstallDir $installDir | Out-Null
    Assert-Calls 'StopService,Change,sc:failure,sc:failureflag,sc:sdset,StartService,StopService,Delete'

    New-Fixture
    & $helper -Action Uninstall -InstallDir $installDir | Out-Null
    Assert-Calls ''

    New-Fixture
    $global:NidavellirInstallerFixture.BinaryExists = $false
    Assert-Failure 'Install' 'binary is missing'
    Assert-Calls ''

    foreach ($operation in @('Create', 'Change', 'StartService', 'StopService', 'Delete')) {
        New-Fixture ($operation -ne 'Create')
        $global:NidavellirInstallerFixture.Fail = $operation
        $action = if ($operation -eq 'Delete') { 'Uninstall' } elseif ($operation -eq 'StopService') { 'Prepare' } else { 'Install' }
        Assert-Failure $action "$operation failed"
        if ($global:NidavellirInstallerFixture.Calls[-1] -ne $operation) { throw 'Workflow continued after a failure' }
    }

    foreach ($operation in @('sc:failure', 'sc:failureflag', 'sc:sdset')) {
        New-Fixture
        $global:NidavellirInstallerFixture.Fail = $operation
        Assert-Failure 'Install' 'could not be set'
        if ($global:NidavellirInstallerFixture.Calls[-1] -ne $operation) { throw 'Service started without recovery actions' }
    }

    New-Fixture $true
    $global:NidavellirInstallerFixture.Pending = $true
    Assert-Failure 'Uninstall' 'did not reach Stopped'
    Assert-Calls 'StopService'

    foreach ($action in @('Prepare', 'Install', 'Uninstall')) {
        New-Fixture $true
        $global:NidavellirInstallerFixture.FailedStop = $true
        Assert-Failure $action 'clean shutdown was not confirmed'
        Assert-Calls 'StopService'
        # The retry sees Stopped + nonzero exit; it must still refuse Change/Delete.
        Assert-Failure $action 'clean shutdown was not confirmed'
        Assert-Calls 'StopService'
    }

    New-Fixture
    $global:NidavellirInstallerFixture.Fail = 'Read'
    Assert-Failure 'Install' 'query failure'
    Assert-Calls ''
    Write-Host 'Installer lifecycle journeys passed with injected Windows services; no host changes.'
} finally {
    Remove-Variable NidavellirInstallerFixture -Scope Global
}

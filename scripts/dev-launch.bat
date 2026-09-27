@echo off
setlocal

cd /d "%~dp0.."
if errorlevel 1 (
    echo Falha: pasta do projeto nao encontrada.
    pause
    exit /b 1
)

echo Iniciando o Core Service em release...
echo Autorize a solicitacao de administrador do Windows.
powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\dev-service-admin.ps1" -Release
if errorlevel 1 (
    echo Falha ao abrir o servico. Verifique se outra instancia ja esta aberta.
    pause
    exit /b 1
)

echo Aguardando o Core Service responder por ate 3 minutos...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$ErrorActionPreference='Stop'; $deadline=[DateTime]::UtcNow.AddMinutes(3); $ready=$false; function RemainingMs { $left=($deadline-[DateTime]::UtcNow).TotalMilliseconds; if ($left -le 0) { throw 'Timeout' }; [int][Math]::Min(2000,[Math]::Max(1,$left)) }; do { $p=$null; $w=$null; $r=$null; try { $p=[IO.Pipes.NamedPipeClientStream]::new('.','NidavellirCore',[IO.Pipes.PipeDirection]::InOut,[IO.Pipes.PipeOptions]::Asynchronous); $p.Connect((RemainingMs)); $w=[IO.StreamWriter]::new($p,[Text.UTF8Encoding]::new($false),1024,$true); $w.AutoFlush=$true; $r=[IO.StreamReader]::new($p); $send=$w.WriteLineAsync((ConvertTo-Json -Compress @{method='Ping'})); if (-not $send.Wait((RemainingMs))) { throw 'Timeout' }; [void]$send.GetAwaiter().GetResult(); $read=$r.ReadLineAsync(); if (-not $read.Wait((RemainingMs))) { throw 'Timeout' }; $reply=ConvertFrom-Json -InputObject ($read.GetAwaiter().GetResult()); $ready=($reply.ok -eq $true -and $reply.data.type -eq 'Pong') } catch { } finally { if ($p) { $p.Dispose() }; if ($r) { try { $r.Dispose() } catch { } }; if ($w) { try { $w.Dispose() } catch { } } }; if ($ready) { exit 0 }; if ([DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 300 } } while ([DateTime]::UtcNow -lt $deadline); exit 1"
if errorlevel 1 (
    echo Falha: o servico nao respondeu em ate 3 minutos.
    echo Verifique a janela elevada antes de tentar novamente.
    pause
    exit /b 1
)

echo.
echo Abrindo a interface em outra janela. Aguarde o programa aparecer.
start "Nidavellir - Interface" /D "%CD%\apps\ui" cmd /d /k "npm.cmd run tauri:dev"
if errorlevel 1 (
    echo Falha ao abrir o terminal da interface.
    pause
    exit /b 1
)
echo.
echo Pronto. Soft Reset, Full Reset e Forge GPU sao feitos pela interface.
exit /b 0

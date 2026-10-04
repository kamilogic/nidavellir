; Checked lifecycle for the NVIDIA/Windows beta. NSIS already runs per-machine/elevated.
!define NIDAVELLIR_SERVICE_HELPER "${__FILEDIR__}\service-lifecycle.ps1"
!macro NidavellirServiceAction ACTION
  InitPluginsDir
  ; Embed the helper in installer and uninstaller; do not trust an older installed copy.
  File "/oname=$PLUGINSDIR\nidavellir-service-lifecycle.ps1" "${NIDAVELLIR_SERVICE_HELPER}"
  nsExec::ExecToStack /TIMEOUT=90000 '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$PLUGINSDIR\nidavellir-service-lifecycle.ps1" -Action ${ACTION} -InstallDir "$INSTDIR"'
  Pop $0
  Pop $1
  DetailPrint "$1"
  ${If} $0 != 0
    SetErrorLevel 1
    MessageBox MB_OK|MB_ICONSTOP "Core Service ${ACTION} did not complete ($0).$\r$\n$1$\r$\nInstallation cannot continue. Close the app and retry; restart Windows if the service cannot stop." /SD IDOK
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro NidavellirServiceAction Prepare
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro NidavellirServiceAction Install
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro NidavellirServiceAction Uninstall
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; These exact files shipped in the legacy CPU bundle. New packages no longer
  ; declare them as resources, so the generated uninstaller cannot remove them.
  Delete "$INSTDIR\resources\pawnio-modules\COPYING-PawnIO.Modules"
  Delete "$INSTDIR\resources\pawnio-modules\IntelMSR.bin"
  Delete "$INSTDIR\resources\pawnio-modules\LpcIO.bin"
  Delete "$INSTDIR\resources\pawnio-modules\README.md"
  Delete "$INSTDIR\resources\third_party\pawnio\PawnIO_setup.exe"
  Delete "$INSTDIR\resources\third_party\pawnio\README.md"
  ; Core binaries an update moved aside while their process was still exiting.
  Delete /REBOOTOK "$INSTDIR\nidavellir-service.*.old"
  ; Non-recursive: unrelated files remain. Never uninstall the shared PawnIO driver.
  RMDir "$INSTDIR\resources\pawnio-modules"
  RMDir "$INSTDIR\resources\third_party\pawnio"
  RMDir "$INSTDIR\resources\third_party"
  RMDir "$INSTDIR\resources"
  RMDir "$INSTDIR"
  ; ProgramData safety/learning history is intentionally retained.
!macroend

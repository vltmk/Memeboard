!macro NSIS_HOOK_PREINSTALL
  ; Stop any running MemeboardVirtualAudio service and wait for it to exit before replacing binaries
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop MemeboardVirtualAudio'
  Pop $0
  StrCpy $R9 0
  ${DoWhile} $R9 < 12
    nsExec::ExecToStack '"$SYSDIR\sc.exe" query MemeboardVirtualAudio'
    Pop $0
    Pop $1
    ${StrLoc} $2 $1 "STOPPED" ">"
    ${If} $2 != ""
      ${Break}
    ${EndIf}
    ${If} $0 != 0
      ${Break}
    ${EndIf}
    Sleep 400
    IntOp $R9 $R9 + 1
  ${Loop}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; 1. Install bundled WHQL-signed usbip-win2 driver if not already installed on this PC
  StrCpy $R0 "0"
  ReadRegStr $R1 HKLM "SYSTEM\CurrentControlSet\Services\usbip2_ude" "ImagePath"
  ${If} $R1 != ""
    ${If} ${FileExists} "$PROGRAMFILES64\USBip\usbip.exe"
      StrCpy $R0 "1"
    ${EndIf}
    ${If} ${FileExists} "$INSTDIR\driver\usbip\usbip.exe"
      StrCpy $R0 "1"
    ${EndIf}
  ${EndIf}

  ${If} $R0 == "0"
    ${If} ${FileExists} "$INSTDIR\driver\USBip-0.9.8.1-x64.exe"
      DetailPrint "Installing built-in USB Audio 2.0 virtual driver (usbip-win2)..."
      nsExec::ExecToLog '"$INSTDIR\driver\USBip-0.9.8.1-x64.exe" /SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS="main,client" /TASKS="vcredist" /DIR="$INSTDIR\driver\usbip"'
      Pop $0
      DetailPrint "usbip-win2 setup exited with code $0"
    ${EndIf}
  ${EndIf}

  ; 2. Register MemeboardVirtualAudio Windows Service (runs as LocalSystem, start/stop allowed for interactive users)
  DetailPrint "Configuring Memeboard Virtual Mic Windows Service..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete MemeboardVirtualAudio'
  Pop $0
  Sleep 400
  nsExec::ExecToLog '"$SYSDIR\sc.exe" create MemeboardVirtualAudio binPath= "\"$INSTDIR\${MAINBINARYNAME}.exe\" --service" start= demand DisplayName= "Memeboard Virtual Mic Service"'
  Pop $0
  nsExec::ExecToLog '"$SYSDIR\sc.exe" description MemeboardVirtualAudio "Provides the built-in low-latency USB Audio 2.0 virtual microphone endpoint for Memeboard."'
  Pop $0
  nsExec::ExecToLog '"$SYSDIR\sc.exe" sdset MemeboardVirtualAudio "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWRPWPLOCRRC;;;IU)(A;;CCLCSWLOCRRC;;;SU)"'
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Restore original default microphone and stop service cleanly before removing files
  ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
    nsExec::ExecToLog '"$INSTDIR\${MAINBINARYNAME}.exe" --prepare-uninstall'
    Pop $0
  ${EndIf}

  ; Stop and remove MemeboardVirtualAudio service
  DetailPrint "Stopping Memeboard Virtual Mic Service..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop MemeboardVirtualAudio'
  Pop $0
  Sleep 1000
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete MemeboardVirtualAudio'
  Pop $0

  ; Uninstall bundled usbip-win2 driver only if Memeboard installed it into $INSTDIR\driver\usbip
  ${If} ${FileExists} "$INSTDIR\driver\usbip\unins000.exe"
    DetailPrint "Removing bundled usbip-win2 virtual audio driver..."
    nsExec::ExecToLog '"$INSTDIR\driver\usbip\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$INSTDIR\driver"
!macroend

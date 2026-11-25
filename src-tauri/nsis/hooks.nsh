!macro NSIS_HOOK_PREINSTALL
  ; Stop any running MemeboardVirtualAudio service before overwriting binaries
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop MemeboardVirtualAudio'
  Pop $0
  Sleep 800
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; 1. Install bundled WHQL-signed usbip-win2 driver if not already installed on this PC
  StrCpy $R0 "0"
  ReadRegStr $R1 HKLM "SYSTEM\CurrentControlSet\Services\usbip2_ude" "ImagePath"
  ${If} $R1 != ""
    IfFileExists "$PROGRAMFILES64\USBip\usbip.exe" 0 +2
      StrCpy $R0 "1"
    IfFileExists "$INSTDIR\driver\usbip\usbip.exe" 0 +2
      StrCpy $R0 "1"
  ${EndIf}

  ${If} $R0 == "0"
    IfFileExists "$INSTDIR\driver\USBip-0.9.8.1-x64.exe" 0 +5
      DetailPrint "Installing built-in USB Audio 2.0 virtual driver (usbip-win2)..."
      nsExec::ExecToLog '"$INSTDIR\driver\USBip-0.9.8.1-x64.exe" /SP- /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /COMPONENTS="main,client" /TASKS="vcredist" /DIR="$INSTDIR\driver\usbip"'
      Pop $0
      DetailPrint "usbip-win2 setup exited with code $0"
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
  ; Stop and remove MemeboardVirtualAudio service
  DetailPrint "Stopping Memeboard Virtual Mic Service..."
  nsExec::ExecToLog '"$SYSDIR\sc.exe" stop MemeboardVirtualAudio'
  Pop $0
  Sleep 1000
  nsExec::ExecToLog '"$SYSDIR\sc.exe" delete MemeboardVirtualAudio'
  Pop $0

  ; Uninstall bundled usbip-win2 driver only if Memeboard installed it into $INSTDIR\driver\usbip
  IfFileExists "$INSTDIR\driver\usbip\unins000.exe" 0 +4
    DetailPrint "Removing bundled usbip-win2 virtual audio driver..."
    nsExec::ExecToLog '"$INSTDIR\driver\usbip\unins000.exe" /VERYSILENT /SUPPRESSMSGBOXES /NORESTART'
    Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$INSTDIR\driver"
!macroend

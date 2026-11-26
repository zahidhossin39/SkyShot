; Sky Shot used to be called Shutter. The installer keys everything by product name, so an
; update from Shutter would install Sky Shot next to it. After installing, remove the old
; program files, shortcuts and Installed-apps entry, and move "start with Windows" over.
; Settings and History live under the app id (unchanged), so they carry over untouched.
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Shutter"
  ${If} $0 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "${PRODUCTNAME}" "$INSTDIR\${MAINBINARYNAME}.exe"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Shutter"
  ${EndIf}
  ${If} "$LOCALAPPDATA\Shutter" != "$INSTDIR"
  ${AndIf} ${FileExists} "$LOCALAPPDATA\Shutter\${MAINBINARYNAME}.exe"
    RMDir /r /REBOOTOK "$LOCALAPPDATA\Shutter"
  ${EndIf}
  Delete "$SMPROGRAMS\Shutter.lnk"
  Delete "$DESKTOP\Shutter.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Shutter"
  DeleteRegKey HKCU "Software\${MANUFACTURER}\Shutter"
!macroend

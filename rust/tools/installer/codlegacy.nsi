; CodLegacy launcher installer (NSIS 3). Per-user: no administrator prompt.
;
;   makensis /DVERSION=0.1.0 /DEXE=path\to\codlegacy-launcher.exe /DOUT=CodLegacy-Setup.exe codlegacy.nsi
;
; Installs only the launcher. The launcher downloads CodLegacy itself (and keeps it up to date)
; into %LOCALAPPDATA%\CodLegacy\game, and never touches the player's Black Ops II files.
; Uninstalling removes the launcher and its shortcuts; the downloaded game build, the settings
; (with the launcher key) and Black Ops II are left alone.

Unicode true
!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !error "pass /DEXE=path\to\codlegacy-launcher.exe"
!endif
!ifndef OUT
  !define OUT "CodLegacy-Setup.exe"
!endif

!define APP "CodLegacy"
!define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\CodLegacy"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\CodLegacy"
InstallDirRegKey HKCU "Software\CodLegacy" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APP} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP} launcher"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APP} launcher setup"

!include "MUI2.nsh"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\codlegacy-launcher.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start the CodLegacy launcher"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  ; An older launcher may be running (self-update leaves a .old copy behind).
  Delete "$INSTDIR\codlegacy-launcher.exe.old"
  File "/oname=codlegacy-launcher.exe" "${EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\codlegacy-launcher.exe"
  CreateShortcut "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk" "$INSTDIR\Uninstall.exe"
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\codlegacy-launcher.exe"

  WriteRegStr HKCU "Software\CodLegacy" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNKEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNKEY}" "Publisher" "CodLegacy"
  WriteRegStr HKCU "${UNKEY}" "DisplayIcon" "$INSTDIR\codlegacy-launcher.exe"
  WriteRegStr HKCU "${UNKEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNKEY}" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\codlegacy-launcher.exe"
  Delete "$INSTDIR\codlegacy-launcher.exe.old"
  Delete "$INSTDIR\codlegacy-launcher.exe.new"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}\${APP}.lnk"
  Delete "$SMPROGRAMS\${APP}\Uninstall ${APP}.lnk"
  RMDir "$SMPROGRAMS\${APP}"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKCU "${UNKEY}"
  DeleteRegKey HKCU "Software\CodLegacy"
SectionEnd

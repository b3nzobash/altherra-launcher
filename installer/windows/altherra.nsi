; Інсталятор лаунчера Altherra для Windows.
; makensis /DVERSION=1.2.0 /DEXE=шлях\Altherra-windows-x64.exe /DOUT=Altherra-Setup-windows-x64.exe altherra.nsi
; Offline-версія: /DAPP="Altherra Offline" /DEXE_BASE=AltherraOffline /DREG_ID=AltherraOfflineLauncher
; Ставиться для поточного користувача в %LOCALAPPDATA%\Programs\Altherra — без прав адміністратора,
; і лаунчер може сам оновлювати свій файл.

Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef EXE
  !error "Вкажи /DEXE=шлях до Altherra-windows-x64.exe"
!endif
!ifndef OUT
  !define OUT "Altherra-Setup-windows-x64.exe"
!endif

!ifndef APP
  !define APP "Altherra"
!endif
!ifndef EXE_BASE
  !define EXE_BASE "Altherra"
!endif
!ifndef REG_ID
  !define REG_ID "AltherraLauncher"
!endif
!define EXE_NAME "${EXE_BASE}.exe"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${REG_ID}"

Name "${APP}"
OutFile "${OUT}"
InstallDir "$LOCALAPPDATA\Programs\${APP}"
InstallDirRegKey HKCU "Software\${REG_ID}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
BrandingText "${APP} Launcher ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP}"
VIAddVersionKey "FileDescription" "Інсталятор лаунчера ${APP}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "${APP}"

!ifndef ICON
  !define ICON "icon.ico"
!endif
!define MUI_ICON "..\..\assets\${ICON}"
!define MUI_UNICON "..\..\assets\${ICON}"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Встановлення ${APP}"
!define MUI_WELCOMEPAGE_TEXT "Лаунчер сам поставить Java, Minecraft, Fabric і моди сервера.$\r$\n$\r$\nПрава адміністратора не потрібні."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXE_NAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Запустити ${APP}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "Ukrainian"

Section "${APP}" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  ; Старий файл після самооновлення лаунчера.
  Delete "$INSTDIR\${EXE_BASE}.old.exe"
  File "/oname=${EXE_NAME}" "${EXE}"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\${APP}"
  CreateShortcut "$SMPROGRAMS\${APP}\${APP}.lnk" "$INSTDIR\${EXE_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP}\Видалити ${APP}.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\${REG_ID}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${APP}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "https://altherra.com.ua"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  IntFmt $0 "0x%08X" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Ярлик на робочому столі" SecDesktop
  CreateShortcut "$DESKTOP\${APP}.lnk" "$INSTDIR\${EXE_NAME}"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "Сам лаунчер."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktop} "Значок ${APP} на робочому столі."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Delete "$INSTDIR\${EXE_NAME}"
  Delete "$INSTDIR\${EXE_BASE}.old.exe"
  Delete "$INSTDIR\.altherra-update.tmp"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP}\${APP}.lnk"
  Delete "$SMPROGRAMS\${APP}\Видалити ${APP}.lnk"
  RMDir "$SMPROGRAMS\${APP}"
  Delete "$DESKTOP\${APP}.lnk"
  DeleteRegKey HKCU "${UNINST_KEY}"
  DeleteRegKey HKCU "Software\${REG_ID}"
  ; Світи, налаштування й акаунти (%APPDATA%\.altherra) не чіпаємо.
SectionEnd

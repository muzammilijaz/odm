; ODM installer hooks -- run after the app's own files are copied.
;
; Checks whether the VC++ 2015-2022 x64 runtime is already present (the
; standard detection key Microsoft's own installers use) and, only if it's
; missing, silently runs the bundled vc_redist.x64.exe (shipped inside this
; same setup.exe via tauri.conf.json's bundle.resources, landing at
; $INSTDIR\resources\vc_redist.x64.exe). Most Windows 10/11 machines already
; have this runtime from other software, so this is a no-op there.

!macro NSIS_HOOK_POSTINSTALL
  ReadRegDWORD $0 HKLM "SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64" "Installed"
  ${If} $0 == 1
    DetailPrint "VC++ x64 runtime already installed, skipping."
  ${Else}
    DetailPrint "Installing VC++ x64 runtime (required by bundled ffmpeg/yt-dlp)..."
    ExecWait '"$INSTDIR\resources\vc_redist.x64.exe" /install /quiet /norestart' $1
    DetailPrint "VC++ runtime installer exit code: $1"
  ${EndIf}

  ; Register the Native Messaging host (com.odm.nativehost) so the browser
  ; extension can reach this installed app -- see register-native-host.ps1,
  ; staged into resources/ by stage-extension.ps1 at build time. Writes only
  ; to HKCU, so no elevation beyond what the installer itself already has.
  DetailPrint "Registering native messaging host..."
  ExecWait 'powershell -ExecutionPolicy Bypass -File "$INSTDIR\resources\register-native-host.ps1"' $2
  DetailPrint "Native messaging host registration exit code: $2"

  ; The browser extension can't be silently installed -- Chrome blocks
  ; unsolicited .crx installs outside the Web Store (see extension/INSTALL.md)
  ; -- so the best we can do is open its folder and tell the user the two
  ; clicks left: chrome://extensions -> Developer mode -> Load unpacked.
  MessageBox MB_OK "ODM installed successfully.$\r$\n$\r$\nTo add the browser extension: this folder will open next -- go to chrome://extensions, turn on Developer mode (top-right), click 'Load unpacked', and select this folder."
  ExecShell "open" "$INSTDIR\resources\extension"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.odm.nativehost"
!macroend

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

  ; Chrome requires the user to confirm Web Store installations. Offer the
  ; official listing now that it is published; no Developer mode is needed.
  MessageBox MB_YESNO|MB_ICONINFORMATION "ODM installed successfully.$\r$\n$\r$\nWould you like to add the official ODM extension from the Chrome Web Store now?" IDNO +2
  ExecShell "open" "https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  SetRegView 32
  DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost"
  DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.odm.nativehost"
  ${If} ${RunningX64}
    SetRegView 64
    DeleteRegKey HKCU "Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost"
    DeleteRegKey HKCU "Software\Microsoft\Edge\NativeMessagingHosts\com.odm.nativehost"
  ${EndIf}
!macroend

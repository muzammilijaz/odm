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
!macroend

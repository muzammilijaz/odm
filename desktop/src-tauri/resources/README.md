# Bundled installer resources

`vc_redist.x64.exe` (the Microsoft Visual C++ 2015-2022 x64 redistributable)
is gitignored — it's a ~24MB third-party binary, not source. It gets bundled
into the NSIS installer (see `tauri.conf.json`'s `bundle.resources` and
`windows/hooks.nsh`, which silently installs it post-install only if the
runtime isn't already present on the target machine).

Fetch it before running `tauri build`:

```sh
curl -L "https://aka.ms/vs/17/release/vc_redist.x64.exe" -o vc_redist.x64.exe
```

(from this `resources/` directory).

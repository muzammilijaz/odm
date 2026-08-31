# Sidecar binaries

This folder holds third-party binaries the app shells out to. They're
gitignored (redistributable, ~180MB combined — not source, doesn't belong in
version control) and detected at runtime via `state.rs::set_bundled_binary_env_vars`,
which just checks whether each file exists — the app runs fine without them,
with those specific features (video-site extraction, HLS/DASH remuxing)
disabled until you add them.

Download and place these files here (Windows x86_64 shown; adjust the
`x86_64-pc-windows-msvc` suffix in filenames for other platforms):

| File | Source |
|---|---|
| `yt-dlp-x86_64-pc-windows-msvc.exe` | Latest release from [yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp/releases) — rename `yt-dlp.exe` to match. |
| `ffmpeg-x86_64-pc-windows-msvc.exe`, `ffprobe-x86_64-pc-windows-msvc.exe`, and the matching `avcodec-*.dll`/`avdevice-*.dll`/`avfilter-*.dll`/`avformat-*.dll`/`avutil-*.dll`/`swresample-*.dll`/`swscale-*.dll` | A shared (not static) Windows ffmpeg build, e.g. from [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds/releases) — pick a `-shared` release, take `ffmpeg.exe`/`ffprobe.exe` from `bin/` and rename, plus every DLL alongside them. |
| `quickjs-x86_64-pc-windows-msvc.exe` | A `qjs.exe`/`quickjs.exe` build (e.g. [quickjs-ng](https://github.com/quickjs-ng/quickjs/releases)) — gives yt-dlp a JS runtime for sites that need one (`--js-runtimes` in yt-dlp's own `--help`). Optional. |

All file names must match Tauri's sidecar convention
(`<name>-<target-triple>.exe`) exactly, since the same files double as the
`externalBin` sources for a packaged installer.

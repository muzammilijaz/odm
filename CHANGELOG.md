# Changelog

## v1.0.1 — Official Chrome Web Store Extension

### Browser integration

- Published the official ODM companion extension on the
  [Chrome Web Store](https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd).
- Replaced the old Developer mode and **Load unpacked** installation flow with
  a normal one-click **Add to Chrome** flow.
- Added an **Add Chrome Extension** button beside **Buy me a coffee** in the
  desktop app, plus another link in the About dialog.
- The Windows installer now offers to open the official Web Store listing as
  soon as setup finishes.

### Fixes

- Fixed `Access to the specified native messaging host is forbidden` for the
  published extension by registering ODM's native host consistently in both
  32-bit and 64-bit Windows registry views.
- Native-host uninstall cleanup now removes both registry-view entries.

## v1.0.0 — First Release 🎉

Free, open-source (AGPL-3.0) download manager for Windows. Everything below ships in a single installer — no separate downloads needed.

### 📥 Downloads

- **`desktop_1.0.0_x64-setup.exe`** — the desktop app. One installer, nothing else to install (bundles ffmpeg, yt-dlp, quickjs, and the VC++ runtime automatically).
- **`odm-extension.zip`** — browser extension for Chrome/Edge. See [installation options](extension/INSTALL.md) (Chrome blocks direct `.crx` installs outside the Web Store, so "Load unpacked" is the working path until the Web Store listing is approved).

### ✨ Highlights

**Downloading**
- Multi-connection chunked downloads with pause/resume, exponential backoff retry, and bandwidth throttling.
- Native HLS/DASH (adaptive streaming) support.
- Video/audio downloads from YouTube, TikTok, Instagram, and more via a bundled `yt-dlp`, with cookie-based sign-in for age/region-gated or bot-checked content.
- Live progress, speed, and auto-fetched title/thumbnail while a download is running — not just after it finishes.
- Categories with configurable file-extension rules and per-category save folders.

**Interface**
- Full UI redesign: sidebar navigation, sortable download table, details panel, right-click context menu (Open, Open with, Move/Rename, Redownload, Resume/Stop, Remove, Properties).
- Delete confirmation with an explicit "also delete from disk" checkbox.
- Opens maximized to whatever screen/resolution it launches on.

**Runs like a real background app**
- Minimizing or closing the window sends it to the system tray instead of quitting — downloads keep running.
- Auto-starts with Windows, launching straight into the tray (no window flash on boot).
- Checks GitHub Releases on startup and shows a dismissible banner when a newer version is available.

**Browser extension**
- Auto-captures ordinary browser downloads and hands them to ODM instead of Chrome's own downloader.
- Right-click any link or media → "Download with ODM".
- Detects downloadable video/audio streams on the page you're viewing.

**Everything else**
- 100% free, open source, AGPL-3.0 — no ads, no telemetry, no data collection of any kind.
- About dialog shows the app version and a link to support the project.

### 🔒 Privacy

Nothing leaves your device. The extension talks only to your own local ODM app over Native Messaging — see [PRIVACY.md](extension/PRIVACY.md).

### ☕ Support

If ODM is useful to you, consider [buying me a coffee](https://muzammilijaz.gumroad.com/coffee) — it helps keep free tools like this coming.

### 🐛 Known limitations

- Resuming a yt-dlp download that needs a video+audio merge doesn't always resume that exact merge cleanly (rare edge case, not blocking).
- Some sites' extractors break upstream in `yt-dlp` from time to time — use the in-app "Update yt-dlp" button, or wait for an upstream fix if it's very recent.

# ODM — Open Download Manager

A free, open-source desktop download manager for Windows, built with
Rust + Tauri + React. AGPL-3.0 licensed: free to use, free to modify, and any
distributed or hosted fork must stay open too.

[![Add ODM to Chrome](https://img.shields.io/badge/Chrome_Web_Store-Add_to_Chrome-4285F4?style=for-the-badge&logo=googlechrome&logoColor=white)](https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd)

The official ODM extension is now available on the Chrome Web Store — no
Developer mode, ZIP extraction, or **Load unpacked** steps are required.

## Features

- Multi-connection chunked downloads with pause/resume, retry with backoff,
  and bandwidth throttling.
- Native HLS/DASH (adaptive streaming) support.
- Video/audio site downloads (YouTube, TikTok, Instagram, and more) via a
  bundled `yt-dlp`, with cookie-based sign-in support for age/region-gated
  or bot-checked content.
- Categories with configurable file-extension rules and per-category save
  folders.
- A Chrome/Edge browser extension that auto-captures downloads from the
  browser, right-click "Download with ODM," and an in-page video-grabber
  button — all relayed to the desktop app over a local loopback API.

## Project layout

- `crates/odm-core` — task queue, SQLite persistence, settings, orchestration.
- `crates/odm-engine` — download engines (native chunked HTTP, native
  HLS/DASH, yt-dlp-backed).
- `crates/odm-native-host` — Native Messaging host bridging the browser
  extension to the desktop app.
- `desktop/` — the Tauri + React desktop application.
- `extension/` — the Manifest V3 browser extension.

## Installation

### 1. Install the desktop app

1. Download the latest `ODM_x.x.x_x64-setup.exe` from the
   [Releases](https://github.com/muzammilijaz/odm/releases) page.
2. Run the installer. It will:
   - Install the app to your chosen folder.
   - Silently install the VC++ runtime if it isn't already on your system
     (required by the bundled ffmpeg/yt-dlp).
   - Register the Native Messaging host so the browser extension can talk
     to the app — no manual setup needed.
   - Offer to open the official Chrome Web Store listing so you can add the
     extension with one click.
3. Launch **ODM** from the Start Menu.

### 2. Install the browser extension

Click the badge below, then select **Add to Chrome**. The desktop installer
also offers to open this page automatically when installation finishes.

[![Install the official ODM browser extension](https://img.shields.io/badge/Install_Official_Extension-Chrome_Web_Store-34A853?style=for-the-badge&logo=googlechrome&logoColor=white)](https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd)

If you previously used the unpacked extension, remove it from
`chrome://extensions` after installing the Web Store version to avoid running
two copies. Developer mode is no longer needed.

### 3. Using it

- Right-click any link, image, or video on a page → **"Download with ODM"**.
- Ordinary browser downloads are automatically handed off to ODM instead of
  Chrome's own downloader (toggle this in the extension popup).
- On video pages (YouTube, TikTok, etc.), a floating "Download this video"
  button appears over the player.

See [extension/PRIVACY.md](extension/PRIVACY.md) for what data the extension
touches and where it goes (short answer: nowhere but your own machine).

### Uninstalling

Uninstall ODM from Windows Settings → Apps like any other program — this
also removes the Native Messaging registration. Remove the browser
extension separately from `chrome://extensions`.

## Building

```sh
cargo build --workspace
cd desktop && npm install && npm run tauri dev
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full dev setup, including how
to build and load the browser extension.

## License

[AGPL-3.0](LICENSE). Contributions welcome — see
[CONTRIBUTING.md](CONTRIBUTING.md).

## About

Built by **Muzammil Ijaz**. More projects: <https://github.com/muzammilijaz>

## ☕ Support My Work

If my projects help you, consider buying me a coffee ❤️

👉 <https://muzammilijaz.gumroad.com/coffee>

Every contribution helps me continue building free tools and open-source
projects.

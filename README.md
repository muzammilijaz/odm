# ODM — Open Download Manager

A free, open-source desktop download manager for Windows, built with
Rust + Tauri + React. AGPL-3.0 licensed: free to use, free to modify, and any
distributed or hosted fork must stay open too.

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
   - Open the extension folder and show a short reminder for step 2 below.
3. Launch **ODM** from the Start Menu.

### 2. Install the browser extension

**Option A — Chrome Web Store** (once the listing is approved — check for a
"CWS" badge/link on the [Releases](https://github.com/muzammilijaz/odm/releases)
page or the project's GitHub page): click "Add to Chrome" and you're done.

**Option B — Load unpacked** (works today, before/without store approval):

1. Download `odm-extension.zip` from the
   [Releases](https://github.com/muzammilijaz/odm/releases) page (the same
   installer step above also opens this folder for you if you installed the
   app first).
2. Unzip it anywhere on your computer.
3. Open `chrome://extensions` (or `edge://extensions` for Microsoft Edge).
4. Turn on **Developer mode** (top-right toggle).
5. Click **Load unpacked** and select the unzipped folder.

The extension works exactly the same either way — Chrome just shows a
"Developer mode extensions" notice on startup for a side-loaded (Option B)
install, which is normal and not specific to ODM. Once the Web Store
listing is approved, you can remove the unpacked version and switch to
Option A if you'd rather not see that notice.

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

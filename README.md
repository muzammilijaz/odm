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

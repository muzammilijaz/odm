# Contributing to ODM

Thanks for considering contributing — ODM is free and open source (AGPL-3.0)
and welcomes contributions from anyone.

## Ground rules

- ODM is licensed under AGPL-3.0 (see [LICENSE](LICENSE)). By submitting a
  contribution, you agree it's licensed under the same terms.
- No commercial forks or paid redistribution — that's the whole point of
  AGPL-3.0 here: everyone gets it free, and any modified version (including
  one run as a hosted service) must also share its source under AGPL-3.0.

## Project layout

- `crates/odm-core` — task queue, SQLite persistence, settings, the
  `TaskManager` orchestrating downloads.
- `crates/odm-engine` — the download engines: native chunked HTTP, native
  HLS/DASH, and the yt-dlp-backed engine for sites needing extraction.
- `crates/odm-native-host` — the Chrome/Edge Native Messaging host bridging
  the browser extension to the desktop app's local HTTP API.
- `desktop/` — the Tauri + React desktop app (the actual GUI).
- `extension/` — the browser extension (Manifest V3).

## Getting set up

```sh
cargo build --workspace
cd desktop && npm install
```

Run the desktop app in dev mode with Tauri's CLI (`npm run tauri dev` from
`desktop/`, or `cargo tauri dev` if you have the Tauri CLI installed).

For the browser extension, see `keys/README.md` first — you'll need to
generate your own local signing key before loading it unpacked in Chrome.

## Making a change

1. Fork the repo and create a branch off `main`.
2. Keep changes focused — one logical change per PR is easier to review.
3. Run `cargo test --workspace` before opening a PR.
4. Open a PR describing what changed and why.

## Reporting bugs / requesting features

Open a GitHub issue. Include steps to reproduce, what you expected, and what
actually happened. For download-site-specific issues (a site suddenly
failing to extract), please check whether it's an upstream `yt-dlp` issue
first — those need to be fixed there, not in ODM.

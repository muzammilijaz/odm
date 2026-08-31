# Publishing ODM to the Chrome Web Store

Everything below is what the Chrome Web Store developer dashboard will ask
for. Nothing here can be automated from outside the dashboard — this is a
checklist plus ready-to-paste text.

## 1. Package the extension

```powershell
cd extension
.\package-for-store.ps1
```

This produces `odm-extension.zip` containing only what Chrome loads
(manifest, scripts, icons) — no dev-only files like
`native-host-manifest/`, `install-native-host.ps1`, or this guide.

## 2. One-time setup

1. Register as a Chrome Web Store developer (one-time $5 fee):
   <https://chrome.google.com/webstore/devconsole>
2. Click "New item", upload `odm-extension.zip`.

Because `manifest.json` already includes the `"key"` field pinned to
`keys/public_key_base64.txt`, the published extension keeps the **same ID**
(`igjebnkcfkjpleeahgjnpdkahplddfdc`) as your local dev build — required so
the native messaging host's `allowed_origins` doesn't need to change between
testing and the published version.

## 3. Store listing fields

**Category**: Productivity (or Tools)

**Language**: English

**Short description** (132 char max) — paste as-is:

> Send browser downloads, links, and detected videos to the free, open-source ODM desktop download manager.

**Detailed description** — paste as-is (edit as you like):

> ODM (Open Download Manager) is a free, open-source, AGPL-3.0-licensed
> download manager. This extension is its browser companion:
>
> - Auto-captures ordinary browser downloads and hands them to ODM for
>   faster multi-connection downloading, instead of Chrome's own downloader.
> - Right-click any link or media → "Download with ODM".
> - Detects downloadable video/audio streams (HLS/DASH) on the page you're
>   viewing and lets you grab them with one click.
>
> Requires the free ODM desktop app (Windows):
> https://github.com/muzammilijaz/odm
>
> 100% open source, no ads, no tracking, no data collection of any kind —
> everything the extension does stays on your own computer, communicating
> only with the ODM app running locally. See the full privacy policy:
> https://github.com/muzammilijaz/odm/blob/main/extension/PRIVACY.md

**Privacy policy URL**:

```
https://github.com/muzammilijaz/odm/blob/main/extension/PRIVACY.md
```

## 4. Privacy practices tab (permission justifications)

Chrome will ask you to justify each permission. Paste these in:

| Permission | Justification |
|---|---|
| `downloads` | Needed to detect when the browser starts a download (`chrome.downloads.onCreated`) so it can be handed off to the ODM desktop app instead, and to cancel/erase Chrome's own copy of that download once ODM has accepted it. |
| `webRequest` | Read-only use (no blocking/modification) to inspect response headers (content-type, content-length) and detect downloadable video/audio streams (HLS/DASH manifests) on the current page, so the user can download them via the in-page button or popup. |
| `contextMenus` | Adds "Download with ODM" to the right-click menu for links and media. |
| `storage` | Persists the user's "auto-capture" preference locally and caches per-tab detected-stream lists for the popup to display. |
| `nativeMessaging` | The only way for a browser extension to talk to a desktop application — used exclusively to relay download requests to the local ODM app on 127.0.0.1. |
| Host permission `<all_urls>` | The extension's entire purpose is capturing downloads and detecting media streams on *any* site the user browses — a fixed allowlist of sites isn't possible for a general-purpose download manager. No page content is read or altered; only network response headers and download events are observed. |

**Are you using remote code?** No.

**Does your extension collect user data?** No user data is collected,
transmitted to, or stored by any server operated by the developer — see
the Privacy practices section and [PRIVACY.md](PRIVACY.md). All the data
listed above (URLs, headers) either stays local to the browser
(`chrome.storage`) or is sent only to the user's own local desktop app via
Native Messaging, never over the network.

## 5. Screenshots

Chrome requires at least one screenshot (1280x800 or 640x400 PNG/JPEG).
Suggested shots:
1. The popup (`popup.html`) showing the toggle and detected-links list.
2. The right-click "Download with ODM" context menu on a link.
3. The floating "Download this video" badge over a video player.

Promotional tile images (440x280, 920x680, 1400x560) are optional but
improve store visibility — not required to publish.

## 6. After publishing

Once approved, Chrome may take a few hours to a few days for review (broad
host permissions like `<all_urls>` and `nativeMessaging` typically get a
closer look, but are commonly approved for legitimate download-manager
extensions when justified as above).

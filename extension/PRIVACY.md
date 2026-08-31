# ODM Browser Extension — Privacy Policy

**No data ever leaves your device.** ODM does not collect, transmit, sell,
or share any of your data with us or any third party — there is no backend
server for this extension to talk to.

## What the extension sees and what it does with it

| Data | Why it's needed | Where it goes |
|---|---|---|
| Links you right-click ("Download with ODM") | To queue that download | Sent only to the ODM desktop app running on **your own computer** (`127.0.0.1`, via Chrome's Native Messaging API) — never to any remote server. |
| Response headers (content-type, content-length) of network requests, read via the `webRequest` permission | To detect downloadable video/audio streams (e.g. HLS/DASH manifests) on the page you're viewing | Kept in memory / `chrome.storage.session` for that browser tab only, cleared when you navigate away or close the tab. Never transmitted anywhere unless you click "Add" on a detected item, at which point that one URL is sent to your local ODM app the same way as above. |
| URLs of files your browser starts downloading (`chrome.downloads.onCreated`), when "Auto-capture downloads" is on | To hand the download to ODM instead of Chrome's own downloader | Sent only to your local ODM app, same as above. |
| The "Auto-capture downloads" toggle state | To remember your preference | Stored locally via `chrome.storage.local`, on your device only. |

## What we don't do

- No analytics, telemetry, or tracking of any kind.
- No ads, no ad networks, no data brokers.
- No account, sign-in, or user identifiers of any kind.
- No remote code execution — everything the extension runs ships in the
  reviewed package; nothing is fetched or `eval`'d at runtime.

## Native Messaging

This extension only functions alongside the free, open-source ODM desktop
app (source: <https://github.com/muzammilijaz/odm>). Communication happens
exclusively over a local Native Messaging connection to a small helper
process running on your own machine — nothing crosses the network.

## Changes to this policy

If this policy ever changes, the update will be reflected in this file at
<https://github.com/muzammilijaz/odm/blob/main/extension/PRIVACY.md> and the
version history is publicly visible via git.

## Contact

Open an issue at <https://github.com/muzammilijaz/odm/issues> for any
privacy questions.

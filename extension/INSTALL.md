# Installing the ODM extension outside the Chrome Web Store

## Important: `.crx` files can't just be dragged into Chrome anymore

Since around 2014, Chrome (and Edge) refuse to silently install a `.crx`
file downloaded from outside the Chrome Web Store — dragging it onto
`chrome://extensions` does nothing on the stable channel, even with
Developer Mode on. This is a hard anti-malware restriction in the browser
itself, not something a `.crx` file or this project can work around. The
`.crx` build here (`odm-extension.crx`, produced by `package-crx.ps1`) is
provided for completeness and for enterprise policy installs
(`ExtensionInstallForcelist`) — **it is not a working "just click to
install" path for a typical user.**

The two ways that actually work:

## Option A — Chrome Web Store (recommended, once published)

Once published (see [CHROME_WEB_STORE.md](CHROME_WEB_STORE.md)), it's a
normal one-click "Add to Chrome" install like any other extension.

## Option B — Load unpacked (works today, no store needed)

1. Download and unzip `odm-extension.zip` (from
   [Releases](https://github.com/muzammilijaz/odm/releases), or build it
   yourself with `package-for-store.ps1`) anywhere on your computer.
2. Open `chrome://extensions` (or `edge://extensions` for Edge).
3. Turn on **Developer mode** (top-right toggle).
4. Click **Load unpacked** and select the unzipped folder.

The extension keeps working exactly the same way after this — Chrome just
shows a "Developer mode extensions" notice on startup, which is normal for
any side-loaded extension and not specific to ODM.

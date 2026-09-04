# Installing the ODM browser extension

## Chrome Web Store — recommended

The official extension is published and can be installed without Developer
mode:

[![Add ODM to Chrome](https://img.shields.io/badge/Chrome_Web_Store-Add_to_Chrome-4285F4?style=for-the-badge&logo=googlechrome&logoColor=white)](https://chromewebstore.google.com/detail/odm-open-download-manager/lfpiggopnkjdgedghgapjnmijgckebkd)

Click the badge and select **Add to Chrome**. Install and run the ODM desktop
app as well; it registers the local Native Messaging host automatically.

The instructions below are retained only for extension development and
enterprise deployment.

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

## Development fallback — Load unpacked

1. Download and unzip `odm-extension.zip` (from
   [Releases](https://github.com/muzammilijaz/odm/releases), or build it
   yourself with `package-for-store.ps1`) anywhere on your computer.
2. Open `chrome://extensions` (or `edge://extensions` for Edge).
3. Turn on **Developer mode** (top-right toggle).
4. Click **Load unpacked** and select the unzipped folder.

The extension keeps working exactly the same way after this — Chrome just
shows a "Developer mode extensions" notice on startup, which is normal for
any side-loaded extension and not specific to ODM.

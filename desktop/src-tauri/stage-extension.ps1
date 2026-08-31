# Copies the browser-loadable extension files into resources/extension/ so
# tauri.conf.json's bundle.resources can ship them inside the installer.
# Run automatically by Tauri's `beforeBundleCommand` -- see tauri.conf.json.
# Mirrors the same file list as extension/package-for-store.ps1's Chrome Web
# Store zip: manifest, scripts, icons, nothing dev-only.

$ErrorActionPreference = "Stop"
$srcTauri = $PSScriptRoot
$extensionSrc = Resolve-Path (Join-Path $srcTauri "..\..\extension")
$dest = Join-Path $srcTauri "resources\extension"

if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
New-Item -ItemType Directory -Path $dest | Out-Null

$include = @("manifest.json", "background.js", "content.js", "content.css", "popup.html", "popup.js", "icons")
foreach ($item in $include) {
    Copy-Item (Join-Path $extensionSrc $item) (Join-Path $dest $item) -Recurse
}

Write-Host "Staged extension resource at $dest"

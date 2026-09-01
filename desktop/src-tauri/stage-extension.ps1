# Copies the browser-loadable extension files into resources/extension/ so
# tauri.conf.json's bundle.resources can ship them inside the installer.
# Run automatically by Tauri's `beforeBundleCommand` -- see tauri.conf.json.
# Mirrors the same file list as extension/package-for-store.ps1's Chrome Web
# Store zip: manifest, scripts, icons, nothing dev-only.
#
# Also stages the odm-native-host.exe binary and its manifest template so
# the installer can register Native Messaging itself on install -- see
# register-native-host.ps1 and windows/hooks.nsh.

$ErrorActionPreference = "Stop"
$srcTauri = $PSScriptRoot
$repoRoot = Resolve-Path (Join-Path $srcTauri "..\..")
$extensionSrc = Resolve-Path (Join-Path $repoRoot "extension")
$dest = Join-Path $srcTauri "resources\extension"

if (Test-Path $dest) { Remove-Item $dest -Recurse -Force }
New-Item -ItemType Directory -Path $dest | Out-Null

$include = @("manifest.json", "background.js", "content.js", "content.css", "popup.html", "popup.js", "icons")
foreach ($item in $include) {
    Copy-Item (Join-Path $extensionSrc $item) (Join-Path $dest $item) -Recurse
}

Write-Host "Staged extension resource at $dest"

$nativeHostExe = Join-Path $repoRoot "target\release\odm-native-host.exe"
if (-not (Test-Path $nativeHostExe)) {
    Write-Error "odm-native-host.exe not found at $nativeHostExe`nBuild it first: cargo build -p odm-native-host --release"
}
Copy-Item $nativeHostExe (Join-Path $srcTauri "resources\odm-native-host.exe") -Force

$manifestTemplateSrc = Join-Path $extensionSrc "native-host-manifest\com.odm.nativehost.json"
Copy-Item $manifestTemplateSrc (Join-Path $srcTauri "resources\com.odm.nativehost.template.json") -Force

$registerScriptSrc = Join-Path $srcTauri "windows\register-native-host.ps1"
Copy-Item $registerScriptSrc (Join-Path $srcTauri "resources\register-native-host.ps1") -Force

Write-Host "Staged native messaging host binary, manifest template, and registration script"

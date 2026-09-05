# Packages a clean .zip of the extension for Chrome Web Store upload --
# includes only what the browser actually loads (manifest, scripts, icons),
# leaving out dev-only files (native-host-manifest/, install script, this
# script itself) that have no place in the uploaded package.

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$outZip = Join-Path $root "odm-extension.zip"

if (Test-Path $outZip) { Remove-Item $outZip -Force }

$include = @(
    "manifest.json",
    "background.js",
    "content.js",
    "content.css",
    "popup.html",
    "popup.js",
    "icons"
)

$stagingDir = Join-Path $env:TEMP "odm-extension-staging"
if (Test-Path $stagingDir) { Remove-Item $stagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $stagingDir | Out-Null

foreach ($item in $include) {
    Copy-Item (Join-Path $root $item) (Join-Path $stagingDir $item) -Recurse
}

# The public key pins a stable ID when the source folder is loaded unpacked
# during development. Chrome Web Store assigns the published ID itself, so
# strip the development-only field from the upload manifest.
$stagedManifestPath = Join-Path $stagingDir "manifest.json"
$stagedManifest = Get-Content $stagedManifestPath -Raw | ConvertFrom-Json
$stagedManifest.PSObject.Properties.Remove("key")
$manifestJson = $stagedManifest | ConvertTo-Json -Depth 10
[System.IO.File]::WriteAllText(
    $stagedManifestPath,
    $manifestJson,
    [System.Text.UTF8Encoding]::new($false)
)

Compress-Archive -Path (Join-Path $stagingDir "*") -DestinationPath $outZip
Remove-Item $stagingDir -Recurse -Force

Write-Host "Packaged: $outZip"
Write-Host "Upload this file directly at https://chrome.google.com/webstore/devconsole"

# Packs the extension into a signed .crx using the pinned private key
# (keys/key.pem), so the resulting package's extension ID always matches
# igjebnkcfkjpleeahgjnpdkahplddfdc -- same ID as the Chrome Web Store build
# and the local dev build, since all three are signed from the same key.

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$repoRoot = Resolve-Path (Join-Path $root "..")
$keyPath = Join-Path $repoRoot "keys\key.pem"

if (-not (Test-Path $keyPath)) {
    Write-Error "Private key not found at $keyPath -- see keys/README.md to generate one."
}

$chrome = @(
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $chrome) {
    Write-Error "Chrome not found. Install Chrome, or pack manually via chrome://extensions -> Pack extension."
}

$stagingDir = Join-Path $env:TEMP "odm-crx-staging"
if (Test-Path $stagingDir) { Remove-Item $stagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $stagingDir | Out-Null

$include = @("manifest.json", "background.js", "content.js", "content.css", "popup.html", "popup.js", "icons")
foreach ($item in $include) {
    Copy-Item (Join-Path $root $item) (Join-Path $stagingDir $item) -Recurse
}

& $chrome --pack-extension="$stagingDir" --pack-extension-key="$keyPath" | Out-Null
Start-Sleep -Seconds 2

$producedCrx = "$stagingDir.crx"
$outCrx = Join-Path $root "odm-extension.crx"
if (Test-Path $producedCrx) {
    Move-Item $producedCrx $outCrx -Force
    Write-Host "Packaged: $outCrx"
} else {
    Write-Error "Chrome did not produce a .crx -- check for an error dialog it may have shown."
}

Remove-Item $stagingDir -Recurse -Force

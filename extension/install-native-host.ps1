# Registers the ODM native messaging host with Chrome and Edge on Windows.
#
# Run this after `cargo build -p odm-native-host --release` (or with no
# extra args to pick up a debug build for local development).
#
# What this does:
#   1. Writes a native-messaging-host manifest JSON next to this script,
#      pointing "path" at the built odm-native-host(.exe).
#   2. Registers that manifest under
#      HKCU\Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost
#      and the equivalent Edge key, per Chrome's Native Messaging spec.
#
# The extension ID baked into com.odm.nativehost.json's "allowed_origins"
# corresponds to the signing key committed at extension/keys/key.pem, which
# also produces the manifest.json "key" field -- so as long as the extension
# is loaded from this repo, the ID (and this registration) stay stable.

param(
    [ValidateSet("debug", "release")]
    [string]$Profile = "debug"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$exePath = Join-Path $repoRoot "target\$Profile\odm-native-host.exe"

if (-not (Test-Path $exePath)) {
    Write-Error "Native host binary not found at $exePath`nBuild it first: cargo build -p odm-native-host $(if ($Profile -eq 'release') { '--release' })"
}

$manifestTemplate = Join-Path $PSScriptRoot "native-host-manifest\com.odm.nativehost.json"
$manifest = Get-Content $manifestTemplate -Raw | ConvertFrom-Json
# Join-Path returns a plain string, not a path object -- `.Path` on a string
# silently resolves to $null in PowerShell, which is how this manifest ended
# up with "path": null on a previous run. Use the string directly.
$manifest.path = (Resolve-Path $exePath).Path
$installedManifestPath = Join-Path $PSScriptRoot "native-host-manifest\com.odm.nativehost.installed.json"
$manifestJson = $manifest | ConvertTo-Json -Depth 5
[System.IO.File]::WriteAllText(
    $installedManifestPath,
    $manifestJson,
    [System.Text.UTF8Encoding]::new($false)
)

function Register-NativeHost([Microsoft.Win32.RegistryView]$view) {
    $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey(
        [Microsoft.Win32.RegistryHive]::CurrentUser,
        $view
    )

    try {
        foreach ($subKeyPath in @(
            "Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost",
            "Software\Microsoft\Edge\NativeMessagingHosts\com.odm.nativehost"
        )) {
            $subKey = $baseKey.CreateSubKey($subKeyPath)
            try {
                $subKey.SetValue("", $installedManifestPath, [Microsoft.Win32.RegistryValueKind]::String)
            }
            finally {
                $subKey.Dispose()
            }
        }
    }
    finally {
        $baseKey.Dispose()
    }
}

$registryViews = @([Microsoft.Win32.RegistryView]::Registry32)
if ([Environment]::Is64BitOperatingSystem) {
    $registryViews += [Microsoft.Win32.RegistryView]::Registry64
}
foreach ($view in $registryViews) {
    Register-NativeHost $view
}

Write-Host "Registered ODM native messaging host:"
Write-Host "  Binary:   $exePath"
Write-Host "  Manifest: $installedManifestPath"
Write-Host ""
Write-Host "Next: open chrome://extensions, enable Developer mode, 'Load unpacked'"
Write-Host "and select the extension/ folder. Its ID should be igjebnkcfkjpleeahgjnpdkahplddfdc"
Write-Host "(pinned by extension/manifest.json's 'key' field) -- matching the allowed_origins"
Write-Host "already baked into the manifest above."

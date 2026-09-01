# Registers the ODM native messaging host with Chrome and Edge, pointing at
# this installation's own odm-native-host.exe. Run automatically by the NSIS
# installer's post-install hook -- see windows/hooks.nsh. Mirrors
# extension/install-native-host.ps1, which does the same thing for local dev
# builds instead of an installed app.

$ErrorActionPreference = "Stop"
$resourcesDir = $PSScriptRoot
$exePath = Join-Path $resourcesDir "odm-native-host.exe"

$manifestTemplate = Join-Path $resourcesDir "com.odm.nativehost.template.json"
$manifest = Get-Content $manifestTemplate -Raw | ConvertFrom-Json
$manifest.path = $exePath
$installedManifestPath = Join-Path $resourcesDir "com.odm.nativehost.installed.json"
$manifest | ConvertTo-Json -Depth 5 | Set-Content -Path $installedManifestPath -Encoding utf8

function Register-NativeHost($registryRoot) {
    $keyPath = "$registryRoot\Software\Google\Chrome\NativeMessagingHosts\com.odm.nativehost"
    New-Item -Path $keyPath -Force | Out-Null
    Set-ItemProperty -Path $keyPath -Name "(default)" -Value $installedManifestPath

    $edgeKeyPath = "$registryRoot\Software\Microsoft\Edge\NativeMessagingHosts\com.odm.nativehost"
    New-Item -Path $edgeKeyPath -Force | Out-Null
    Set-ItemProperty -Path $edgeKeyPath -Name "(default)" -Value $installedManifestPath
}

Register-NativeHost "HKCU:"

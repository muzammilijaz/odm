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

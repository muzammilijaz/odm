# Build and register the debug Native Messaging host before `tauri dev`
# starts Vite. npm automatically runs this script through the `predev` hook.

$ErrorActionPreference = "Stop"
$desktopDir = $PSScriptRoot
$repoRoot = Resolve-Path (Join-Path $desktopDir "..")

Push-Location $repoRoot
try {
    cargo build -p odm-native-host
    if ($LASTEXITCODE -ne 0) {
        throw "Could not build odm-native-host (cargo exit code $LASTEXITCODE)"
    }
}
finally {
    Pop-Location
}

& (Join-Path $repoRoot "extension\install-native-host.ps1") -Profile debug
if ($LASTEXITCODE -ne 0) {
    throw "Could not register com.odm.nativehost (exit code $LASTEXITCODE)"
}

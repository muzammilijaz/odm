//! Repairs Chrome/Edge Native Messaging registration whenever ODM starts.
//!
//! Installer hooks normally create these registry entries, but they can be
//! absent after an interrupted install, an app move, or during development.
//! Registration is per-user and does not require elevation.

use serde_json::json;
use std::{fs, io, path::PathBuf};
use tauri::{path::BaseDirectory, AppHandle, Manager};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_WOW64_32KEY, KEY_WOW64_64KEY, KEY_WRITE},
    RegKey,
};

const HOST_NAME: &str = "com.odm.nativehost";
const OFFICIAL_EXTENSION_ORIGIN: &str = "chrome-extension://lfpiggopnkjdgedghgapjnmijgckebkd/";
const DEV_EXTENSION_ORIGIN: &str = "chrome-extension://igjebnkcfkjpleeahgjnpdkahplddfdc/";

pub fn register(app: &AppHandle) -> io::Result<PathBuf> {
    let host_exe = find_host_executable(app)?;
    let manifest_dir = super::state::app_data_dir().join("native-messaging");
    fs::create_dir_all(&manifest_dir)?;

    let manifest_path = manifest_dir.join(format!("{HOST_NAME}.json"));
    let manifest = json!({
        "name": HOST_NAME,
        "description": "ODM (Open Download Manager) native messaging host",
        "path": host_exe,
        "type": "stdio",
        "allowed_origins": [OFFICIAL_EXTENSION_ORIGIN, DEV_EXTENSION_ORIGIN]
    });
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(io::Error::other)?;
    fs::write(&manifest_path, bytes)?;

    let manifest_value = manifest_path.to_string_lossy().into_owned();
    let registry_path = format!(r"Software\Google\Chrome\NativeMessagingHosts\{HOST_NAME}");
    let edge_registry_path = format!(r"Software\Microsoft\Edge\NativeMessagingHosts\{HOST_NAME}");
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        for path in [&registry_path, &edge_registry_path] {
            let (key, _) = current_user.create_subkey_with_flags(path, KEY_WRITE | view)?;
            key.set_value("", &manifest_value)?;
        }
    }

    Ok(manifest_path)
}

fn find_host_executable(app: &AppHandle) -> io::Result<PathBuf> {
    let mut candidates = Vec::new();

    // Workspace debug/release builds place both executables together. Check
    // this first so a stale staged resource from an earlier bundle cannot
    // override the freshly built development host.
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            candidates.push(parent.join("odm-native-host.exe"));
        }
    }

    // Packaged builds preserve the source `resources/` folder under Tauri's
    // resource directory. Keep the flat candidate for compatibility with a
    // custom bundle mapping.
    if let Ok(path) = app
        .path()
        .resolve("resources/odm-native-host.exe", BaseDirectory::Resource)
    {
        candidates.push(path);
    }
    if let Ok(path) = app
        .path()
        .resolve("odm-native-host.exe", BaseDirectory::Resource)
    {
        candidates.push(path);
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "odm-native-host.exe was not found"))
}

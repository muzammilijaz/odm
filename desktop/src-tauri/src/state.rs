use odm_core::TaskManager;
use std::path::PathBuf;
use tauri::{path::BaseDirectory, AppHandle, Manager};

#[derive(Clone)]
pub struct AppState {
    pub manager: TaskManager,
}

pub fn default_downloads_root() -> PathBuf {
    if let Some(user_dirs) = directories::UserDirs::new() {
        if let Some(downloads) = user_dirs.download_dir() {
            return downloads.join("ODM");
        }
        return user_dirs.home_dir().join("ODM Downloads");
    }
    PathBuf::from("ODM Downloads")
}

pub fn app_data_dir() -> PathBuf {
    if let Some(proj) = directories::ProjectDirs::from("com", "odm", "ODM") {
        return proj.data_dir().to_path_buf();
    }
    PathBuf::from(".odm-data")
}

/// Points odm-engine's ffmpeg resolution at bundled binaries and gives yt-dlp
/// a user-writable managed path for self-updates. Binary names follow
/// Tauri's sidecar convention (`<name>-<target-triple>`).
///
/// In a packaged (installed) build, these live under the app's resource
/// directory (`resources/binaries/` in the app bundle -- see
/// `tauri.conf.json`'s `bundle.resources`, which ships `binaries/*` inside
/// the installer). In dev (`cargo tauri dev`/`cargo run`), that resource
/// directory doesn't exist yet, so fall back to reading straight out of
/// `src-tauri/binaries/` via `CARGO_MANIFEST_DIR`.
pub fn set_bundled_binary_env_vars(app: &AppHandle) {
    let binaries_dir = app
        .path()
        .resolve("binaries", BaseDirectory::Resource)
        .ok()
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries"));
    let triple = current_target_triple();
    let suffix = if cfg!(windows) { ".exe" } else { "" };

    let ffmpeg = binaries_dir.join(format!("ffmpeg-{triple}{suffix}"));
    if ffmpeg.exists() {
        std::env::set_var("ODM_FFMPEG_PATH", &ffmpeg);
    }

    let ffprobe = binaries_dir.join(format!("ffprobe-{triple}{suffix}"));
    if ffprobe.exists() {
        std::env::set_var("ODM_FFPROBE_PATH", &ffprobe);
    }

    // Never force yt-dlp to the install directory: Program Files and custom
    // protected folders are commonly read-only after installation. The
    // engine prefers this managed copy when present and creates it on the
    // first in-app update.
    let managed_ytdlp = app_data_dir().join("binaries").join(format!("yt-dlp{suffix}"));
    std::env::set_var("ODM_YTDLP_USER_PATH", &managed_ytdlp);
    let bundled_ytdlp = binaries_dir.join(format!("yt-dlp-{triple}{suffix}"));
    if bundled_ytdlp.is_file() {
        std::env::set_var("ODM_YTDLP_BUNDLED_PATH", &bundled_ytdlp);
    }

    // Gives yt-dlp a JS runtime for sites that need one to solve extraction
    // challenges -- yt-dlp's own `--help` lists quickjs as a supported
    // `--js-runtimes` engine.
    let quickjs = binaries_dir.join(format!("quickjs-{triple}{suffix}"));
    if quickjs.exists() {
        std::env::set_var("ODM_QUICKJS_PATH", &quickjs);
    }
}

fn current_target_triple() -> &'static str {
    if cfg!(all(target_arch = "x86_64", target_os = "windows")) {
        "x86_64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "aarch64", target_os = "windows")) {
        "aarch64-pc-windows-msvc"
    } else if cfg!(all(target_arch = "x86_64", target_os = "macos")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_arch = "aarch64", target_os = "macos")) {
        "aarch64-apple-darwin"
    } else if cfg!(target_os = "linux") {
        "x86_64-unknown-linux-gnu"
    } else {
        "unknown"
    }
}

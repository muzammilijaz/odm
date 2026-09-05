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

/// Points odm-engine's ffmpeg/yt-dlp resolution (which checks
/// `ODM_FFMPEG_PATH`/`ODM_YTDLP_PATH` first) at the bundled binaries, so
/// neither needs to be separately installed by the user. Binary names follow
/// Tauri's sidecar convention (`<name>-<target-triple>.exe`).
///
/// In a packaged (installed) build, these live under the app's resource
/// directory (`$INSTDIR\resources\binaries\` on Windows -- see
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
    const TRIPLE: &str = "x86_64-pc-windows-msvc";

    let ffmpeg = binaries_dir.join(format!("ffmpeg-{TRIPLE}.exe"));
    if ffmpeg.exists() {
        std::env::set_var("ODM_FFMPEG_PATH", &ffmpeg);
    }

    let ffprobe = binaries_dir.join(format!("ffprobe-{TRIPLE}.exe"));
    if ffprobe.exists() {
        std::env::set_var("ODM_FFPROBE_PATH", &ffprobe);
    }

    let ytdlp = binaries_dir.join(format!("yt-dlp-{TRIPLE}.exe"));
    if ytdlp.exists() {
        std::env::set_var("ODM_YTDLP_PATH", &ytdlp);
    }

    // Gives yt-dlp a JS runtime for sites that need one to solve extraction
    // challenges -- yt-dlp's own `--help` lists quickjs as a supported
    // `--js-runtimes` engine.
    let quickjs = binaries_dir.join(format!("quickjs-{TRIPLE}.exe"));
    if quickjs.exists() {
        std::env::set_var("ODM_QUICKJS_PATH", &quickjs);
    }
}

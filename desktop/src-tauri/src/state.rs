use odm_core::TaskManager;
use std::path::PathBuf;

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
/// `ODM_FFMPEG_PATH`/`ODM_YTDLP_PATH` first) at the bundled binaries in
/// `src-tauri/binaries/`, so neither needs to be separately installed by the
/// user. Binary names follow Tauri's sidecar convention
/// (`<name>-<target-triple>.exe`) so the same files double as the
/// `externalBin` sources for a packaged installer later.
///
/// `CARGO_MANIFEST_DIR` is baked in at compile time and points at
/// `desktop/src-tauri` on the machine that built this binary — correct for
/// local dev (`cargo tauri dev`/`cargo run`), but not portable to an
/// installed build on a different machine. A packaged build should instead
/// resolve these via Tauri's resource-path API; tracked as follow-up work.
pub fn set_bundled_binary_env_vars() {
    let binaries_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    const TRIPLE: &str = "x86_64-pc-windows-msvc";

    let ffmpeg = binaries_dir.join(format!("ffmpeg-{TRIPLE}.exe"));
    if ffmpeg.exists() {
        std::env::set_var("ODM_FFMPEG_PATH", &ffmpeg);
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

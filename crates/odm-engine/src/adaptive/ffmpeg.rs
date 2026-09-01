use crate::error::{EngineError, Result};
use crate::process_ext::no_window_command;
use std::path::{Path, PathBuf};

/// Resolves the ffmpeg binary to invoke: an `ODM_FFMPEG_PATH` override, a
/// bundled `ffmpeg`/`ffmpeg.exe` next to the running executable (how the Tauri
/// package ships it in Phase 3), or whatever `ffmpeg` is on `PATH`.
pub fn resolve_ffmpeg_path() -> PathBuf {
    if let Ok(p) = std::env::var("ODM_FFMPEG_PATH") {
        return PathBuf::from(p);
    }
    let exe_name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let bundled = dir.join(exe_name);
            if bundled.exists() {
                return bundled;
            }
        }
    }
    PathBuf::from(exe_name)
}

async fn run_ffmpeg(ffmpeg: &Path, args: &[&str]) -> Result<()> {
    let output = no_window_command(ffmpeg)
        .args(args)
        .output()
        .await
        .map_err(EngineError::Io)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(EngineError::Io(std::io::Error::other(format!(
            "ffmpeg exited with {}: {}",
            output.status,
            stderr.lines().rev().take(8).collect::<Vec<_>>().join(" | ")
        ))));
    }
    Ok(())
}

/// Concatenates a sequence of downloaded HLS `.ts`/fMP4 segments (already in
/// playback order) into one output file via ffmpeg's concat demuxer, doing a
/// stream copy (no re-encode).
pub async fn concat_segments(ffmpeg: &Path, segments: &[PathBuf], output: &Path) -> Result<()> {
    let list_path = output.with_extension("concat.txt");
    let mut list = String::new();
    for seg in segments {
        let escaped = seg.display().to_string().replace('\'', "'\\''");
        list.push_str(&format!("file '{escaped}'\n"));
    }
    tokio::fs::write(&list_path, list).await?;

    let list_str = list_path.display().to_string();
    let out_str = output.display().to_string();
    let result = run_ffmpeg(
        ffmpeg,
        &["-y", "-f", "concat", "-safe", "0", "-i", &list_str, "-c", "copy", &out_str],
    )
    .await;

    let _ = tokio::fs::remove_file(&list_path).await;
    result
}

/// Muxes a separately-downloaded video and audio stream (the common DASH case
/// of split adaptation sets) into one output container, stream-copied.
pub async fn mux_video_audio(ffmpeg: &Path, video: &Path, audio: &Path, output: &Path) -> Result<()> {
    let video_str = video.display().to_string();
    let audio_str = audio.display().to_string();
    let out_str = output.display().to_string();
    run_ffmpeg(
        ffmpeg,
        &[
            "-y", "-i", &video_str, "-i", &audio_str, "-c", "copy", &out_str,
        ],
    )
    .await
}

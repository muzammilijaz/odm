//! Native adaptive-streaming (HLS/DASH) support: download every media segment
//! through the shared HTTP client, then remux/mux with ffmpeg (stream copy).
//! Covers open standard streaming formats without any per-site reverse
//! engineering; sites requiring signed/private-API resolution (YouTube,
//! TikTok, ...) stay on the `yt-dlp` path instead.

mod dash;
pub mod ffmpeg;
mod hls;
mod segments;

use crate::error::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Hls,
    Dash,
}

/// Sniffs the stream kind from the URL's extension (`.m3u8` / `.mpd`); when
/// neither matches, `Content-Type` is checked as a fallback.
pub async fn detect_stream_kind(client: &reqwest::Client, url: &str) -> Result<Option<StreamKind>> {
    let lower = url.to_lowercase();
    if lower.contains(".m3u8") {
        return Ok(Some(StreamKind::Hls));
    }
    if lower.contains(".mpd") {
        return Ok(Some(StreamKind::Dash));
    }

    let resp = client.head(url).send().await;
    if let Ok(resp) = resp {
        if let Some(ct) = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
        {
            if ct.contains("mpegurl") {
                return Ok(Some(StreamKind::Hls));
            }
            if ct.contains("dash+xml") {
                return Ok(Some(StreamKind::Dash));
            }
        }
    }

    Ok(None)
}

/// Downloads an HLS or DASH stream to `dest`, given an already-built client
/// (so proxy/cookie/header config from `DownloadConfig` carries through).
pub async fn download_adaptive(
    client: &reqwest::Client,
    url: &str,
    dest: impl AsRef<Path>,
    kind: StreamKind,
) -> Result<PathBuf> {
    let dest = dest.as_ref();
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }
    match kind {
        StreamKind::Hls => hls::download_hls(client, url, dest).await,
        StreamKind::Dash => dash::download_dash(client, url, dest).await,
    }
}

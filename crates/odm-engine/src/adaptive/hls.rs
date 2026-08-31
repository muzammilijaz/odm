use super::ffmpeg;
use super::segments::{fetch_segments, resolve_url, segment_dir, SegmentSpec};
use crate::error::{EngineError, Result};
use m3u8_rs::Playlist;
use std::path::{Path, PathBuf};

/// Downloads an HLS stream (`.m3u8`, master or media playlist) to `dest`.
/// Picks the highest-bandwidth variant when given a master playlist, fetches
/// every media segment through the shared HTTP client, then remuxes with
/// ffmpeg (stream copy, no re-encode).
pub async fn download_hls(client: &reqwest::Client, playlist_url: &str, dest: &Path) -> Result<PathBuf> {
    let media_url = resolve_media_playlist_url(client, playlist_url).await?;
    let (bytes, base_url) = fetch_playlist(client, &media_url).await?;

    let playlist = m3u8_rs::parse_playlist_res(&bytes)
        .map_err(|e| EngineError::Io(std::io::Error::other(format!("invalid HLS media playlist: {e:?}"))))?;

    let media = match playlist {
        Playlist::MediaPlaylist(m) => m,
        Playlist::MasterPlaylist(_) => {
            return Err(EngineError::Io(std::io::Error::other(
                "expected a media playlist after variant selection",
            )))
        }
    };

    let seg_dir = segment_dir(dest);
    tokio::fs::create_dir_all(&seg_dir).await?;

    let specs: Vec<SegmentSpec> = media
        .segments
        .iter()
        .enumerate()
        .map(|(i, seg)| SegmentSpec {
            url: resolve_url(&base_url, &seg.uri),
            byte_range: None, // v1: byte-range media segments not yet supported
            dest: seg_dir.join(format!("{i:06}.ts")),
        })
        .collect();

    if specs.is_empty() {
        return Err(EngineError::Io(std::io::Error::other("HLS playlist has no segments")));
    }

    let segment_paths = fetch_segments(client, specs).await?;

    let ffmpeg_bin = ffmpeg::resolve_ffmpeg_path();
    ffmpeg::concat_segments(&ffmpeg_bin, &segment_paths, dest).await?;

    let _ = tokio::fs::remove_dir_all(&seg_dir).await;
    Ok(dest.to_path_buf())
}

/// If `playlist_url` is a master playlist, resolves it to the highest-bandwidth
/// variant's media playlist URL; if it's already a media playlist, returns it
/// unchanged.
async fn resolve_media_playlist_url(client: &reqwest::Client, playlist_url: &str) -> Result<String> {
    let (bytes, base_url) = fetch_playlist(client, playlist_url).await?;
    let playlist = m3u8_rs::parse_playlist_res(&bytes)
        .map_err(|e| EngineError::Io(std::io::Error::other(format!("invalid HLS playlist: {e:?}"))))?;

    match playlist {
        Playlist::MediaPlaylist(_) => Ok(playlist_url.to_string()),
        Playlist::MasterPlaylist(master) => {
            let best = master
                .variants
                .iter()
                .max_by_key(|v| v.bandwidth)
                .ok_or_else(|| EngineError::Io(std::io::Error::other("master playlist has no variants")))?;
            Ok(resolve_url(&base_url, &best.uri))
        }
    }
}

async fn fetch_playlist(client: &reqwest::Client, url: &str) -> Result<(bytes::Bytes, String)> {
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(EngineError::BadStatus(resp.status()));
    }
    let final_url = resp.url().to_string();
    let bytes = resp.bytes().await?;
    Ok((bytes, final_url))
}

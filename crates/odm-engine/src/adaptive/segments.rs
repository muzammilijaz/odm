use crate::error::{EngineError, Result};
use crate::retry::backoff_delay;
use futures_util::stream::{self, StreamExt};
use reqwest::header::RANGE;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;

/// One media segment to fetch: a URL plus an optional byte range (used by
/// DASH's indexed/byte-range addressing) and the local file it should land in.
#[derive(Debug, Clone)]
pub struct SegmentSpec {
    pub url: String,
    pub byte_range: Option<(u64, u64)>,
    pub dest: PathBuf,
}

const MAX_RETRIES: u32 = 4;
const MAX_CONCURRENT_SEGMENTS: usize = 6;

/// Downloads every segment (bounded concurrency, each with its own small retry
/// loop) into its target path, returning the destination paths in the same
/// order they were given — i.e. playback order.
pub async fn fetch_segments(
    client: &reqwest::Client,
    specs: Vec<SegmentSpec>,
) -> Result<Vec<PathBuf>> {
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_SEGMENTS));
    let dests: Vec<PathBuf> = specs.iter().map(|s| s.dest.clone()).collect();

    let results: Vec<Result<()>> = stream::iter(specs.into_iter().map(|spec| {
        let client = client.clone();
        let semaphore = semaphore.clone();
        async move {
            let _permit = semaphore.acquire_owned().await.ok();
            fetch_one_with_retry(&client, &spec).await
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_SEGMENTS)
    .collect()
    .await;

    for r in results {
        r?;
    }
    Ok(dests)
}

async fn fetch_one_with_retry(client: &reqwest::Client, spec: &SegmentSpec) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..=MAX_RETRIES {
        match fetch_one(client, spec).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(backoff_delay(attempt)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or(EngineError::Cancelled))
}

async fn fetch_one(client: &reqwest::Client, spec: &SegmentSpec) -> Result<()> {
    if let Some(parent) = spec.dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut req = client.get(&spec.url);
    if let Some((start, end)) = spec.byte_range {
        req = req.header(RANGE, format!("bytes={start}-{end}"));
    }
    let resp = req.send().await?;
    if !resp.status().is_success() {
        return Err(EngineError::BadStatus(resp.status()));
    }
    let bytes = resp.bytes().await?;
    tokio::fs::write(&spec.dest, &bytes).await?;
    Ok(())
}

pub fn resolve_url(base: &str, maybe_relative: &str) -> String {
    match url::Url::parse(maybe_relative) {
        Ok(u) => u.to_string(),
        Err(_) => match url::Url::parse(base).and_then(|b| b.join(maybe_relative)) {
            Ok(u) => u.to_string(),
            Err(_) => maybe_relative.to_string(),
        },
    }
}

pub fn segment_dir(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".segments");
    p.into()
}

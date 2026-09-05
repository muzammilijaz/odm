use crate::chunk::{plan_chunks, Chunk};
use crate::config::{DownloadConfig, FileExistPolicy};
use crate::control::{DownloadControl, PauseToken};
use crate::error::{EngineError, Result};
use crate::posio::write_all_at;
use crate::progress::{Progress, SpeedTracker};
use crate::retry::{backoff_delay, is_retryable};
use crate::state::{part_path, state_path, DownloadState};
use crate::throttle::Throttle;

use futures_util::StreamExt;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, CONTENT_LENGTH, CONTENT_RANGE, RANGE, USER_AGENT,
};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub struct DownloadHandle {
    pub control: DownloadControl,
    pub progress: watch::Receiver<Progress>,
    throttle: Throttle,
    join: JoinHandle<Result<PathBuf>>,
}

impl DownloadHandle {
    pub fn pause(&self) {
        self.control.pause.pause();
    }

    pub fn resume(&self) {
        self.control.pause.resume();
    }

    pub fn cancel(&self) {
        self.control.cancel.cancel();
    }

    /// Retunes the bandwidth cap live (0 = unlimited); takes effect on the
    /// next throttle check for each active chunk.
    pub fn set_speed_limit(&self, max_bytes_per_sec: u64) {
        self.throttle.set_limit(max_bytes_per_sec);
    }

    /// Waits for the download to finish, returning the final file path.
    pub async fn wait(self) -> Result<PathBuf> {
        match self.join.await {
            Ok(result) => result,
            Err(_join_err) => Err(EngineError::Cancelled),
        }
    }
}

fn build_client(config: &DownloadConfig) -> Result<reqwest::Client> {
    // Deliberately no auto-decompression feature: this engine's chunk math
    // (Range offsets, Content-Length-derived total size, positioned writes)
    // all operate on the wire byte count. Transparent gzip decompression
    // breaks that — a byte range is sliced from the *compressed* stream and
    // generally isn't independently decompressible, and the decompressed
    // byte count no longer matches Content-Length, corrupting multi-chunk
    // downloads and progress reporting alike. Confirmed live: a gzip'd
    // response reported downloaded_bytes > total_bytes before this was
    // disabled (reqwest's "gzip" cargo feature is not enabled here).
    let mut builder = reqwest::Client::builder()
        .connect_timeout(config.connect_timeout)
        .cookie_store(true);
    builder = config.proxy.apply(builder)?;
    Ok(builder.build()?)
}

fn build_headers(config: &DownloadConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_str(&config.user_agent).unwrap_or(HeaderValue::from_static("ODM/0.1")),
    );
    for (k, v) in &config.extra_headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            headers.insert(name, value);
        }
    }
    Ok(headers)
}

struct ProbeResult {
    total_size: Option<u64>,
    supports_range: bool,
}

async fn probe(client: &reqwest::Client, url: &str, headers: &HeaderMap) -> Result<ProbeResult> {
    let resp = client
        .get(url)
        .headers(headers.clone())
        .header(RANGE, "bytes=0-0")
        .send()
        .await?;

    let status = resp.status();
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.rsplit('/').next())
            .and_then(|s| s.parse::<u64>().ok());
        return Ok(ProbeResult {
            total_size: total,
            supports_range: total.is_some(),
        });
    }

    if status.is_success() {
        let total = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        return Ok(ProbeResult {
            total_size: total,
            supports_range: false,
        });
    }

    Err(EngineError::BadStatus(status))
}

/// Downloads `url` into `dest`, splitting into concurrent ranged chunks per
/// `config`. Returns a handle for pause/resume/cancel + a live progress feed;
/// the download itself runs on a spawned task.
pub async fn download(
    url: impl Into<String>,
    dest: impl AsRef<Path>,
    config: DownloadConfig,
) -> Result<DownloadHandle> {
    let url = url.into();
    let dest = dest.as_ref().to_path_buf();

    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent).await?;
        }
    }

    match config.file_exist_policy {
        FileExistPolicy::Skip if tokio::fs::try_exists(&dest).await.unwrap_or(false) => {
            let (control, _pause_token) = DownloadControl::new();
            let (_tx, rx) = watch::channel(Progress::default());
            let join = tokio::spawn(async move { Ok(dest) });
            return Ok(DownloadHandle {
                control,
                progress: rx,
                throttle: Throttle::new(0),
                join,
            });
        }
        FileExistPolicy::Error if tokio::fs::try_exists(&dest).await.unwrap_or(false) => {
            return Err(EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                dest.display().to_string(),
            )));
        }
        _ => {}
    }

    let client = build_client(&config)?;
    let headers = build_headers(&config)?;
    let probe_result = probe(&client, &url, &headers).await?;

    let part = part_path(&dest);
    let state_file = state_path(&dest);

    let existing_state = DownloadState::load(&state_file).await?;
    let state = match existing_state {
        Some(s)
            if s.url == url
                && s.total_size == probe_result.total_size.unwrap_or(0)
                && !s.is_complete() =>
        {
            s
        }
        _ => {
            let chunks = match probe_result.total_size {
                Some(size) => plan_chunks(
                    size,
                    config.chunk_count,
                    config.min_chunk_size,
                    probe_result.supports_range,
                ),
                None => vec![Chunk {
                    index: 0,
                    start: 0,
                    end: 0,
                    position: 0,
                    unbounded: true,
                }],
            };
            DownloadState {
                url: url.clone(),
                total_size: probe_result.total_size.unwrap_or(0),
                supports_range: probe_result.supports_range,
                chunks,
            }
        }
    };

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(&part)?;
    if let Some(size) = probe_result.total_size {
        if probe_result.supports_range {
            file.set_len(size)?;
        }
    }
    let file = Arc::new(file);

    let positions: Vec<Arc<AtomicU64>> = state
        .chunks
        .iter()
        .map(|c| Arc::new(AtomicU64::new(c.position)))
        .collect();

    let (control, pause_token) = DownloadControl::new();
    let (progress_tx, progress_rx) = watch::channel(Progress::default());
    let throttle = Throttle::new(config.max_bytes_per_sec);

    let join = tokio::spawn(run_supervisor(
        client,
        url,
        headers,
        state,
        positions,
        file,
        part,
        dest,
        state_file,
        config,
        control.clone(),
        pause_token,
        throttle.clone(),
        progress_tx,
    ));

    Ok(DownloadHandle {
        control,
        progress: progress_rx,
        throttle,
        join,
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_supervisor(
    client: reqwest::Client,
    url: String,
    headers: HeaderMap,
    mut state: DownloadState,
    positions: Vec<Arc<AtomicU64>>,
    file: Arc<std::fs::File>,
    part: PathBuf,
    dest: PathBuf,
    state_file: PathBuf,
    config: DownloadConfig,
    control: DownloadControl,
    pause_token: PauseToken,
    throttle: Throttle,
    progress_tx: watch::Sender<Progress>,
) -> Result<PathBuf> {
    let bytes_before_attempt: u64 = positions.iter().map(|p| p.load(Ordering::Relaxed)).sum();

    // Progress/state-save ticker running alongside the chunk workers.
    let progress_positions = positions.clone();
    let progress_url = url.clone();
    let progress_total = state.total_size;
    let progress_supports_range = state.supports_range;
    let progress_state_file = state_file.clone();
    let progress_chunks = state.chunks.clone();
    let progress_tx_clone = progress_tx.clone();
    let ticker_cancel = control.cancel.clone();
    let ticker = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        let mut speed = SpeedTracker::new();
        loop {
            tokio::select! {
                _ = ticker_cancel.cancelled() => break,
                _ = interval.tick() => {
                    let downloaded: u64 = progress_positions.iter().map(|p| p.load(Ordering::Relaxed)).sum();
                    let bps = speed.sample(downloaded);
                    let _ = progress_tx_clone.send(Progress {
                        downloaded_bytes: downloaded,
                        total_bytes: if progress_total > 0 { Some(progress_total) } else { None },
                        bytes_per_sec: bps,
                        active_chunks: progress_chunks.len(),
                    });
                    let snapshot = DownloadState {
                        url: progress_url.clone(),
                        total_size: progress_total,
                        supports_range: progress_supports_range,
                        chunks: progress_chunks
                            .iter()
                            .zip(progress_positions.iter())
                            .map(|(c, p)| Chunk { position: p.load(Ordering::Relaxed), ..c.clone() })
                            .collect(),
                    };
                    let _ = snapshot.save(&progress_state_file).await;
                }
            }
        }
    });

    let results = run_all_chunks(
        &client,
        &url,
        &headers,
        &state.chunks,
        &positions,
        &file,
        &throttle,
        &pause_token,
        &control.cancel,
        &config,
    )
    .await;

    let any_failed = results.iter().any(|r| r.is_err());
    let bytes_after_attempt: u64 = positions.iter().map(|p| p.load(Ordering::Relaxed)).sum();

    if any_failed
        && state.chunks.len() > 1
        && bytes_after_attempt == bytes_before_attempt
        && !control.cancel.is_cancelled()
    {
        // Single-connection fallback: every chunk failed transiently before any
        // bytes arrived (e.g. a proxy/middlebox breaking concurrent connections).
        // Retry once as one sequential stream covering the whole file.
        let fallback_chunk = Chunk {
            index: 0,
            start: 0,
            end: state.total_size.saturating_sub(1),
            position: 0,
            unbounded: state.total_size == 0,
        };
        let fallback_positions = vec![Arc::new(AtomicU64::new(0))];
        let fallback_results = run_all_chunks(
            &client,
            &url,
            &headers,
            &[fallback_chunk.clone()],
            &fallback_positions,
            &file,
            &throttle,
            &pause_token,
            &control.cancel,
            &config,
        )
        .await;
        if fallback_results.iter().all(|r| r.is_ok()) {
            state.chunks = vec![fallback_chunk];
            ticker.abort();
            let downloaded = fallback_positions[0].load(Ordering::Relaxed);
            send_final_progress(&progress_tx, downloaded, state.total_size);
            return finalize(&file, &part, &dest, &state_file, &state, downloaded).await;
        } else if let Some(Err(e)) = fallback_results.into_iter().next() {
            ticker.abort();
            return Err(e);
        }
    }

    ticker.abort();

    if control.cancel.is_cancelled() {
        return Err(EngineError::Cancelled);
    }

    for r in results {
        r?;
    }

    let total_downloaded: u64 = positions.iter().map(|p| p.load(Ordering::Relaxed)).sum();
    // The periodic ticker may never have fired for a download that finished
    // faster than its interval — send one last update so observers (the task
    // queue's DB row, in particular) see the true final byte count rather
    // than whatever stale value they last saw.
    send_final_progress(&progress_tx, total_downloaded, state.total_size);
    finalize(&file, &part, &dest, &state_file, &state, total_downloaded).await
}

fn send_final_progress(progress_tx: &watch::Sender<Progress>, downloaded: u64, total: u64) {
    let _ = progress_tx.send(Progress {
        downloaded_bytes: downloaded,
        total_bytes: if total > 0 { Some(total) } else { None },
        bytes_per_sec: 0.0,
        active_chunks: 0,
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_all_chunks(
    client: &reqwest::Client,
    url: &str,
    headers: &HeaderMap,
    chunks: &[Chunk],
    positions: &[Arc<AtomicU64>],
    file: &Arc<std::fs::File>,
    throttle: &Throttle,
    pause_token: &PauseToken,
    cancel: &CancellationToken,
    config: &DownloadConfig,
) -> Vec<Result<()>> {
    let semaphore = Arc::new(Semaphore::new(config.parallel_count.max(1)));
    let mut handles = Vec::with_capacity(chunks.len());
    for (chunk, position) in chunks.iter().zip(positions.iter()) {
        let sem = semaphore.clone();
        let client = client.clone();
        let url = url.to_string();
        let headers = headers.clone();
        let chunk = chunk.clone();
        let position = position.clone();
        let file = file.clone();
        let throttle = throttle.clone();
        let mut pause_token = pause_token.clone();
        let cancel = cancel.clone();
        let max_retries = config.max_retries;
        let read_timeout = config.read_timeout;
        handles.push(tokio::spawn(async move {
            let _permit = sem.acquire_owned().await.ok();
            let guard = throttle.register_chunk();
            let result = run_chunk(
                client,
                url,
                headers,
                chunk,
                position,
                file,
                throttle.clone(),
                &mut pause_token,
                cancel,
                max_retries,
                read_timeout,
            )
            .await;
            drop(guard);
            result
        }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for h in handles {
        results.push(match h.await {
            Ok(r) => r,
            Err(_) => Err(EngineError::Cancelled),
        });
    }
    results
}

#[allow(clippy::too_many_arguments)]
async fn run_chunk(
    client: reqwest::Client,
    url: String,
    headers: HeaderMap,
    chunk: Chunk,
    position: Arc<AtomicU64>,
    file: Arc<std::fs::File>,
    throttle: Throttle,
    pause_token: &mut PauseToken,
    cancel: CancellationToken,
    max_retries: u32,
    read_timeout: Duration,
) -> Result<()> {
    let mut last_err: Option<EngineError> = None;

    for attempt in 0..=max_retries {
        pause_token.wait_while_paused().await;
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }

        let start = chunk.start + position.load(Ordering::Relaxed);
        if !chunk.unbounded && start > chunk.end {
            return Ok(());
        }

        let mut req = client.get(&url).headers(headers.clone());
        if chunk.unbounded {
            if start > 0 {
                req = req.header(RANGE, format!("bytes={}-", start));
            }
        } else {
            req = req.header(RANGE, format!("bytes={}-{}", start, chunk.end));
        }

        let attempt_result = run_chunk_attempt(
            req,
            &chunk,
            &position,
            &file,
            &throttle,
            pause_token,
            &cancel,
            read_timeout,
        )
        .await;

        match attempt_result {
            Ok(()) => return Ok(()),
            Err(e) => {
                let retryable = match &e {
                    EngineError::Cancelled => false,
                    EngineError::Http(http_err) => is_retryable(http_err),
                    // Io (timeout/short-stream) and BadStatus are treated as
                    // transient by default — the retry budget bounds the cost.
                    _ => true,
                };
                last_err = Some(e);
                if !retryable || attempt == max_retries {
                    break;
                }
                tokio::time::sleep(backoff_delay(attempt)).await;
            }
        }
    }

    Err(EngineError::ChunkFailed {
        index: chunk.index,
        attempts: max_retries + 1,
        source: Box::new(last_err.unwrap_or(EngineError::Cancelled)),
    })
}

async fn run_chunk_attempt(
    req: reqwest::RequestBuilder,
    chunk: &Chunk,
    position: &Arc<AtomicU64>,
    file: &Arc<std::fs::File>,
    throttle: &Throttle,
    pause_token: &mut PauseToken,
    cancel: &CancellationToken,
    read_timeout: Duration,
) -> Result<()> {
    let resp = tokio::select! {
        _ = cancel.cancelled() => return Err(EngineError::Cancelled),
        r = req.send() => r.map_err(EngineError::from)?,
    };

    let status = resp.status();
    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(EngineError::BadStatus(status));
    }

    let mut stream = resp.bytes_stream();
    loop {
        pause_token.wait_while_paused().await;
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }

        let next = tokio::select! {
            _ = cancel.cancelled() => return Err(EngineError::Cancelled),
            n = tokio::time::timeout(read_timeout, stream.next()) => n,
        };

        match next {
            Ok(Some(Ok(bytes))) => {
                let offset = chunk.start + position.load(Ordering::Relaxed);
                let file = file.clone();
                let buf = bytes.to_vec();
                tokio::task::spawn_blocking(move || write_all_at(&file, &buf, offset))
                    .await
                    .map_err(|_| EngineError::Cancelled)??;
                position.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                throttle.throttle(bytes.len() as u64).await;
            }
            Ok(Some(Err(e))) => return Err(e.into()),
            Ok(None) => break,
            Err(_elapsed) => {
                return Err(EngineError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "chunk read timed out",
                )))
            }
        }
    }

    if !chunk.unbounded {
        let final_pos = position.load(Ordering::Relaxed);
        if chunk.start + final_pos <= chunk.end {
            // Server closed the connection before delivering the full range —
            // an early close that doesn't throw looks like success otherwise.
            return Err(EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "chunk closed before completion",
            )));
        }
    }

    Ok(())
}

async fn finalize(
    file: &Arc<std::fs::File>,
    part: &Path,
    dest: &Path,
    state_file: &Path,
    state: &DownloadState,
    _total_downloaded: u64,
) -> Result<PathBuf> {
    if state.total_size > 0 {
        file.set_len(state.total_size)?;
    }
    tokio::fs::rename(part, dest).await?;
    DownloadState::remove(state_file).await?;
    Ok(dest.to_path_buf())
}

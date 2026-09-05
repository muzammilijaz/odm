use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::model::{Task, TaskStatus};
use odm_engine::{DownloadConfig, DownloadControl, EngineError, Progress, StreamKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{watch, Mutex, Semaphore};

/// Relays a live `Progress` feed into the DB as it changes, for the
/// duration of `body`. Shared between the progressive engine and yt-dlp
/// (both expose the same `watch::Receiver<Progress>` shape) so the "poll
/// while running, then force one final read" pattern isn't duplicated.
async fn with_progress_relay<F, Fut, T>(
    db: &Db,
    id: i64,
    progress_rx: watch::Receiver<Progress>,
    body: F,
) -> T
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    // Interleaves periodic DB writes into *this same task* via `select!`
    // rather than spawning a separate watcher task -- simpler than juggling
    // an extra `tokio::spawn` + `JoinHandle::abort()` for the same effect.
    let body_fut = body();
    tokio::pin!(body_fut);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let result = loop {
        tokio::select! {
            result = &mut body_fut => break result,
            _ = interval.tick() => {
                let p = *progress_rx.borrow();
                let _ = db.update_progress(id, p.downloaded_bytes, p.total_bytes).await;
            }
        }
    };

    // One last write to guarantee the DB ends up with the true final byte
    // count even if `body` finished between two ticks.
    let final_progress = *progress_rx.borrow();
    let _ = db
        .update_progress(
            id,
            final_progress.downloaded_bytes,
            final_progress.total_bytes,
        )
        .await;

    result
}

enum Handle {
    Progressive(DownloadControl),
    /// yt-dlp has no live pause/resume of its own -- "pause" kills the
    /// process (yt-dlp writes a resumable `.part` file as it goes), and
    /// "resume" is a fresh `download_with_ytdlp` call for the same task,
    /// which picks that file back up by default.
    Ytdlp(tokio::task::AbortHandle),
    /// HLS/DASH (`odm_engine::download_adaptive`) runs to completion in one
    /// shot -- there's no live pause/resume/cancel handle for it yet, so
    /// this variant just tracks that one is running.
    RunToCompletion,
}

/// Bounds concurrent downloads, resolves categories/destinations for new
/// URLs, and keeps the SQLite queue in sync with each download's live state —
/// the "task manager" from the plan's Phase 2, sitting on top of the Phase 1
/// engine and the Phase 1b adaptive-stream engine.
#[derive(Clone)]
pub struct TaskManager {
    db: Db,
    config: DownloadConfig,
    downloads_root: PathBuf,
    semaphore: Arc<Semaphore>,
    client: reqwest::Client,
    handles: Arc<Mutex<HashMap<i64, Handle>>>,
    browser_fallbacks: Arc<Mutex<HashMap<i64, (String, Option<String>)>>>,
}

impl TaskManager {
    pub fn new(
        db: Db,
        config: DownloadConfig,
        downloads_root: impl Into<PathBuf>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            db,
            config,
            downloads_root: downloads_root.into(),
            semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))),
            client: reqwest::Client::new(),
            handles: Arc::new(Mutex::new(HashMap::new())),
            browser_fallbacks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Adds a URL to the queue: resolves its category by extension (routing
    /// it to that category's default subfolder), derives a destination
    /// filename when the caller doesn't supply one, and immediately starts it
    /// (subject to the concurrency limit).
    pub async fn add_download(
        &self,
        url: &str,
        filename_hint: Option<&str>,
        allow_playlist: bool,
        quality: Option<&str>,
    ) -> Result<Task> {
        self.add_download_with_fallback(url, filename_hint, allow_playlist, quality, None, None).await
    }

    pub async fn add_download_with_fallback(&self, url: &str, filename_hint: Option<&str>, allow_playlist: bool, quality: Option<&str>, fallback_url: Option<&str>, fallback_audio: Option<&str>) -> Result<Task> {
        // Treat an explicit YouTube playlist URL as a playlist even when an
        // older UI/native bridge omitted the checkbox flag. This prevents a
        // `watch?v=...&list=...` link from silently downloading only its first
        // video.
        let allow_playlist = allow_playlist || is_playlist_url(url);
        if allow_playlist && is_playlist_url(url) {
            return self.add_playlist(url, filename_hint, quality).await;
        }
        // Known video sites (YouTube, TikTok, ...) go through yt-dlp, whose
        // real output filename (video title + chosen container) isn't known
        // until the download finishes -- so there's no meaningful filename
        // to derive from the URL itself (e.g. youtube.com/watch?v=... has no
        // usable path segment). Store the destination *directory*; the
        // actual path is corrected via `Db::set_dest_path` once yt-dlp
        // reports it (see `run_ytdlp`).
        if odm_engine::is_known_video_site(url) {
            let video_quality = self.resolve_video_quality(quality).await;
            let video_category = self
                .db
                .list_categories()
                .await?
                .into_iter()
                .find(|c| c.name.eq_ignore_ascii_case("video"));
            let dest_dir = match &video_category {
                Some(c) => self.downloads_root.join(&c.default_folder),
                None => self.downloads_root.join("Video"),
            };
            let task = self
                .db
                .enqueue(
                    url,
                    &dest_dir.to_string_lossy(),
                    video_category.as_ref().map(|c| c.name.as_str()),
                    allow_playlist,
                    video_quality,
                )
                .await?;

            if let Some(media) = fallback_url {
                if valid_browser_media_url(media) {
                    self.browser_fallbacks.lock().await.insert(task.id, (media.to_string(), fallback_audio.filter(|audio| valid_browser_media_url(audio)).map(str::to_string)));
                }
            }
            if task.status == TaskStatus::Queued {
                self.start_task(task.id);
            }

            // Fetches the real title/thumbnail in the background (bounded so
            // a slow or hung probe can't run forever) so the UI can show
            // them for the whole download instead of only once the file
            // lands and its title is derived from the final filename. Runs
            // concurrently with the download itself -- rather than before
            // it -- so this is purely a cosmetic delay on the title/preview
            // showing up a moment later, not a delay on the download
            // starting. Any failure or timeout here is silently ignored,
            // falling back to the generic destination-folder name as before.
            if task.title.is_none()
                || task.thumbnail_url.is_none()
                || task.actual_video_quality.is_none()
            {
                let db = self.db.clone();
                let url = url.to_string();
                let id = task.id;
                let preferred_height = video_quality;
                tokio::spawn(async move {
                    if let Ok(Ok(info)) = tokio::time::timeout(
                        std::time::Duration::from_secs(15),
                        odm_engine::probe_video_qualities(&url),
                    )
                    .await
                    {
                        let selected_height =
                            odm_engine::select_available_height(preferred_height, &info.heights);
                        let _ = db
                            .set_metadata(id, &info.title, info.thumbnail.as_deref())
                            .await;
                        let _ = db.set_provisional_video_quality(id, selected_height).await;
                    }
                });
            }

            return Ok(task);
        }

        let filename = filename_hint
            .map(|s| s.to_string())
            .or_else(|| filename_from_url(url))
            .unwrap_or_else(|| "download".to_string());

        let category = self.db.resolve_category(&filename).await?;
        let dest_dir = match &category {
            Some(c) => self.downloads_root.join(&c.default_folder),
            None => self.downloads_root.join("General"),
        };
        let (dest_path, _) = self
            .unique_destination_path(dest_dir.join(&filename))
            .await?;

        let task = self
            .db
            .enqueue(
                url,
                &dest_path.to_string_lossy(),
                category.as_ref().map(|c| c.name.as_str()),
                false,
                None,
            )
            .await?;

        if task.status == TaskStatus::Queued {
            self.start_task(task.id);
        }
        Ok(task)
    }

    /// Spawns the background task that actually runs one download and keeps
    /// the DB row in sync until it reaches a terminal state.
    fn start_task(&self, id: i64) {
        let this = self.clone();
        tokio::spawn(async move {
            let _ = this.run_task(id).await;
        });
    }

    async fn add_playlist(&self, url: &str, folder: Option<&str>, quality: Option<&str>) -> Result<Task> {
        let file = self.db.get_setting(crate::settings::COOKIES_FILE).await?.filter(|v| !v.is_empty());
        let browser = self.db.get_setting(crate::settings::COOKIES_BROWSER).await?.filter(|v| !v.is_empty());
        // Normalize watch+list links to the playlist endpoint.
        let parsed = url::Url::parse(url).map_err(|e| std::io::Error::other(e.to_string()))?;
        let list = parsed.query_pairs().find(|(k, _)| k == "list").map(|(_, v)| v.into_owned()).unwrap_or_default();
        let mut endpoint = url::Url::parse("https://www.youtube.com/playlist").unwrap();
        endpoint.query_pairs_mut().append_pair("list", &list);
        let info = odm_engine::ytdlp::fetch_playlist(endpoint.as_str(), file.as_deref(), browser.as_deref()).await?;
        self.enqueue_playlist_info(&info, folder, quality).await
    }

    async fn enqueue_playlist_info(&self, info: &serde_json::Value, folder: Option<&str>, quality: Option<&str>) -> Result<Task> {
        let entries = info["entries"].as_array().filter(|v| !v.is_empty())
            .ok_or_else(|| std::io::Error::other("Playlist contains no visible videos. Check the link and sign-in settings."))?;
        let title = folder.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| info["title"].as_str().unwrap_or("YouTube Playlist"));
        let safe: String = title.chars().take(100).map(|c| if c.is_control() || "<>:\"/\\|?*%".contains(c) { '_' } else { c }).collect();
        let safe = safe.trim_matches([' ', '.']);
        let group = format!("playlist-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        let dest = self.downloads_root.join("Video").join(format!("{} [{}]", if safe.is_empty() { "Playlist" } else { safe }, &group));
        tokio::fs::create_dir_all(&dest).await?;
        let height = self.resolve_video_quality(quality).await;
        let mut tasks = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let id = entry["id"].as_str().unwrap_or("");
            let mut video_url = url::Url::parse("https://www.youtube.com/watch").unwrap();
            video_url.query_pairs_mut().append_pair("v", id);
            let task = self.db.enqueue(video_url.as_str(), &dest.to_string_lossy(), Some("Video"), false, height).await?;
            self.db.set_playlist_group(task.id, &group, title).await?;
            self.db.set_metadata(task.id, &format!("{}. {}", index + 1, entry["title"].as_str().unwrap_or("Unavailable video")), entry["thumbnails"].as_array().and_then(|v| v.last()).and_then(|v| v["url"].as_str())).await?;
            if id.is_empty() || matches!(entry["availability"].as_str(), Some("private" | "premium_only" | "subscriber_only")) {
                self.db.set_error(task.id, "This playlist video is unavailable or requires access.").await?;
            }
            tasks.push(task.id);
        }
        let first = self.db.get_task(tasks[0]).await?.ok_or(CoreError::TaskNotFound(tasks[0]))?;
        let concurrency = self.db.get_setting("playlist_concurrent").await?.and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1,4);
        self.schedule_playlist(tasks, concurrency);
        Ok(first)
    }

    pub async fn recover_playlist_queue(&self) -> Result<()> {
        let mut groups: HashMap<String, Vec<i64>> = HashMap::new();
        for task in self.db.list_tasks().await? {
            if let Some(group) = task.playlist_group {
                if matches!(task.status, TaskStatus::Queued | TaskStatus::Downloading) {
                    self.db.set_status(task.id, TaskStatus::Queued).await?;
                    groups.entry(group).or_default().push(task.id);
                }
            }
        }
        let concurrency = self.db.get_setting("playlist_concurrent").await?.and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1,4);
        for (_, mut ids) in groups { ids.sort_unstable(); self.schedule_playlist(ids, concurrency); }
        Ok(())
    }

    fn schedule_playlist(&self, tasks: Vec<i64>, concurrency: usize) {
        // Enqueue every entry before starting the first; the persisted rows
        // are the source of truth for group totals and per-video progress.
        let manager = self.clone();
        tokio::spawn(async move {
            let mut running = tokio::task::JoinSet::new();
            for id in tasks {
                if running.len() >= concurrency { let _ = running.join_next().await; }
                let manager = manager.clone();
                running.spawn(async move { let _ = manager.run_task(id).await; });
            }
            while running.join_next().await.is_some() {}
        });
    }

    async fn run_task(&self, id: i64) -> Result<()> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore never closed");

        let Some(task) = self.db.get_task(id).await? else {
            return Ok(());
        };
        if task.status != TaskStatus::Queued { return Ok(()); }
        if odm_engine::is_known_video_site(&task.url) {
            // A resumed/redownloaded task must not show a stale result from
            // its previous attempt while the new output is still pending.
            self.db.set_actual_video_quality(id, None).await?;
        }
        self.db.set_status(id, TaskStatus::Downloading).await?;

        // Dispatch order: known video sites (YouTube, TikTok, ...) need
        // yt-dlp's per-site extractors; open-standard HLS/DASH streams go
        // through the native adaptive engine; everything else is a plain
        // direct-file download through the native progressive engine.
        let mut outcome = if odm_engine::is_known_video_site(&task.url) {
            self.run_ytdlp(
                id,
                &task.url,
                &task.dest_path,
                task.allow_playlist,
                task.video_quality,
            )
            .await
        } else {
            let kind = odm_engine::detect_stream_kind(&self.client, &task.url)
                .await
                .ok()
                .flatten();
            match kind {
                Some(stream_kind) => {
                    self.run_adaptive(id, &task.url, &task.dest_path, stream_kind)
                        .await
                }
                None => self.run_progressive(id, &task.url, &task.dest_path).await,
            }
        };

        if outcome.is_err() && !matches!(&outcome, Err(EngineError::Cancelled)) && !task.allow_playlist {
            let fallback = self.browser_fallbacks.lock().await.get(&id).cloned();
            if let Some((media, audio)) = fallback {
                if self.db.get_task(id).await?.is_some_and(|t| t.status == TaskStatus::Downloading) {
                    outcome = self.run_browser_fallback(id, &media, audio.as_deref(), &task.dest_path, &task.url).await;
                }
            }
        }
        drop(permit);
        self.handles.lock().await.remove(&id);

        match outcome {
            Ok(()) => self.db.set_status(id, TaskStatus::Completed).await?,
            Err(EngineError::Cancelled) => {
                // `pause()` on a yt-dlp task stops it the same way a real
                // cancel does (aborting the process) and races this same
                // branch to record the outcome -- don't clobber an
                // intentional pause with "Cancelled" if that write already
                // landed first. If `pause()`'s write hasn't landed yet, it
                // still runs right after this and wins either way, so the
                // final state is "Paused" regardless of which happens first.
                let current = self.db.get_task(id).await?.map(|t| t.status);
                if current != Some(TaskStatus::Paused) {
                    self.db.set_status(id, TaskStatus::Cancelled).await?;
                }
            }
            Err(e) => self.db.set_error(id, &e.to_string()).await?,
        }
        Ok(())
    }

    async fn run_browser_fallback(&self, id: i64, media: &str, audio: Option<&str>, directory: &str, page_url: &str) -> std::result::Result<(), EngineError> {
        self.db.set_actual_video_quality(id, None).await.ok();
        let opts = odm_engine::YtdlpOptions { force_generic: true, referer: Some(page_url.to_string()), copy_index: id as u32, ..Default::default() };
        let handle = odm_engine::download_with_ytdlp(media, directory.as_ref(), &opts).await?;
        let progress = handle.progress.clone();
        self.handles.lock().await.insert(id, Handle::Ytdlp(handle.abort_handle()));
        let mut outcome = with_progress_relay(&self.db, id, progress, || handle.wait()).await.map_err(|error| {
            if matches!(error, EngineError::Cancelled) { error } else {
                EngineError::Io(std::io::Error::other("Browser backup failed. The captured link may have expired or require browser authentication. Replay the video and add a fresh download."))
            }
        })?;
        if !odm_engine::ffmpeg::has_audio(&outcome.path).await? {
            if let Some(audio) = audio {
                let audio_dir = PathBuf::from(directory).join(format!(".odm-browser-{id}-audio"));
                let handle = odm_engine::download_with_ytdlp(audio, &audio_dir, &opts).await?;
                self.handles.lock().await.insert(id, Handle::Ytdlp(handle.abort_handle()));
                let audio_file = handle.wait().await?;
                let merged = PathBuf::from(directory).join(format!("Browser merged-{id}.mp4"));
                if merged.exists() { return Err(EngineError::Io(std::io::Error::other("Merged destination already exists; add a new download."))); }
                odm_engine::ffmpeg::mux_video_audio(&odm_engine::ffmpeg::resolve_ffmpeg_path(), &outcome.path, &audio_file.path, &merged).await?;
                // These are intermediates created by this fallback attempt.
                tokio::fs::remove_file(&outcome.path).await.ok();
                tokio::fs::remove_file(&audio_file.path).await.ok();
                tokio::fs::remove_dir(&audio_dir).await.ok();
                outcome.path = merged;
            }
        }
        self.db.set_dest_path(id, &outcome.path.to_string_lossy()).await.ok();
        let height = odm_engine::ffmpeg::probe_video_height(&outcome.path).await?;
        self.db.set_actual_video_quality(id, height).await.ok();
        if height.is_none() || !odm_engine::ffmpeg::has_audio(&outcome.path).await? {
            return Err(EngineError::Io(std::io::Error::other("Captured stream is missing video or audio. A matching stream pair is required; replay the video and retry.")));
        }
        let renamed = outcome.path.with_file_name(format!("Browser video [{}p]-{}.mp4", height.unwrap(), id));
        if !renamed.exists() {
            tokio::fs::rename(&outcome.path, &renamed).await?;
            self.db.set_dest_path(id, &renamed.to_string_lossy()).await.ok();
        }
        if let Ok(metadata) = tokio::fs::metadata(&renamed).await {
            self.db.update_progress(id, metadata.len(), Some(metadata.len())).await.ok();
        }
        Ok(())
    }

    async fn run_adaptive(
        &self,
        id: i64,
        url: &str,
        dest_path: &str,
        kind: StreamKind,
    ) -> std::result::Result<(), EngineError> {
        self.handles
            .lock()
            .await
            .insert(id, Handle::RunToCompletion);
        odm_engine::download_adaptive(&self.client, url, dest_path, kind)
            .await
            .map(|_| ())
    }

    async fn run_ytdlp(
        &self,
        id: i64,
        url: &str,
        dest_dir: &str,
        allow_playlist: bool,
        video_quality: Option<u32>,
    ) -> std::result::Result<(), EngineError> {
        let cookies_from_browser = self
            .db
            .get_setting(crate::settings::COOKIES_BROWSER)
            .await
            .ok()
            .flatten()
            .filter(|s| !s.is_empty());
        let cookies_file = self
            .db
            .get_setting(crate::settings::COOKIES_FILE)
            .await
            .ok()
            .flatten()
            .filter(|s| !s.is_empty());

        // Resolve the request to one concrete source height before invoking
        // yt-dlp. This makes 1080p a real selection, not a label that can fall
        // through to 4K. If probing fails, the selector still caps at the
        // requested height and never chooses anything above it.
        let probed = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            odm_engine::probe_video_qualities_with_cookies(url, cookies_file.as_deref(), cookies_from_browser.as_deref()),
        )
        .await
        .ok()
        .and_then(std::result::Result::ok);
        let resolved_quality = match &probed {
            Some(info) => odm_engine::select_available_height(video_quality, &info.heights),
            None => video_quality,
        };
        let selected_quality = if probed.as_ref().is_some_and(|info| info.heights.is_empty()) {
            // Some Facebook/Instagram extractors expose only SD/HD format
            // labels. Numeric filters would reject every available stream.
            None
        } else {
            video_quality.and(resolved_quality)
        };
        if resolved_quality.is_some() {
            let _ = self.db.set_actual_video_quality(id, resolved_quality).await;
        }

        // Repeated downloads are always separate queue rows. Only add a copy
        // suffix when an earlier download of this URL resolved to the same
        // output height, so different resolutions retain clean filenames.
        let copy_index = self
            .db
            .list_tasks()
            .await
            .ok()
            .map(|tasks| {
                tasks
                    .iter()
                    .filter(|task| {
                        task.id < id
                            && task.url == url
                            && match resolved_quality {
                                Some(height) => {
                                    task.actual_video_quality == Some(height)
                                        || (task.actual_video_quality.is_none()
                                            && task.video_quality == video_quality)
                                }
                                None => task.video_quality.is_none(),
                            }
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        let disk_copy_index = next_video_copy_index(
            std::path::Path::new(dest_dir),
            probed.as_ref().and_then(|info| info.id.as_deref()),
            resolved_quality,
        )
        .await;
        let copy_index = copy_index.max(disk_copy_index);
        let opts = odm_engine::YtdlpOptions {
            force_generic: false,
            referer: None,
            format_id: selected_quality.map(odm_engine::quality_format_selector),
            allow_playlist,
            cookies_from_browser,
            cookies_file,
            copy_index,
        };
        let handle = odm_engine::download_with_ytdlp(url, dest_dir.as_ref(), &opts).await?;
        let progress = handle.progress.clone();
        self.handles
            .lock()
            .await
            .insert(id, Handle::Ytdlp(handle.abort_handle()));

        let outcome = with_progress_relay(&self.db, id, progress, || handle.wait()).await?;
        // `dest_dir` (see `add_download`) was a directory, not a file --
        // yt-dlp picks the real filename from the video title. Correct the
        // DB row with the true path it reports back.
        let _ = self
            .db
            .set_dest_path(id, &outcome.path.to_string_lossy())
            .await;
        let actual_height = odm_engine::ffmpeg::probe_video_height(&outcome.path)
            .await
            .ok()
            .flatten()
            .or(outcome.video_height);
        let _ = self.db.set_actual_video_quality(id, actual_height).await;
        Ok(())
    }

    /// Re-checks completed video files so rows created by older development
    /// builds no longer display source/requested quality as if it were the
    /// encoded resolution that actually landed on disk.
    pub async fn refresh_completed_video_qualities(&self) {
        let Ok(tasks) = self.list().await else { return };
        for task in tasks {
            if task.status != TaskStatus::Completed || !odm_engine::is_known_video_site(&task.url) {
                continue;
            }
            let path = std::path::Path::new(&task.dest_path);
            if !path.is_file() {
                continue;
            }
            if let Ok(Some(height)) = odm_engine::ffmpeg::probe_video_height(path).await {
                if task.actual_video_quality != Some(height) {
                    let _ = self
                        .db
                        .set_actual_video_quality(task.id, Some(height))
                        .await;
                }
            }
        }
    }

    async fn run_progressive(
        &self,
        id: i64,
        url: &str,
        dest_path: &str,
    ) -> std::result::Result<(), EngineError> {
        let handle = odm_engine::download(url, dest_path, self.config.clone()).await?;
        let progress_rx = handle.progress.clone();
        self.handles
            .lock()
            .await
            .insert(id, Handle::Progressive(handle.control.clone()));

        with_progress_relay(&self.db, id, progress_rx, || handle.wait())
            .await
            .map(|_| ())
    }

    pub async fn pause(&self, id: i64) -> Result<()> {
        let task = self.db.get_task(id).await?.ok_or(CoreError::TaskNotFound(id))?;
        if task.status == TaskStatus::Queued {
            self.db.set_status(id, TaskStatus::Paused).await?;
            return Ok(());
        }
        let mut should_mark_paused = false;
        {
            let mut handles = self.handles.lock().await;
            match handles.remove(&id) {
                Some(Handle::Progressive(control)) => {
                    control.pause.pause();
                    // Keep the live handle around -- resume() un-pauses it
                    // directly rather than restarting the download.
                    handles.insert(id, Handle::Progressive(control));
                    should_mark_paused = true;
                }
                Some(Handle::Ytdlp(abort)) => {
                    // yt-dlp has no live pause -- kill the process (it
                    // writes a resumable `.part` file as it goes; `resume()`
                    // restarts it fresh and yt-dlp picks that back up by
                    // default). Deliberately not reinserted: the process is
                    // gone, so there's nothing left to hold a handle to.
                    abort.abort();
                    should_mark_paused = true;
                }
                Some(other) => {
                    // RunToCompletion (adaptive/DASH) -- no pause support
                    // yet; put it back untouched.
                    handles.insert(id, other);
                }
                None => {}
            }
        }
        if should_mark_paused {
            self.db.set_status(id, TaskStatus::Paused).await?;
        }
        Ok(())
    }

    pub async fn resume(&self, id: i64) -> Result<()> {
        let resumed_live = {
            let handles = self.handles.lock().await;
            if let Some(Handle::Progressive(control)) = handles.get(&id) {
                control.pause.resume();
                true
            } else {
                false
            }
        };
        if resumed_live {
            self.db.set_status(id, TaskStatus::Downloading).await?;
            return Ok(());
        }

        // No live progressive handle to just un-pause -- either this is a
        // yt-dlp task paused via `pause()` above (its process is already
        // gone), or a handle orphaned by the app restarting while a task
        // sat mid-download. Either way, the only way to actually continue
        // is a fresh download for the same task: yt-dlp resumes from its
        // own `.part` file by default, and the progressive engine resumes
        // via its existing chunk/state-file tracking as long as the
        // partial file is still on disk.
        if let Some(task) = self.db.get_task(id).await? {
            if task.status == TaskStatus::Paused {
                self.db.set_status(id, TaskStatus::Queued).await?;
                self.start_task(id);
            }
        }
        Ok(())
    }

    pub async fn cancel(&self, id: i64) -> Result<()> {
        self.db.set_status(id, TaskStatus::Cancelled).await?;
        match self.handles.lock().await.get(&id) {
            Some(Handle::Progressive(control)) => control.cancel.cancel(),
            Some(Handle::Ytdlp(handle)) => handle.abort(),
            _ => {}
        }
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<Task>> {
        self.db.list_tasks().await
    }

    /// Removes a task from the list (best-effort cancel first, in case it's
    /// still running). When `delete_file` is set, also deletes the
    /// downloaded file from disk -- a missing file (e.g. one the user
    /// already moved) doesn't fail the removal, since the row should still
    /// go away either way.
    pub async fn remove(&self, id: i64, delete_file: bool) -> Result<()> {
        if self.db.get_task(id).await?.is_none() {
            return Err(CoreError::TaskNotFound(id));
        }
        let _ = self.cancel(id).await;
        if delete_file {
            if let Some(task) = self.db.get_task(id).await? {
                let path = std::path::Path::new(&task.dest_path);
                // Queued video destinations are directories, completed ones
                // are files. Never recursively delete a directory here.
                match tokio::fs::metadata(path).await {
                    Ok(meta) if meta.is_file() => { tokio::fs::remove_file(path).await?; }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                if let Some(group) = task.playlist_group.as_deref() {
                    let suffix = format!(" [{group}]");
                    let is_group_folder = |p: &std::path::Path| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&suffix));
                    let folder = if is_group_folder(path) { Some(path) } else { path.parent().filter(|p| is_group_folder(p)) };
                    if let Some(folder) = folder {
                        // remove_dir only succeeds when empty. Keep personal
                        // extras and partial files rather than deleting them.
                        let _ = tokio::fs::remove_dir(folder).await;
                    }
                }
            }
        }
        self.db.delete_task(id).await
    }

    /// Moves/renames a download's file on disk and updates the stored path
    /// to match -- used by the "Move/Rename" UI action.
    pub async fn rename(&self, id: i64, new_path: &str) -> Result<()> {
        let task = self
            .db
            .get_task(id)
            .await?
            .ok_or(CoreError::TaskNotFound(id))?;
        if let Some(parent) = std::path::Path::new(new_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::rename(&task.dest_path, new_path).await?;
        self.db.set_dest_path(id, new_path).await
    }

    /// Self-updates the bundled yt-dlp binary (`yt-dlp -U`) so site-extractor
    /// fixes land without an ODM release. Returns yt-dlp's own status line.
    pub async fn update_ytdlp(&self) -> Result<String> {
        odm_engine::update_ytdlp().await.map_err(Into::into)
    }

    /// Returns the distinct source heights available for the extension picker.
    pub async fn probe_video_qualities(&self, url: &str) -> Result<odm_engine::VideoQualities> {
        let file = self.db.get_setting(crate::settings::COOKIES_FILE).await?.filter(|s| !s.is_empty());
        let browser = self.db.get_setting(crate::settings::COOKIES_BROWSER).await?.filter(|s| !s.is_empty());
        odm_engine::probe_video_qualities_with_cookies(url, file.as_deref(), browser.as_deref())
            .await
            .map_err(Into::into)
    }

    async fn resolve_video_quality(&self, requested: Option<&str>) -> Option<u32> {
        let choice = match requested.map(str::trim) {
            Some(value) if !value.is_empty() && value != "default" => value.to_string(),
            _ => self
                .db
                .get_setting(crate::settings::VIDEO_QUALITY)
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| "best".to_string()),
        };
        if choice.eq_ignore_ascii_case("best") {
            None
        } else {
            choice
                .parse::<u32>()
                .ok()
                .filter(|height| (144..=4320).contains(height))
        }
    }

    /// Finds a free direct-download filename without overwriting an existing
    /// file or another queued task: `name.ext`, `name-1.ext`, `name-2.ext`.
    async fn unique_destination_path(
        &self,
        requested: std::path::PathBuf,
    ) -> Result<(std::path::PathBuf, bool)> {
        let task_paths: Vec<String> = self
            .db
            .list_tasks()
            .await?
            .into_iter()
            .map(|task| task.dest_path.to_lowercase())
            .collect();

        for index in 0u32.. {
            let candidate = if index == 0 {
                requested.clone()
            } else {
                path_with_copy_suffix(&requested, index)
            };
            let in_queue = task_paths
                .iter()
                .any(|path| *path == candidate.to_string_lossy().to_lowercase());
            if !in_queue && !tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
                return Ok((candidate, index > 0));
            }
        }
        unreachable!("u32 filename suffix space exhausted")
    }

    /// Imports a user-picked `cookies.txt`: keeps a copy inside
    /// `downloads_root` (so the user's original can be moved/deleted
    /// afterward) and records it as the active cookies source, taking
    /// priority over `cookies_from_browser` for subsequent yt-dlp downloads.
    /// Returns the stored copy's path.
    pub async fn import_cookies_file(&self, source_path: &str) -> Result<String> {
        let dest = self.downloads_root.join(".odm-cookies.txt");
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(source_path, &dest).await?;
        let dest_str = dest.to_string_lossy().to_string();
        self.db
            .set_setting(crate::settings::COOKIES_FILE, &dest_str)
            .await?;
        Ok(dest_str)
    }

    pub async fn clear_cookies_file(&self) -> Result<()> {
        self.db.clear_setting(crate::settings::COOKIES_FILE).await
    }
}

fn is_playlist_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false; };
    let host = parsed.host_str().unwrap_or_default();
    let youtube = host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be";
    youtube && (parsed.query_pairs().any(|(key, value)| key == "list" && !value.is_empty()) || parsed.path().starts_with("/playlist"))
}

#[cfg(test)]
mod relay_tests {
    use super::*;

    #[tokio::test]
    async fn queued_pause_prevents_download_and_cancelled_task_stays_cancelled() {
        let db = Db::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let manager = TaskManager::new(db.clone(), DownloadConfig::default(), root.path(), 1);
        let dest = root.path().join("must-not-download.mp4");
        let task = db.enqueue("https://www.youtube.com/watch?v=test", &dest.to_string_lossy(), Some("Video"), false, None).await.unwrap();
        manager.pause(task.id).await.unwrap();
        assert_eq!(db.get_task(task.id).await.unwrap().unwrap().status, TaskStatus::Paused);
        manager.run_task(task.id).await.unwrap();
        assert_eq!(db.get_task(task.id).await.unwrap().unwrap().status, TaskStatus::Paused);
        manager.cancel(task.id).await.unwrap();
        manager.run_task(task.id).await.unwrap();
        assert_eq!(db.get_task(task.id).await.unwrap().unwrap().status, TaskStatus::Cancelled);
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn playlist_removal_preserves_files_unless_requested_and_only_removes_empty_folders() {
        for (delete_files, extra_file) in [(false, false), (true, false), (true, true)] {
            let db = Db::open_in_memory().await.unwrap();
            let root = tempfile::tempdir().unwrap();
            let manager = TaskManager::new(db.clone(), DownloadConfig::default(), root.path(), 1);
            let folder = root.path().join("Playlist [playlist-test]");
            tokio::fs::create_dir_all(&folder).await.unwrap();
            let path = folder.join("video.mp4");
            tokio::fs::write(&path, b"fixture").await.unwrap();
            if extra_file { tokio::fs::write(folder.join("personal.txt"), b"keep").await.unwrap(); }
            let task = db.enqueue("https://www.youtube.com/watch?v=test", &path.to_string_lossy(), Some("Video"), false, None).await.unwrap();
            db.set_playlist_group(task.id, "playlist-test", "Playlist").await.unwrap();
            db.set_status(task.id, TaskStatus::Completed).await.unwrap();
            manager.remove(task.id, delete_files).await.unwrap();
            assert!(db.get_task(task.id).await.unwrap().is_none());
            assert_eq!(path.exists(), !delete_files);
            assert_eq!(folder.exists(), !delete_files || extra_file);
            if extra_file { assert!(folder.join("personal.txt").exists()); }
        }
    }

    #[tokio::test]
    async fn playlist_creates_all_rows_with_shared_folder_quality_and_unavailable_entries() {
        let db = Db::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let manager = TaskManager::new(db.clone(), DownloadConfig::default(), root.path(), 1);
        let _permit = manager.semaphore.clone().acquire_owned().await.unwrap();
        db.set_setting(crate::settings::VIDEO_QUALITY, "720").await.unwrap();
        let info = serde_json::json!({"title":"Test / playlist", "entries":[
            {"id":"one", "title":"First"}, {"id":"two", "title":"Second"},
            {"id":"three", "title":"Private video", "availability":"private"}
        ]});
        let first = manager.enqueue_playlist_info(&info, Some("../Custom folder"), Some("default")).await.unwrap();
        let tasks = db.list_tasks().await.unwrap();
        assert_eq!(tasks.len(), 3);
        assert!(tasks.iter().all(|t| t.playlist_group == first.playlist_group && t.video_quality == Some(720) && !t.allow_playlist));
        assert!(tasks.iter().all(|t| t.dest_path == first.dest_path && std::path::Path::new(&t.dest_path).starts_with(root.path())));
        assert_eq!(tasks.iter().filter(|t| t.status == TaskStatus::Queued).count(), 2);
        assert_eq!(tasks.iter().filter(|t| t.status == TaskStatus::Failed).count(), 1);
        assert_eq!(first.title.as_deref(), Some("1. First"));
        assert!(first.playlist_group.is_some());
        assert!(!first.url.contains("list="));
    }

    /// Run explicitly with ODM_YTDLP_PATH, ODM_FFMPEG_PATH and
    /// ODM_FFPROBE_PATH pointing to the bundled binaries.
    #[tokio::test]
    #[ignore = "requires bundled media tools"]
    async fn browser_backup_downloads_and_verifies_combined_media() {
        use axum::{routing::get, Router};
        let dir = tempfile::tempdir().unwrap();
        let fixture = dir.path().join("fixture.mp4");
        let result = tokio::process::Command::new(std::env::var("ODM_FFMPEG_PATH").unwrap())
            .args(["-v", "error", "-f", "lavfi", "-i", "color=size=320x240:rate=10", "-f", "lavfi", "-i", "sine=frequency=440", "-t", "0.3", "-c:v", "libx264", "-c:a", "aac"])
            .arg(&fixture).output().await.unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let bytes = tokio::fs::read(&fixture).await.unwrap();
        let video_path = dir.path().join("video.mp4");
        let audio_path = dir.path().join("audio.m4a");
        for (destination, flag) in [(&video_path, "-an"), (&audio_path, "-vn")] {
            let result = tokio::process::Command::new(std::env::var("ODM_FFMPEG_PATH").unwrap())
                .args(["-v", "error", "-i"]).arg(&fixture).args([flag, "-c", "copy"])
                .arg(destination).output().await.unwrap();
            assert!(result.status.success());
        }
        let video_bytes = tokio::fs::read(video_path).await.unwrap();
        let audio_bytes = tokio::fs::read(audio_path).await.unwrap();
        let app = Router::new().route("/fixture.mp4", get(move || {
            let bytes = bytes.clone();
            async move { ([("content-type", "video/mp4")], bytes) }
        }))
        .route("/video.mp4", get(move || { let bytes = video_bytes.clone(); async move { ([("content-type", "video/mp4")], bytes) } }))
        .route("/audio.m4a", get(move || { let bytes = audio_bytes.clone(); async move { ([("content-type", "audio/mp4")], bytes) } }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let db = Db::open_in_memory().await.unwrap();
        let destination = dir.path().join("output");
        let task = db.enqueue("https://www.facebook.com/reel/test", &destination.to_string_lossy(), Some("Video"), false, Some(1080)).await.unwrap();
        let manager = TaskManager::new(db.clone(), DownloadConfig::default(), dir.path(), 1);
        manager.run_browser_fallback(task.id, &format!("http://{addr}/fixture.mp4"), None, &destination.to_string_lossy(), &task.url).await.unwrap();
        let saved = db.get_task(task.id).await.unwrap().unwrap();
        assert_eq!(saved.actual_video_quality, Some(240));
        assert!(std::path::Path::new(&saved.dest_path).is_file());
        assert!(odm_engine::ffmpeg::has_audio(std::path::Path::new(&saved.dest_path)).await.unwrap());
        assert!(saved.downloaded_bytes > 0);
        let second = db.enqueue("https://www.youtube.com/watch?v=test", &destination.to_string_lossy(), Some("Video"), false, None).await.unwrap();
        manager.run_browser_fallback(second.id, &format!("http://{addr}/video.mp4"), Some(&format!("http://{addr}/audio.m4a")), &destination.to_string_lossy(), &second.url).await.unwrap();
        let merged = db.get_task(second.id).await.unwrap().unwrap();
        assert_eq!(merged.actual_video_quality, Some(240));
        assert!(odm_engine::ffmpeg::has_audio(std::path::Path::new(&merged.dest_path)).await.unwrap());
        server.abort();
    }

    /// Regression test for a bug where `with_progress_relay`'s DB-writing
    /// loop, when run as a separately `tokio::spawn`ed task, silently never
    /// observed values sent by a concurrent sibling task in the real desktop
    /// app -- despite the sender demonstrably calling `.send()` throughout an
    /// entire multi-minute download (confirmed via live tracing), the
    /// watcher's `.borrow()` stayed on the channel's initial default value
    /// the whole time, so the UI only ever saw 0 bytes until the download
    /// jumped straight to its final size on completion. Every isolated
    /// reproduction of that exact spawn pattern under a plain `#[tokio::test]`
    /// runtime worked fine, pointing at something specific to interacting
    /// with Tauri's managed runtime -- so the fix removes the second spawned
    /// task entirely and interleaves the DB writes into the caller's own task
    /// via `select!` instead, which cannot be stranded on a different
    /// execution context than the task it's timed against.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn relay_observes_progress_sent_by_a_concurrent_task() {
        let db = Db::open_in_memory().await.unwrap();
        let task = db
            .enqueue("https://example.com/x", "C:/tmp/x", None, false, None)
            .await
            .unwrap();
        let id = task.id;

        let (tx, rx) = watch::channel(Progress::default());
        let sender = tokio::spawn(async move {
            for i in 1..=20u64 {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let _ = tx.send(Progress {
                    downloaded_bytes: i * 1000,
                    total_bytes: Some(20000),
                    bytes_per_sec: 0.0,
                    active_chunks: 1,
                });
            }
        });

        with_progress_relay(&db, id, rx, || async move { sender.await.unwrap() }).await;

        let final_task = db.get_task(id).await.unwrap().unwrap();
        assert_eq!(
            final_task.downloaded_bytes, 20000,
            "relay never observed the sender's progress updates"
        );
        assert_eq!(final_task.total_bytes, Some(20000));
    }

    #[tokio::test]
    async fn video_copy_index_survives_cleared_download_history() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("Video [abc123] [1080p].mp4"), b"first")
            .await
            .unwrap();
        tokio::fs::write(dir.path().join("Video [abc123] [1080p]-1.mp4"), b"second")
            .await
            .unwrap();

        assert_eq!(
            next_video_copy_index(dir.path(), Some("abc123"), Some(1080)).await,
            2
        );
        assert_eq!(
            next_video_copy_index(dir.path(), Some("abc123"), Some(720)).await,
            0
        );
    }
}

fn filename_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?;
    if last.is_empty() {
        None
    } else {
        Some(
            percent_encoding::percent_decode_str(last)
                .decode_utf8_lossy()
                .to_string(),
        )
    }
}

fn valid_browser_media_url(value: &str) -> bool {
    let Ok(url) = url::Url::parse(value) else { return false };
    url.scheme() == "https" && url.host_str().is_some_and(|host|
        ["fbcdn.net", "cdninstagram.com", "googlevideo.com", "tiktok.com", "tiktokcdn.com"]
            .iter().any(|suffix| host.ends_with(&format!(".{suffix}"))))
}

fn path_with_copy_suffix(path: &std::path::Path, index: u32) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let extension = path.extension().and_then(|value| value.to_str());
    let filename = match extension {
        Some(extension) => format!("{stem}-{index}.{extension}"),
        None => format!("{stem}-{index}"),
    };
    path.with_file_name(filename)
}

/// Returns the next free copy suffix by inspecting self-describing video
/// filenames such as `Title [video-id] [1080p].mp4` and `... [1080p]-1.mp4`.
/// This remains correct even if the user cleared ODM's download history but
/// kept the actual files on disk.
async fn next_video_copy_index(
    dest_dir: &std::path::Path,
    video_id: Option<&str>,
    height: Option<u32>,
) -> u32 {
    let (Some(video_id), Some(height)) = (video_id, height) else {
        return 0;
    };
    let marker = format!("[{video_id}] [{height}p]");
    let Ok(mut entries) = tokio::fs::read_dir(dest_dir).await else {
        return 0;
    };
    let mut highest: Option<u32> = None;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((_, tail)) = name.rsplit_once(&marker) else {
            continue;
        };
        let index = tail
            .strip_prefix('-')
            .and_then(|suffix| suffix.split('.').next())
            .and_then(|digits| digits.parse::<u32>().ok())
            .unwrap_or(0);
        highest = Some(highest.map_or(index, |current| current.max(index)));
    }
    highest.map_or(0, |index| index.saturating_add(1))
}

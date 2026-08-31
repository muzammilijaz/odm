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
async fn with_progress_relay<F, Fut, T>(db: &Db, id: i64, progress_rx: watch::Receiver<Progress>, body: F) -> T
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
    let _ = db.update_progress(id, final_progress.downloaded_bytes, final_progress.total_bytes).await;

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
}

impl TaskManager {
    pub fn new(db: Db, config: DownloadConfig, downloads_root: impl Into<PathBuf>, max_concurrent: usize) -> Self {
        Self {
            db,
            config,
            downloads_root: downloads_root.into(),
            semaphore: Arc::new(Semaphore::new(max_concurrent.max(1))),
            client: reqwest::Client::new(),
            handles: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Adds a URL to the queue: resolves its category by extension (routing
    /// it to that category's default subfolder), derives a destination
    /// filename when the caller doesn't supply one, and immediately starts it
    /// (subject to the concurrency limit).
    pub async fn add_download(&self, url: &str, filename_hint: Option<&str>, allow_playlist: bool) -> Result<Task> {
        // Known video sites (YouTube, TikTok, ...) go through yt-dlp, whose
        // real output filename (video title + chosen container) isn't known
        // until the download finishes -- so there's no meaningful filename
        // to derive from the URL itself (e.g. youtube.com/watch?v=... has no
        // usable path segment). Store the destination *directory*; the
        // actual path is corrected via `Db::set_dest_path` once yt-dlp
        // reports it (see `run_ytdlp`).
        if odm_engine::is_known_video_site(url) {
            let video_category = self.db.list_categories().await?.into_iter().find(|c| c.name.eq_ignore_ascii_case("video"));
            let dest_dir = match &video_category {
                Some(c) => self.downloads_root.join(&c.default_folder),
                None => self.downloads_root.join("Video"),
            };
            let task = self
                .db
                .enqueue(url, &dest_dir.to_string_lossy(), video_category.as_ref().map(|c| c.name.as_str()), allow_playlist)
                .await?;

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
            if task.title.is_none() {
                let db = self.db.clone();
                let url = url.to_string();
                let id = task.id;
                tokio::spawn(async move {
                    if let Ok(Ok((title, thumbnail))) = tokio::time::timeout(std::time::Duration::from_secs(10), odm_engine::probe_title_thumbnail(&url)).await
                    {
                        let _ = db.set_metadata(id, &title, thumbnail.as_deref()).await;
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
        let dest_path = dest_dir.join(&filename);

        let task = self
            .db
            .enqueue(url, &dest_path.to_string_lossy(), category.as_ref().map(|c| c.name.as_str()), false)
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

    async fn run_task(&self, id: i64) -> Result<()> {
        let permit = self.semaphore.clone().acquire_owned().await.expect("semaphore never closed");

        let Some(task) = self.db.get_task(id).await? else { return Ok(()) };
        self.db.set_status(id, TaskStatus::Downloading).await?;

        // Dispatch order: known video sites (YouTube, TikTok, ...) need
        // yt-dlp's per-site extractors; open-standard HLS/DASH streams go
        // through the native adaptive engine; everything else is a plain
        // direct-file download through the native progressive engine.
        let outcome = if odm_engine::is_known_video_site(&task.url) {
            self.run_ytdlp(id, &task.url, &task.dest_path, task.allow_playlist).await
        } else {
            let kind = odm_engine::detect_stream_kind(&self.client, &task.url).await.ok().flatten();
            match kind {
                Some(stream_kind) => self.run_adaptive(id, &task.url, &task.dest_path, stream_kind).await,
                None => self.run_progressive(id, &task.url, &task.dest_path).await,
            }
        };

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

    async fn run_adaptive(&self, id: i64, url: &str, dest_path: &str, kind: StreamKind) -> std::result::Result<(), EngineError> {
        self.handles.lock().await.insert(id, Handle::RunToCompletion);
        odm_engine::download_adaptive(&self.client, url, dest_path, kind).await.map(|_| ())
    }

    async fn run_ytdlp(&self, id: i64, url: &str, dest_dir: &str, allow_playlist: bool) -> std::result::Result<(), EngineError> {
        let cookies_from_browser = self.db.get_setting(crate::settings::COOKIES_BROWSER).await.ok().flatten().filter(|s| !s.is_empty());
        let cookies_file = self.db.get_setting(crate::settings::COOKIES_FILE).await.ok().flatten().filter(|s| !s.is_empty());
        let opts = odm_engine::YtdlpOptions { allow_playlist, cookies_from_browser, cookies_file, ..Default::default() };
        let handle = odm_engine::download_with_ytdlp(url, dest_dir.as_ref(), &opts).await?;
        let progress = handle.progress.clone();
        self.handles.lock().await.insert(id, Handle::Ytdlp(handle.abort_handle()));

        let real_path = with_progress_relay(&self.db, id, progress, || handle.wait()).await?;
        // `dest_dir` (see `add_download`) was a directory, not a file --
        // yt-dlp picks the real filename from the video title. Correct the
        // DB row with the true path it reports back.
        let _ = self.db.set_dest_path(id, &real_path.to_string_lossy()).await;
        Ok(())
    }

    async fn run_progressive(&self, id: i64, url: &str, dest_path: &str) -> std::result::Result<(), EngineError> {
        let handle = odm_engine::download(url, dest_path, self.config.clone()).await?;
        let progress_rx = handle.progress.clone();
        self.handles.lock().await.insert(id, Handle::Progressive(handle.control.clone()));

        with_progress_relay(&self.db, id, progress_rx, || handle.wait()).await.map(|_| ())
    }

    pub async fn pause(&self, id: i64) -> Result<()> {
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
        if let Some(Handle::Progressive(control)) = self.handles.lock().await.get(&id) {
            control.cancel.cancel();
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
                let _ = tokio::fs::remove_file(&task.dest_path).await;
            }
        }
        self.db.delete_task(id).await
    }

    /// Moves/renames a download's file on disk and updates the stored path
    /// to match -- used by the "Move/Rename" UI action.
    pub async fn rename(&self, id: i64, new_path: &str) -> Result<()> {
        let task = self.db.get_task(id).await?.ok_or(CoreError::TaskNotFound(id))?;
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
        self.db.set_setting(crate::settings::COOKIES_FILE, &dest_str).await?;
        Ok(dest_str)
    }

    pub async fn clear_cookies_file(&self) -> Result<()> {
        self.db.clear_setting(crate::settings::COOKIES_FILE).await
    }
}

#[cfg(test)]
mod relay_tests {
    use super::*;

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
        let task = db.enqueue("https://example.com/x", "C:/tmp/x", None, false).await.unwrap();
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
        assert_eq!(final_task.downloaded_bytes, 20000, "relay never observed the sender's progress updates");
        assert_eq!(final_task.total_bytes, Some(20000));
    }
}

fn filename_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.next_back()?;
    if last.is_empty() {
        None
    } else {
        Some(percent_encoding::percent_decode_str(last).decode_utf8_lossy().to_string())
    }
}

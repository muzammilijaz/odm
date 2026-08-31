//! Delegates site-specific video/audio extraction to a bundled `yt-dlp`
//! binary rather than reimplementing the same reverse-engineered,
//! constantly-breaking per-site signing algorithms directly. Generic
//! direct-file links stay on the native engine; open-standard HLS/DASH stays
//! on the native adaptive engine. This module is reserved for sites that
//! need account/API-signature resolution — YouTube, TikTok, Instagram, etc.

use crate::error::{EngineError, Result};
use crate::progress::Progress;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Hostnames known to require yt-dlp's per-site extractors rather than a
/// direct or open-standard-adaptive download. Deliberately conservative —
/// unrecognized hosts fall through to the generic engine, which is correct
/// for the overwhelming majority of direct-file links.
const KNOWN_VIDEO_HOSTS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "m.youtube.com",
    "tiktok.com",
    "vm.tiktok.com",
    "instagram.com",
    "facebook.com",
    "fb.watch",
    "twitter.com",
    "x.com",
    "vimeo.com",
    "dailymotion.com",
    "twitch.tv",
    "clips.twitch.tv",
    "soundcloud.com",
    "reddit.com",
];

pub fn is_known_video_site(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    let Some(host) = parsed.host_str() else { return false };
    KNOWN_VIDEO_HOSTS.iter().any(|known| host == *known || host.ends_with(&format!(".{known}")))
}

/// Resolves the yt-dlp binary: an `ODM_YTDLP_PATH` override, a copy bundled
/// next to the running executable, or whatever `yt-dlp` is on `PATH`.
pub fn resolve_ytdlp_path() -> PathBuf {
    if let Ok(p) = std::env::var("ODM_YTDLP_PATH") {
        return PathBuf::from(p);
    }
    let exe_name = if cfg!(windows) { "yt-dlp.exe" } else { "yt-dlp" };
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

/// Resolves a bundled QuickJS binary, if present (an `ODM_QUICKJS_PATH`
/// override, or one bundled next to the running executable). Returns `None`
/// rather than falling back to bare `quickjs`/`quickjs.exe` on `PATH`,
/// unlike the other resolvers -- QuickJS isn't something most systems have
/// installed, so silently passing a bare name that won't resolve would just
/// make yt-dlp's `--js-runtimes` invocation fail; better to omit the flag
/// entirely and let yt-dlp fall back to its own defaults.
///
/// This matters for real extraction reliability, not just nice-to-have:
/// yt-dlp needs a JS runtime to solve JS-based extraction challenges on
/// sites like YouTube (confirmed live: our own yt-dlp invocations warn "No
/// supported JavaScript runtime could be found... YouTube extraction
/// without a JS runtime has been deprecated, and some formats may be
/// missing" on every run). `quickjs` is one of yt-dlp's own supported
/// runtimes (`--help`: "deno, node, quickjs, bun") and, at ~2MB, is by far
/// the lightest one to bundle compared to Deno or Node.
pub fn resolve_quickjs_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ODM_QUICKJS_PATH") {
        return Some(PathBuf::from(p));
    }
    let exe_name = if cfg!(windows) { "quickjs.exe" } else { "quickjs" };
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let bundled = dir.join(exe_name);
    bundled.exists().then_some(bundled)
}

/// Fetches yt-dlp's metadata/format-list JSON for `url` (`yt-dlp -J`), for a
/// future quality-picker UI. Doesn't download anything.
pub async fn probe_formats(url: &str) -> Result<serde_json::Value> {
    let ytdlp = resolve_ytdlp_path();
    let output = Command::new(&ytdlp)
        .args(["-J", "--no-playlist", url])
        .output()
        .await
        .map_err(EngineError::Io)?;

    if !output.status.success() {
        return Err(ytdlp_error(&output.stderr, output.status));
    }
    serde_json::from_slice(&output.stdout).map_err(EngineError::from)
}

/// Fetches just a video's title and thumbnail URL upfront (built on
/// `probe_formats`), so the UI can show the real name/preview for the whole
/// download instead of only once the file lands and its title is derived
/// from the actual downloaded filename.
pub async fn probe_title_thumbnail(url: &str) -> Result<(String, Option<String>)> {
    let info = probe_formats(url).await?;
    let title = info.get("title").and_then(|v| v.as_str()).unwrap_or("video").to_string();
    let thumbnail = info.get("thumbnail").and_then(|v| v.as_str()).map(|s| s.to_string());
    Ok((title, thumbnail))
}

/// Self-updates the bundled yt-dlp binary in place (`yt-dlp -U`).
///
/// This matters operationally, not just as a nice-to-have: yt-dlp's
/// per-site extractors break every time a site changes its player/API, and
/// a bundled binary that's pinned at install time will silently start
/// failing downloads on popular sites until updated. Returns yt-dlp's own
/// report of what happened ("up to date" / "updated to X" / etc).
pub async fn update_ytdlp() -> Result<String> {
    let ytdlp = resolve_ytdlp_path();
    let output = Command::new(&ytdlp).arg("-U").output().await.map_err(EngineError::Io)?;
    if !output.status.success() {
        return Err(ytdlp_error(&output.stderr, output.status));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Handle to a running yt-dlp download: a live progress feed (parsed from
/// yt-dlp's own `--progress-template` JSON output) plus the eventual real
/// output path.
pub struct YtdlpHandle {
    pub progress: watch::Receiver<Progress>,
    join: JoinHandle<Result<PathBuf>>,
}

impl YtdlpHandle {
    pub async fn wait(self) -> Result<PathBuf> {
        match self.join.await {
            Ok(result) => result,
            Err(_join_err) => Err(EngineError::Cancelled),
        }
    }

    /// A handle that can stop this download from outside `wait()` -- yt-dlp
    /// has no live pause/resume of its own, so "pause" here means kill the
    /// process; "resume" is a fresh `download_with_ytdlp` call for the same
    /// URL/destination, which yt-dlp resumes from its own `.part` file by
    /// default. Aborting the underlying task drops the `Child` mid-future,
    /// which (combined with `kill_on_drop(true)` on the spawned command)
    /// actually kills the OS process rather than leaving it orphaned.
    pub fn abort_handle(&self) -> tokio::task::AbortHandle {
        self.join.abort_handle()
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct YtdlpProgressLine {
    downloaded_bytes: Option<f64>,
    total_bytes: Option<f64>,
    total_bytes_estimate: Option<f64>,
    speed: Option<f64>,
}

/// Extra per-download choices layered on yt-dlp's own defaults. Kept as one
/// struct (rather than stacking `Option` params) since this shape of
/// per-job options (url/playlist/cookies-from-browser/cookies-file/section)
/// covers the real-world cases cleanly.
#[derive(Debug, Default, Clone)]
pub struct YtdlpOptions {
    /// Selects a specific format from `probe_formats`'s output instead of
    /// yt-dlp's own best-quality default.
    pub format_id: Option<String>,
    /// If the URL is a playlist, download every entry instead of just the
    /// one video. Defaults to off, since accidentally pulling in an entire
    /// playlist is a worse default than requiring an explicit opt-in.
    pub allow_playlist: bool,
    /// Import cookies from an installed browser (chrome, firefox, edge,
    /// brave, ...) so member-only/private/age-restricted videos the user is
    /// already logged into (in that browser) can be downloaded. Mirrors
    /// yt-dlp's `--cookies-from-browser`. Ignored when `cookies_file` is
    /// also set -- they're mutually exclusive in yt-dlp, and a file is more
    /// reliable (reading a browser's live cookie store fails while that
    /// browser is running and holding it open).
    pub cookies_from_browser: Option<String>,
    /// Path to a Netscape-format `cookies.txt`. Takes priority over
    /// `cookies_from_browser` when both are set. Mirrors yt-dlp's
    /// `--cookies`.
    pub cookies_file: Option<String>,
}

/// Downloads `url` via yt-dlp into `dest_dir` (best available quality by
/// default, or `opts.format_id` when set).
///
/// Unlike the progressive/adaptive engines, the caller can't dictate an
/// exact output filename here: the real filename depends on the video's
/// title and yt-dlp's own extension/container choice, resolved only once
/// the download (and any muxing) finishes. So this takes a *directory* and
/// a `%(title)s.%(ext)s`-style template, and asks yt-dlp to print the true
/// final path (`--print after_move:filepath`) rather than guessing it.
///
/// Progress is streamed live rather than only reported once at the end:
/// yt-dlp is run with `--progress-template` emitting one JSON line per
/// update (prefixed `download:`), read incrementally from its stdout pipe
/// and republished as the same `Progress` type the progressive/adaptive
/// engines use.
pub async fn download_with_ytdlp(url: &str, dest_dir: &Path, opts: &YtdlpOptions) -> Result<YtdlpHandle> {
    tokio::fs::create_dir_all(dest_dir).await?;
    let before_entries = snapshot_dir(dest_dir).await;

    let ytdlp = resolve_ytdlp_path();
    let output_template = dest_dir.join("%(title).200B [%(id)s].%(ext)s");
    let output_template_str = output_template.display().to_string();

    // yt-dlp needs its own ffmpeg location -- it has no idea our engine
    // resolves one via ODM_FFMPEG_PATH/bundled-next-to-exe. Without this,
    // best-quality downloads that need separate video+audio streams merged
    // (the common case) silently fail to merge and leave two unmuxed files
    // behind instead of one final video. Confirmed live: got
    // "Title [id].f395.mp4" + "Title [id].f251.webm" instead of one merged
    // file, because yt-dlp couldn't find ffmpeg.
    let ffmpeg_path = crate::adaptive::ffmpeg::resolve_ffmpeg_path();
    let ffmpeg_path_str = ffmpeg_path.display().to_string();

    let mut args: Vec<String> = vec![
        "-o".into(),
        output_template_str,
        "--print".into(),
        "after_move:filepath".into(),
        // `--print` alone silently suppresses `--progress-template`'s
        // "download" output entirely (confirmed live: with `--print` and no
        // `--progress`, the download's own progress JSON never appears on
        // stdout at all -- not delayed, just gone -- so `downloaded_bytes`
        // sat frozen at 0 for the whole download and only jumped to the true
        // final size once the post-download `fs::metadata` stat below ran).
        // `--progress` forces the progress mechanism on regardless, which is
        // what actually lets our template through.
        "--progress".into(),
        "--ffmpeg-location".into(),
        ffmpeg_path_str,
    ];
    if let Some(quickjs_path) = resolve_quickjs_path() {
        args.push("--js-runtimes".into());
        args.push(format!("quickjs:{}", quickjs_path.display()));
    }
    args.extend([
        "--newline".into(),
        "--progress-template".into(),
        // Progress fields live under the "progress" namespace, not as a
        // single top-level value -- `%(progress)j` (what this used to say)
        // isn't valid and silently produced nothing parseable, which is why
        // progress looked frozen at 0 for the whole download regardless of
        // the buffering fix below. Build the JSON object from the actual
        // namespaced fields instead (`--help` confirms: "the progress
        // attributes are accessible under 'progress' key").
        r#"download:{"downloaded_bytes":%(progress.downloaded_bytes)j,"total_bytes":%(progress.total_bytes)j,"total_bytes_estimate":%(progress.total_bytes_estimate)j,"speed":%(progress.speed)j}"#.into(),
        // Forces yt-dlp's own output encoding to UTF-8 -- Python's default
        // stdout encoding on Windows falls back to the legacy system
        // codepage whenever stdout isn't a real console (exactly our case,
        // piping it), which silently mangles non-ASCII titles regardless of
        // PYTHONIOENCODING/PYTHONUTF8 env vars (confirmed live: both set,
        // still got cp1252 bytes back). `run_ytdlp` also diffs the
        // destination directory rather than trusting this printed path, as
        // a second line of defense.
        "--encoding".into(),
        "utf-8".into(),
        // Always merge/remux to mp4 -- without this yt-dlp's container
        // choice follows whatever the source formats were (confirmed live:
        // got a .webm for a source that had no native mp4 pairing), which
        // is a worse default for a general-purpose download manager than
        // one consistent, universally-playable container.
        "--merge-output-format".into(),
        "mp4".into(),
        // Prefer h264/aac (broadly compatible) over vp9/av1/opus when an
        // equivalent-quality option exists, but fall through to whatever's
        // actually available rather than failing when a site only offers
        // vp9/av1 (increasingly common, e.g. many YouTube 1080p+ streams).
        "-S".into(),
        "vcodec:h264,acodec:aac,res,fps".into(),
        // DASH/HLS-fragmented formats (the common case for these sites)
        // download noticeably faster with a few fragments in flight instead
        // of yt-dlp's serial default.
        "--concurrent-fragments".into(),
        "4".into(),
        "--fragment-retries".into(),
        "10".into(),
        "--retry-sleep".into(),
        "linear=1:5".into(),
        "--socket-timeout".into(),
        "30".into(),
    ]);
    args.push(if opts.allow_playlist { "--yes-playlist".into() } else { "--no-playlist".into() });
    if let Some(fmt) = &opts.format_id {
        args.push("-f".into());
        args.push(fmt.clone());
    }
    if let Some(cookies_file) = &opts.cookies_file {
        args.push("--cookies".into());
        args.push(cookies_file.clone());
    } else if let Some(browser) = &opts.cookies_from_browser {
        args.push("--cookies-from-browser".into());
        args.push(browser.clone());
    }
    args.push(url.into());

    let mut child = Command::new(&ytdlp)
        .args(&args)
        // yt-dlp is a frozen Python program; Python block-buffers stdout
        // instead of flushing per line whenever it isn't a real terminal
        // (i.e. exactly our case, piping it). Without this, every
        // `--progress-template` line sits in that buffer until it fills or
        // the process exits, so progress looks like it jumps straight from
        // 0 to done -- confirmed live (15s of polling during a real
        // download showed downloaded_bytes stuck at 0 the whole time).
        .env("PYTHONUNBUFFERED", "1")
        // Python defaults stdout to the legacy system codepage (not UTF-8)
        // on Windows whenever it isn't attached to a real console -- exactly
        // our case, piping it. That silently mangles non-ASCII titles
        // (confirmed live: a real file named
        // "...136 Years？ 😭 Business...｜..." came back from
        // `--print after_move:filepath` as "...136 Years  Business...  ...",
        // the full-width punctuation and emoji replaced with spaces), which
        // then made the post-download `fs::metadata` stat silently fail
        // against a path that didn't actually exist.
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Lets `YtdlpHandle::abort_handle()` actually kill the OS process
        // (used for "pause") rather than just dropping our side of the pipe
        // and leaving yt-dlp/ffmpeg running orphaned in the background.
        .kill_on_drop(true)
        .spawn()
        .map_err(EngineError::Io)?;

    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let (progress_tx, progress_rx) = watch::channel(Progress::default());
    let dest_dir_owned = dest_dir.to_path_buf();

    let join = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut final_path: Option<PathBuf> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            // `download:` in `--progress-template download:{...}` (built
            // above) is yt-dlp's own TYPES:TEMPLATE selector syntax -- it
            // tells yt-dlp which event the template applies to and is
            // consumed internally, never echoed back into the printed
            // output. yt-dlp just prints the substituted JSON directly, so
            // stripping a "download:" prefix here never matched anything
            // (confirmed live: `downloaded_bytes`/`total_bytes` stayed frozen
            // at their zero/null defaults for an entire multi-minute
            // download, jumping straight to the true final size only once
            // the post-download `fs::metadata` stat below ran). Dispatch on
            // whether the line actually parses as our JSON shape instead.
            let trimmed = line.trim();
            if trimmed.starts_with('{') {
                // yt-dlp's `%()j` (json-encode) template operator prints the
                // bare token `NA` -- not valid JSON, and not `null` either --
                // for a field it has no value for yet (`total_bytes_estimate`
                // and `speed` both start out this way at the beginning of a
                // download). `serde_json::from_str` treats that as a syntax
                // error and fails the *entire* line, silently, via the `if
                // let Ok(..)` below -- which is why this was the real root
                // cause of progress looking frozen at 0 for an entire
                // download despite everything upstream (the process
                // spawning, the pipe being read, real lines arriving) working
                // correctly: confirmed live by printing the parse error
                // itself, which was "expected value" at the exact column
                // where `NA` starts, on literally every single line. `:NA`
                // only ever appears as a bare value in this specific
                // template's output, never inside a legitimate string or
                // number, so a blind substitution is safe here.
                if let Ok(p) = serde_json::from_str::<YtdlpProgressLine>(&trimmed.replace(":NA", ":null")) {
                    let downloaded = p.downloaded_bytes.unwrap_or(0.0).round() as u64;
                    let total = p.total_bytes.or(p.total_bytes_estimate).map(|t| t.round() as u64);
                    let _ = progress_tx.send(Progress {
                        downloaded_bytes: downloaded,
                        total_bytes: total,
                        bytes_per_sec: p.speed.unwrap_or(0.0),
                        active_chunks: 1,
                    });
                }
            } else if !trimmed.is_empty() {
                final_path = Some(PathBuf::from(trimmed));
            }
        }

        let mut stderr_buf = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut stderr_buf).await;

        let status = child.wait().await.map_err(EngineError::Io)?;
        if !status.success() {
            let tail: String = stderr_buf.lines().rev().take(6).collect::<Vec<_>>().join(" | ");
            if stderr_buf.contains("Could not copy") && stderr_buf.contains("cookie database") {
                // yt-dlp copies the browser's live cookie DB to read it, which
                // fails whenever that browser still holds the file open --
                // the #1 cause of this error (confirmed by yt-dlp's own
                // linked issue). Surface the actual fix instead of the raw
                // Python traceback tail.
                return Err(EngineError::Io(std::io::Error::other(
                    "Could not read cookies: close the selected browser completely (including any background/tray process) and try again, or use \"Your own cookies file\" in Settings instead.",
                )));
            }
            if stderr_buf.contains("Sign in to confirm you") && stderr_buf.contains("not a bot") {
                // YouTube's bot-check gate -- yt-dlp needs cookies from a
                // logged-in browser session to get past it. Not a bug in ODM;
                // point the user at the fix (Settings -> pick a browser to
                // import cookies from) instead of the raw yt-dlp traceback.
                return Err(EngineError::Io(std::io::Error::other(
                    "YouTube is asking to confirm you're not a bot. Go to Settings and select a browser you're signed into YouTube with (under cookies/sign-in) so yt-dlp can use its session, then retry this download.",
                )));
            }
            return Err(EngineError::Io(std::io::Error::other(format!("yt-dlp exited with {status}: {tail}"))));
        }

        // Prefer diffing the destination directory over the path yt-dlp
        // printed: even with `--encoding utf-8`, non-ASCII titles have been
        // seen coming back mangled (see comment above), silently pointing
        // at a file that doesn't exist. A before/after directory snapshot
        // sidesteps the whole stdout-encoding question entirely.
        let after_entries = snapshot_dir(&dest_dir_owned).await;
        let mut new_entries: Vec<PathBuf> = after_entries.difference(&before_entries).cloned().collect();
        if new_entries.len() == 1 {
            final_path = Some(new_entries.remove(0));
        } else if let Some(p) = &final_path {
            if !tokio::fs::try_exists(p).await.unwrap_or(false) {
                if let Some(newest) = pick_newest(&new_entries).await {
                    final_path = Some(newest);
                }
            }
        }

        let final_path = final_path.ok_or_else(|| EngineError::Io(std::io::Error::other("yt-dlp did not report a final file path")))?;

        // Short/fast downloads can finish before yt-dlp ever emits a
        // progress-template line (confirmed live: a 19-second video showed
        // 0 B the whole time despite completing correctly) -- stat the real
        // output file so the final progress report reflects the truth
        // rather than staying at its zero default.
        if let Ok(meta) = tokio::fs::metadata(&final_path).await {
            let size = meta.len();
            let _ = progress_tx.send(Progress {
                downloaded_bytes: size,
                total_bytes: Some(size),
                bytes_per_sec: 0.0,
                active_chunks: 0,
            });
        }

        Ok(final_path)
    });

    Ok(YtdlpHandle { progress: progress_rx, join })
}

fn ytdlp_error(stderr: &[u8], status: std::process::ExitStatus) -> EngineError {
    let text = String::from_utf8_lossy(stderr);
    let tail: String = text.lines().rev().take(6).collect::<Vec<_>>().join(" | ");
    EngineError::Io(std::io::Error::other(format!("yt-dlp exited with {status}: {tail}")))
}

/// Full paths of every entry directly inside `dir` (non-recursive). Used to
/// diff before/after a yt-dlp run and find the file it actually produced,
/// independent of whatever yt-dlp printed about it.
async fn snapshot_dir(dir: &Path) -> std::collections::HashSet<PathBuf> {
    let mut entries = std::collections::HashSet::new();
    if let Ok(mut read_dir) = tokio::fs::read_dir(dir).await {
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            entries.insert(entry.path());
        }
    }
    entries
}

async fn pick_newest(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
    for p in paths {
        if let Ok(meta) = tokio::fs::metadata(p).await {
            if let Ok(modified) = meta.modified() {
                if best.as_ref().is_none_or(|(_, t)| modified > *t) {
                    best = Some((p.clone(), modified));
                }
            }
        }
    }
    best.map(|(p, _)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_known_video_hosts_and_subdomains() {
        assert!(is_known_video_site("https://www.youtube.com/watch?v=abc"));
        assert!(is_known_video_site("https://youtu.be/abc"));
        assert!(is_known_video_site("https://vm.tiktok.com/xyz"));
        assert!(is_known_video_site("https://m.youtube.com/watch?v=abc"));
    }

    #[test]
    fn does_not_flag_generic_direct_file_hosts() {
        assert!(!is_known_video_site("https://cdn.example.com/archive.zip"));
        assert!(!is_known_video_site("not a url"));
    }

    /// Isolated (no Tauri) regression test: races a real yt-dlp child
    /// process's progress feed against a plain timer reading
    /// `handle.progress.clone()`, mirroring how `TaskManager::run_ytdlp`
    /// uses this handle. Guards against yt-dlp's `%()j` (json-encode)
    /// template operator printing the bare token `NA` (not valid JSON, not
    /// `null`) for a field it has no value for yet -- `serde_json::from_str`
    /// treats that as a syntax error and silently fails the *entire* line,
    /// which meant progress looked frozen at 0 for an entire download
    /// despite the process, the pipe, and every line actually arriving
    /// correctly. Requires network + the bundled binaries; run manually with
    /// `cargo test -p odm-engine --lib real_ytdlp_progress_is_observed_live -- --ignored --nocapture`.
    #[ignore]
    #[tokio::test(flavor = "multi_thread")]
    async fn real_ytdlp_progress_is_observed_live() {
        let binaries = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../desktop/src-tauri/binaries");
        std::env::set_var("ODM_YTDLP_PATH", binaries.join("yt-dlp-x86_64-pc-windows-msvc.exe"));
        std::env::set_var("ODM_FFMPEG_PATH", binaries.join("ffmpeg-x86_64-pc-windows-msvc.exe"));

        let dest_dir = std::env::temp_dir().join("odm_repro_test");
        let opts = YtdlpOptions::default();
        let handle = download_with_ytdlp("https://www.youtube.com/watch?v=aqz-KE-bpKQ", &dest_dir, &opts).await.unwrap();
        let mut progress_rx = handle.progress.clone();

        let watcher = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(250));
            for _ in 0..40 {
                interval.tick().await;
                let p = *progress_rx.borrow_and_update();
                if p.downloaded_bytes > 0 {
                    return true;
                }
            }
            false
        });

        let saw_nonzero = watcher.await.unwrap();
        // Don't wait for the whole multi-minute download in a test -- once
        // we've proven (or disproven) live visibility, that's the answer.
        assert!(saw_nonzero, "never observed a non-zero progress value while the real yt-dlp process was running");
    }
}

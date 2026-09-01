//! Core multi-connection HTTP download engine for ODM.
//!
//! Chunked parallel ranged downloads, cooperative pause/cancel, live speed
//! throttling, backoff-with-jitter retry, single-connection fallback, and a
//! system/explicit proxy model, implemented in async Rust on top of
//! `reqwest`/`tokio`.

mod adaptive;
mod chunk;
mod config;
mod control;
mod engine;
mod error;
mod posio;
mod process_ext;
mod progress;
mod proxy;
mod retry;
mod state;
mod throttle;
mod ytdlp;

pub use adaptive::{detect_stream_kind, download_adaptive, ffmpeg, StreamKind};
pub use ytdlp::{
    download_with_ytdlp, is_known_video_site, probe_formats as probe_ytdlp_formats, probe_title_thumbnail, update_ytdlp, YtdlpHandle, YtdlpOptions,
};
pub use chunk::Chunk;
pub use config::{DownloadConfig, FileExistPolicy};
pub use control::{DownloadControl, PauseToken};
pub use engine::{download, DownloadHandle};
pub use error::{EngineError, Result};
pub use progress::Progress;
pub use proxy::ProxyConfig;
pub use state::DownloadState;

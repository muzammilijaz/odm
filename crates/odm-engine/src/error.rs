use std::io;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("state (de)serialization failed: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("invalid proxy url: {0}")]
    InvalidProxy(String),

    #[error("server returned status {0}")]
    BadStatus(reqwest::StatusCode),

    #[error("download was cancelled")]
    Cancelled,

    #[error("chunk {index} failed after {attempts} attempts: {source}")]
    ChunkFailed {
        index: usize,
        attempts: u32,
        #[source]
        source: Box<EngineError>,
    },

    #[error("resume state does not match remote file (expected {expected} bytes, got {actual})")]
    ResumeMismatch { expected: u64, actual: u64 },
}

pub type Result<T> = std::result::Result<T, EngineError>;

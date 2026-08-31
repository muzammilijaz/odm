use crate::proxy::ProxyConfig;
use std::collections::HashMap;
use std::time::Duration;

/// What to do when the destination file already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FileExistPolicy {
    #[default]
    Overwrite,
    Skip,
    Rename,
    Error,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadConfig {
    /// How many pieces to split the file into (upper bound; shrinks automatically
    /// for small files or servers without range support).
    pub chunk_count: usize,
    /// How many chunks may be actively downloading at once. Independent of
    /// `chunk_count`, capped by a semaphore.
    pub parallel_count: usize,
    /// Below this size, don't bother splitting into multiple chunks.
    pub min_chunk_size: u64,
    /// 0 = unlimited. Divided live across currently-active chunks.
    pub max_bytes_per_sec: u64,
    /// Per-chunk retry attempts before giving up.
    pub max_retries: u32,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub extra_headers: HashMap<String, String>,
    pub user_agent: String,
    pub file_exist_policy: FileExistPolicy,
    pub proxy: ProxyConfig,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            chunk_count: 8,
            parallel_count: 4,
            min_chunk_size: 1024 * 1024, // 1 MiB
            max_bytes_per_sec: 0,
            max_retries: 5,
            connect_timeout: Duration::from_secs(15),
            read_timeout: Duration::from_secs(30),
            extra_headers: HashMap::new(),
            user_agent: "ODM/0.1 (+https://github.com/odm)".to_string(),
            file_exist_policy: FileExistPolicy::default(),
            proxy: ProxyConfig::default(),
        }
    }
}

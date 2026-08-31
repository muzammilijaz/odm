use crate::chunk::Chunk;
use crate::error::Result;
use std::path::Path;

/// Small, serializable snapshot of an in-progress download — pure data,
/// decoupled from live streams/sockets, so it can be persisted to disk (or a
/// DB row later) and used to resume.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadState {
    pub url: String,
    pub total_size: u64,
    pub supports_range: bool,
    pub chunks: Vec<Chunk>,
}

impl DownloadState {
    pub fn downloaded_bytes(&self) -> u64 {
        self.chunks.iter().map(|c| c.position).sum()
    }

    pub fn is_complete(&self) -> bool {
        self.chunks.iter().all(|c| c.is_complete())
    }

    pub async fn save(&self, state_path: &Path) -> Result<()> {
        let json = serde_json::to_vec_pretty(self)?;
        tokio::fs::write(state_path, json).await?;
        Ok(())
    }

    pub async fn load(state_path: &Path) -> Result<Option<Self>> {
        match tokio::fs::read(state_path).await {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn remove(state_path: &Path) -> Result<()> {
        match tokio::fs::remove_file(state_path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// `<dest>.part` while downloading; `<dest>.part.odm-state.json` for resume metadata.
pub fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".part");
    p.into()
}

pub fn state_path(dest: &Path) -> std::path::PathBuf {
    let mut p = part_path(dest).into_os_string();
    p.push(".odm-state.json");
    p.into()
}

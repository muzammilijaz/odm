#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TaskStatus {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Queued => "queued",
            TaskStatus::Downloading => "downloading",
            TaskStatus::Paused => "paused",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "downloading" => TaskStatus::Downloading,
            "paused" => TaskStatus::Paused,
            "completed" => TaskStatus::Completed,
            "failed" => TaskStatus::Failed,
            "cancelled" => TaskStatus::Cancelled,
            _ => TaskStatus::Queued,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Task {
    pub playlist_group: Option<String>,
    pub playlist_title: Option<String>,
    pub id: i64,
    pub url: String,
    pub dest_path: String,
    pub category: Option<String>,
    pub status: TaskStatus,
    pub total_bytes: Option<u64>,
    pub downloaded_bytes: u64,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub retry_count: u32,
    pub error_message: Option<String>,
    pub allow_playlist: bool,
    /// Preferred video height for yt-dlp downloads. `None` means best
    /// available quality. The engine falls back if the exact height is absent.
    pub video_quality: Option<u32>,
    /// Actual height read from the completed output file after any fallback.
    pub actual_video_quality: Option<u32>,
    /// The real video title, fetched upfront via a metadata probe for known
    /// video sites -- lets the UI show it immediately instead of the
    /// generic destination-folder name for the whole download.
    pub title: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Category {
    pub name: String,
    pub default_folder: String,
    pub extensions: Vec<String>,
}

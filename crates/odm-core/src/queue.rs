use crate::db::Db;
use crate::error::Result;
use crate::model::{Task, TaskStatus};
use rusqlite::{params, OptionalExtension, Row};

fn row_to_task(row: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        playlist_group: row.get("playlist_group")?,
        playlist_title: row.get("playlist_title")?,
        id: row.get("id")?,
        url: row.get("url")?,
        dest_path: row.get("dest_path")?,
        category: row.get("category")?,
        status: TaskStatus::from_str(&row.get::<_, String>("status")?),
        total_bytes: row.get::<_, Option<i64>>("total_bytes")?.map(|v| v as u64),
        downloaded_bytes: row.get::<_, i64>("downloaded_bytes")? as u64,
        created_at: row.get("created_at")?,
        completed_at: row.get("completed_at")?,
        retry_count: row.get::<_, i64>("retry_count")? as u32,
        error_message: row.get("error_message")?,
        allow_playlist: row.get::<_, i64>("allow_playlist")? != 0,
        video_quality: row
            .get::<_, Option<i64>>("video_quality")?
            .map(|v| v as u32),
        actual_video_quality: row
            .get::<_, Option<i64>>("actual_video_quality")?
            .map(|v| v as u32),
        title: row.get("title")?,
        thumbnail_url: row.get("thumbnail_url")?,
    })
}

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn unique_download_hash(url: &str, video_quality: Option<u32>) -> String {
    // Existing databases require this column to be unique. Make it unique per
    // insertion so the same URL and quality can intentionally be queued more
    // than once; output paths are auto-renamed by the manager/downloader.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let mut hasher = DefaultHasher::new();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (
        url,
        video_quality,
        nanos,
        SEQUENCE.fetch_add(1, Ordering::Relaxed),
    )
        .hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

impl Db {
    pub async fn set_playlist_group(&self, id: i64, group: &str, title: &str) -> Result<()> {
        let group = group.to_string();
        let title = title.to_string();
        self.with_conn(move |conn| {
            conn.execute("UPDATE downloads SET playlist_group=?1, playlist_title=?2 WHERE id=?3", params![group, title, id])?;
            Ok(())
        }).await
    }
    /// Inserts a new task. Repeated URLs are intentional new downloads.
    pub async fn enqueue(
        &self,
        url: &str,
        dest_path: &str,
        category: Option<&str>,
        allow_playlist: bool,
        video_quality: Option<u32>,
    ) -> Result<Task> {
        let url = url.to_string();
        let dest_path = dest_path.to_string();
        let category = category.map(|s| s.to_string());
        let hash = unique_download_hash(&url, video_quality);
        let created_at = now_iso();

        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO downloads (url, url_hash, dest_path, category, status, downloaded_bytes, created_at, retry_count, allow_playlist, video_quality)
                 VALUES (?1, ?2, ?3, ?4, 'queued', 0, ?5, 0, ?6, ?7)",
                params![url, hash, dest_path, category, created_at, allow_playlist as i64, video_quality.map(i64::from)],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row("SELECT * FROM downloads WHERE id = ?1", [id], row_to_task).map_err(Into::into)
        })
        .await
    }

    pub async fn get_task(&self, id: i64) -> Result<Option<Task>> {
        self.with_conn(move |conn| {
            Ok(conn
                .query_row("SELECT * FROM downloads WHERE id = ?1", [id], row_to_task)
                .optional()?)
        })
        .await
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM downloads ORDER BY id DESC")?;
            let tasks = stmt
                .query_map([], row_to_task)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(tasks)
        })
        .await
    }

    pub async fn set_status(&self, id: i64, status: TaskStatus) -> Result<()> {
        let status_str = status.as_str().to_string();
        let completed_at = matches!(status, TaskStatus::Completed).then(now_iso);
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET status = ?1, completed_at = COALESCE(?2, completed_at) WHERE id = ?3",
                params![status_str, completed_at, id],
            )?;
            Ok(())
        })
        .await
    }

    /// Corrects the stored destination path once it's actually known --
    /// needed for yt-dlp downloads, where the real filename (video title +
    /// chosen container) isn't decided until the download finishes.
    pub async fn set_dest_path(&self, id: i64, dest_path: &str) -> Result<()> {
        let dest_path = dest_path.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET dest_path = ?1 WHERE id = ?2",
                params![dest_path, id],
            )?;
            Ok(())
        })
        .await
    }

    /// Records a video's real title/thumbnail, fetched upfront via a
    /// metadata probe -- lets the UI show them for the whole download
    /// instead of only once the file lands (see `TaskManager::add_download`).
    pub async fn set_metadata(
        &self,
        id: i64,
        title: &str,
        thumbnail_url: Option<&str>,
    ) -> Result<()> {
        let title = title.to_string();
        let thumbnail_url = thumbnail_url.map(|s| s.to_string());
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET title = ?1, thumbnail_url = ?2 WHERE id = ?3",
                params![title, thumbnail_url, id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_actual_video_quality(&self, id: i64, height: Option<u32>) -> Result<()> {
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET actual_video_quality = ?1 WHERE id = ?2",
                params![height.map(i64::from), id],
            )?;
            Ok(())
        })
        .await
    }

    /// Stores the format probe's selected height only while no authoritative
    /// final-file height has been recorded. Completion always overwrites this
    /// provisional value after ffprobe inspects the downloaded file.
    pub async fn set_provisional_video_quality(&self, id: i64, height: Option<u32>) -> Result<()> {
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET actual_video_quality = ?1 WHERE id = ?2 AND actual_video_quality IS NULL",
                params![height.map(i64::from), id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn set_error(&self, id: i64, message: &str) -> Result<()> {
        let message = message.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET status = 'failed', error_message = ?1, retry_count = retry_count + 1 WHERE id = ?2",
                params![message, id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn update_progress(
        &self,
        id: i64,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    ) -> Result<()> {
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE downloads SET downloaded_bytes = ?1, total_bytes = COALESCE(?2, total_bytes) WHERE id = ?3",
                params![downloaded_bytes as i64, total_bytes.map(|v| v as i64), id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_task(&self, id: i64) -> Result<()> {
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM downloads WHERE id = ?1", [id])?;
            Ok(())
        })
        .await
    }
}

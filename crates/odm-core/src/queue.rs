use crate::db::Db;
use crate::error::Result;
use crate::model::{Task, TaskStatus};
use rusqlite::{params, OptionalExtension, Row};

fn row_to_task(row: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
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
        title: row.get("title")?,
        thumbnail_url: row.get("thumbnail_url")?,
    })
}

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn hash_url(url: &str) -> String {
    // A stable, dependency-free content hash for URL dedup — not
    // cryptographic, just needs to be collision-resistant enough for a
    // personal download queue.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

impl Db {
    /// Inserts a new task, deduped by URL. Returns the existing task instead
    /// of inserting a duplicate if this URL is already queued/downloading.
    pub async fn enqueue(&self, url: &str, dest_path: &str, category: Option<&str>, allow_playlist: bool) -> Result<Task> {
        let url = url.to_string();
        let dest_path = dest_path.to_string();
        let category = category.map(|s| s.to_string());
        let hash = hash_url(&url);
        let created_at = now_iso();

        self.with_conn(move |conn| {
            if let Some(existing) = conn
                .query_row("SELECT * FROM downloads WHERE url_hash = ?1", [&hash], row_to_task)
                .optional()?
            {
                return Ok(existing);
            }

            conn.execute(
                "INSERT INTO downloads (url, url_hash, dest_path, category, status, downloaded_bytes, created_at, retry_count, allow_playlist)
                 VALUES (?1, ?2, ?3, ?4, 'queued', 0, ?5, 0, ?6)",
                params![url, hash, dest_path, category, created_at, allow_playlist as i64],
            )?;
            let id = conn.last_insert_rowid();
            conn.query_row("SELECT * FROM downloads WHERE id = ?1", [id], row_to_task).map_err(Into::into)
        })
        .await
    }

    pub async fn get_task(&self, id: i64) -> Result<Option<Task>> {
        self.with_conn(move |conn| {
            Ok(conn.query_row("SELECT * FROM downloads WHERE id = ?1", [id], row_to_task).optional()?)
        })
        .await
    }

    pub async fn list_tasks(&self) -> Result<Vec<Task>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM downloads ORDER BY id DESC")?;
            let tasks = stmt.query_map([], row_to_task)?.collect::<rusqlite::Result<Vec<_>>>()?;
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
            conn.execute("UPDATE downloads SET dest_path = ?1 WHERE id = ?2", params![dest_path, id])?;
            Ok(())
        })
        .await
    }

    /// Records a video's real title/thumbnail, fetched upfront via a
    /// metadata probe -- lets the UI show them for the whole download
    /// instead of only once the file lands (see `TaskManager::add_download`).
    pub async fn set_metadata(&self, id: i64, title: &str, thumbnail_url: Option<&str>) -> Result<()> {
        let title = title.to_string();
        let thumbnail_url = thumbnail_url.map(|s| s.to_string());
        self.with_conn(move |conn| {
            conn.execute("UPDATE downloads SET title = ?1, thumbnail_url = ?2 WHERE id = ?3", params![title, thumbnail_url, id])?;
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

    pub async fn update_progress(&self, id: i64, downloaded_bytes: u64, total_bytes: Option<u64>) -> Result<()> {
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

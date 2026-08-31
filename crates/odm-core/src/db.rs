use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Thin async facade over a synchronous `rusqlite::Connection` — every
/// operation runs on a blocking thread via `spawn_blocking`, guarded by a
/// mutex since SQLite connections aren't `Sync`.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// Default category → extension seed data, aligned with conventional
/// download-manager defaults. Fully editable afterward via
/// `categories::add_extension`/`remove_extension`/`add_category`.
const DEFAULT_CATEGORIES: &[(&str, &str, &[&str])] = &[
    (
        "Compressed",
        "Compressed",
        &["7Z", "ACE", "ARJ", "BIN", "BZ2", "GZ", "GZIP", "ISO", "IMG", "LZH", "R0*", "R1*", "RAR", "SEA", "SIT", "SITX", "TAR", "Z", "ZIP"],
    ),
    ("Programs", "Programs", &["APK", "EXE", "MSI", "MSU"]),
    (
        "Video",
        "Video",
        &["3GP", "ASF", "AVI", "M4V", "MKV", "MOV", "MPE", "MPEG", "MPG", "OGV", "QT", "RM", "RMVB", "WMV"],
    ),
    ("Music", "Music", &["AAC", "AIF", "M4A", "MP3", "MPA", "OGG", "RA", "WAV", "WMA"]),
    ("Documents", "Documents", &["PDF", "PLJ", "PPS", "PPT", "TIF", "TIFF"]),
];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    url             TEXT NOT NULL,
    url_hash        TEXT NOT NULL UNIQUE,
    dest_path       TEXT NOT NULL,
    category        TEXT,
    status          TEXT NOT NULL DEFAULT 'queued',
    total_bytes     INTEGER,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    completed_at    TEXT,
    retry_count     INTEGER NOT NULL DEFAULT 0,
    error_message   TEXT,
    allow_playlist  INTEGER NOT NULL DEFAULT 0,
    title           TEXT,
    thumbnail_url   TEXT
);

CREATE TABLE IF NOT EXISTS settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS categories (
    name            TEXT PRIMARY KEY,
    default_folder  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS category_extensions (
    category        TEXT NOT NULL REFERENCES categories(name) ON DELETE CASCADE,
    extension       TEXT NOT NULL,
    PRIMARY KEY (category, extension)
);
"#;

impl Db {
    /// Opens (creating if needed) the SQLite database at `path` and seeds
    /// default categories on first run.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let conn = tokio::task::spawn_blocking(move || -> Result<Connection> {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            let conn = Connection::open(path)?;
            conn.execute_batch(SCHEMA)?;
            // Migrate pre-existing DBs created before these columns existed
            // -- `CREATE TABLE IF NOT EXISTS` above is a no-op against an
            // already-created table, so add them here instead. Errors are
            // ignored since they mean the column is already there.
            let _ = conn.execute("ALTER TABLE downloads ADD COLUMN title TEXT", []);
            let _ = conn.execute("ALTER TABLE downloads ADD COLUMN thumbnail_url TEXT", []);
            Ok(conn)
        })
        .await
        .map_err(|_| crate::error::CoreError::Join)??;

        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.seed_default_categories().await?;
        Ok(db)
    }

    pub async fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let db = Db { conn: Arc::new(Mutex::new(conn)) };
        db.seed_default_categories().await?;
        Ok(db)
    }

    async fn seed_default_categories(&self) -> Result<()> {
        self.with_conn(|conn| {
            let existing: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
            if existing > 0 {
                return Ok(());
            }
            for (name, folder, extensions) in DEFAULT_CATEGORIES {
                conn.execute("INSERT INTO categories (name, default_folder) VALUES (?1, ?2)", (name, folder))?;
                for ext in *extensions {
                    conn.execute(
                        "INSERT OR IGNORE INTO category_extensions (category, extension) VALUES (?1, ?2)",
                        (name, ext.to_uppercase()),
                    )?;
                }
            }
            Ok(())
        })
        .await
    }

    /// Runs a synchronous closure against the connection on a blocking
    /// thread pool. All `Db` methods elsewhere are built on top of this.
    pub async fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().expect("db mutex poisoned");
            f(&conn)
        })
        .await
        .map_err(|_| crate::error::CoreError::Join)?
    }
}

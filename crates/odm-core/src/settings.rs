use crate::db::Db;
use crate::error::Result;
use rusqlite::{params, OptionalExtension};

/// Well-known setting keys. Global (not per-download) preferences --
/// `cookiesBrowser` is an app-wide setting rather than a per-job option.
pub const COOKIES_BROWSER: &str = "cookies_browser";
/// Path to an imported `cookies.txt` (Netscape format). Takes priority over
/// `COOKIES_BROWSER` when set -- useful when the account needed isn't
/// signed into any local browser, or when reading the browser's cookie
/// store directly doesn't work (e.g. it's open and holding its own cookie
/// file locked).
pub const COOKIES_FILE: &str = "cookies_file";

impl Db {
    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_string();
        self.with_conn(move |conn| {
            Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [&key], |r| r.get::<_, String>(0)).optional()?)
        })
        .await
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = key.to_string();
        let value = value.to_string();
        self.with_conn(move |conn| {
            conn.execute("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)", params![key, value])?;
            Ok(())
        })
        .await
    }

    pub async fn clear_setting(&self, key: &str) -> Result<()> {
        let key = key.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM settings WHERE key = ?1", [&key])?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrips_a_setting() {
        let db = Db::open_in_memory().await.unwrap();
        assert_eq!(db.get_setting(COOKIES_BROWSER).await.unwrap(), None);

        db.set_setting(COOKIES_BROWSER, "chrome").await.unwrap();
        assert_eq!(db.get_setting(COOKIES_BROWSER).await.unwrap(), Some("chrome".to_string()));

        db.clear_setting(COOKIES_BROWSER).await.unwrap();
        assert_eq!(db.get_setting(COOKIES_BROWSER).await.unwrap(), None);
    }
}

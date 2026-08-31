use crate::db::Db;
use crate::error::Result;
use crate::model::Category;
use std::collections::HashMap;

impl Db {
    pub async fn list_categories(&self) -> Result<Vec<Category>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT name, default_folder FROM categories ORDER BY name")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;

            let mut categories: Vec<Category> = Vec::new();
            let mut ext_stmt = conn.prepare("SELECT extension FROM category_extensions WHERE category = ?1 ORDER BY extension")?;
            for row in rows {
                let (name, default_folder) = row?;
                let extensions: Vec<String> = ext_stmt.query_map([&name], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<_>>()?;
                categories.push(Category { name, default_folder, extensions });
            }
            Ok(categories)
        })
        .await
    }

    pub async fn add_category(&self, name: &str, default_folder: &str) -> Result<()> {
        let name = name.to_string();
        let folder = default_folder.to_string();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO categories (name, default_folder) VALUES (?1, ?2)",
                (&name, &folder),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn remove_category(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM categories WHERE name = ?1", [&name])?;
            Ok(())
        })
        .await
    }

    pub async fn add_extension(&self, category: &str, extension: &str) -> Result<()> {
        let category = category.to_string();
        let extension = extension.trim_start_matches('.').to_uppercase();
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO category_extensions (category, extension) VALUES (?1, ?2)",
                (&category, &extension),
            )?;
            Ok(())
        })
        .await
    }

    pub async fn remove_extension(&self, category: &str, extension: &str) -> Result<()> {
        let category = category.to_string();
        let extension = extension.trim_start_matches('.').to_uppercase();
        self.with_conn(move |conn| {
            conn.execute(
                "DELETE FROM category_extensions WHERE category = ?1 AND extension = ?2",
                (&category, &extension),
            )?;
            Ok(())
        })
        .await
    }

    /// Given a filename (or full URL), resolves which category it belongs to
    /// by extension, if any — case-insensitive, matches the user's editable
    /// category/extension table rather than any hardcoded list.
    pub async fn resolve_category(&self, filename_or_url: &str) -> Result<Option<Category>> {
        let ext = extract_extension(filename_or_url);
        let Some(ext) = ext else { return Ok(None) };

        let categories = self.list_categories().await?;
        let lookup: HashMap<String, &Category> = categories
            .iter()
            .flat_map(|c| c.extensions.iter().map(move |e| (e.clone(), c)))
            .collect();

        Ok(lookup.get(&ext).map(|c| (*c).clone()))
    }
}

fn extract_extension(filename_or_url: &str) -> Option<String> {
    let without_query = filename_or_url.split(['?', '#']).next().unwrap_or(filename_or_url);
    let last_segment = without_query.rsplit('/').next().unwrap_or(without_query);
    let ext = last_segment.rsplit_once('.').map(|(_, ext)| ext)?;
    if ext.is_empty() || ext.len() > 8 {
        return None;
    }
    Some(ext.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_extension_from_plain_filename() {
        assert_eq!(extract_extension("movie.mkv"), Some("MKV".to_string()));
    }

    #[test]
    fn extracts_extension_from_url_with_query() {
        assert_eq!(extract_extension("https://cdn.example.com/path/archive.zip?token=abc&x=1"), Some("ZIP".to_string()));
    }

    #[test]
    fn returns_none_without_extension() {
        assert_eq!(extract_extension("https://example.com/download"), None);
    }
}

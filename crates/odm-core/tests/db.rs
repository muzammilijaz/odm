use odm_core::{Db, TaskStatus};

#[tokio::test]
async fn migrates_existing_downloads_with_video_quality_columns() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let conn = rusqlite::Connection::open(file.path()).unwrap();
    conn.execute_batch(
        "CREATE TABLE downloads (
            id INTEGER PRIMARY KEY AUTOINCREMENT, url TEXT NOT NULL, url_hash TEXT NOT NULL UNIQUE,
            dest_path TEXT NOT NULL, category TEXT, status TEXT NOT NULL DEFAULT 'queued', total_bytes INTEGER,
            downloaded_bytes INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL, completed_at TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0, error_message TEXT, allow_playlist INTEGER NOT NULL DEFAULT 0,
            title TEXT, thumbnail_url TEXT
        );",
    )
    .unwrap();
    drop(conn);

    let db = Db::open(file.path()).await.unwrap();
    let task = db
        .enqueue(
            "https://youtube.com/watch?v=migrate",
            "/tmp/video",
            Some("Video"),
            false,
            Some(720),
        )
        .await
        .unwrap();
    assert_eq!(task.video_quality, Some(720));
    assert_eq!(task.actual_video_quality, None);
}

#[tokio::test]
async fn seeds_default_categories_on_first_open() {
    let db = Db::open_in_memory().await.unwrap();
    let categories = db.list_categories().await.unwrap();
    let names: Vec<&str> = categories.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"Video"));
    assert!(names.contains(&"Music"));
    assert!(names.contains(&"Compressed"));

    let video = categories.iter().find(|c| c.name == "Video").unwrap();
    assert!(video.extensions.contains(&"MKV".to_string()));
    assert!(video.extensions.contains(&"AVI".to_string()));
}

#[tokio::test]
async fn resolves_category_by_extension_case_insensitively() {
    let db = Db::open_in_memory().await.unwrap();
    let category = db
        .resolve_category("https://example.com/video/Movie.MKV?x=1")
        .await
        .unwrap();
    assert_eq!(category.unwrap().name, "Video");

    let none = db
        .resolve_category("https://example.com/page")
        .await
        .unwrap();
    assert!(none.is_none());
}

#[tokio::test]
async fn extensions_are_user_editable() {
    let db = Db::open_in_memory().await.unwrap();
    db.add_extension("Video", "webm").await.unwrap();
    let category = db.resolve_category("clip.webm").await.unwrap().unwrap();
    assert_eq!(category.name, "Video");

    db.remove_extension("Video", "webm").await.unwrap();
    let category = db.resolve_category("clip.webm").await.unwrap();
    assert!(category.is_none());
}

#[tokio::test]
async fn new_categories_can_be_added() {
    let db = Db::open_in_memory().await.unwrap();
    db.add_category("Ebooks", "Ebooks").await.unwrap();
    db.add_extension("Ebooks", "epub").await.unwrap();

    let category = db.resolve_category("book.epub").await.unwrap().unwrap();
    assert_eq!(category.name, "Ebooks");
    assert_eq!(category.default_folder, "Ebooks");
}

#[tokio::test]
async fn enqueue_allows_the_same_url_more_than_once() {
    let db = Db::open_in_memory().await.unwrap();
    let a = db
        .enqueue(
            "https://example.com/f.zip",
            "/tmp/f.zip",
            Some("Compressed"),
            false,
            None,
        )
        .await
        .unwrap();
    let b = db
        .enqueue(
            "https://example.com/f.zip",
            "/tmp/f.zip",
            Some("Compressed"),
            false,
            None,
        )
        .await
        .unwrap();
    assert_ne!(a.id, b.id);

    let all = db.list_tasks().await.unwrap();
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn same_video_can_be_queued_at_different_qualities() {
    let db = Db::open_in_memory().await.unwrap();
    let hd = db
        .enqueue(
            "https://youtube.com/watch?v=test",
            "/tmp/video",
            Some("Video"),
            false,
            Some(720),
        )
        .await
        .unwrap();
    let full_hd = db
        .enqueue(
            "https://youtube.com/watch?v=test",
            "/tmp/video",
            Some("Video"),
            false,
            Some(1080),
        )
        .await
        .unwrap();
    assert_ne!(hd.id, full_hd.id);
    assert_eq!(hd.video_quality, Some(720));
    assert_eq!(full_hd.video_quality, Some(1080));
}

#[tokio::test]
async fn final_video_quality_overrides_but_is_not_replaced_by_probe() {
    let db = Db::open_in_memory().await.unwrap();
    let task = db
        .enqueue(
            "https://youtube.com/watch?v=quality",
            "/tmp/video",
            Some("Video"),
            false,
            None,
        )
        .await
        .unwrap();

    db.set_provisional_video_quality(task.id, Some(2160))
        .await
        .unwrap();
    assert_eq!(
        db.get_task(task.id)
            .await
            .unwrap()
            .unwrap()
            .actual_video_quality,
        Some(2160)
    );

    db.set_actual_video_quality(task.id, Some(1080))
        .await
        .unwrap();
    db.set_provisional_video_quality(task.id, Some(2160))
        .await
        .unwrap();
    assert_eq!(
        db.get_task(task.id)
            .await
            .unwrap()
            .unwrap()
            .actual_video_quality,
        Some(1080)
    );
}

#[tokio::test]
async fn status_and_progress_updates_persist() {
    let db = Db::open_in_memory().await.unwrap();
    let task = db
        .enqueue("https://example.com/f.zip", "/tmp/f.zip", None, false, None)
        .await
        .unwrap();

    db.set_status(task.id, TaskStatus::Downloading)
        .await
        .unwrap();
    db.update_progress(task.id, 500, Some(1000)).await.unwrap();

    let updated = db.get_task(task.id).await.unwrap().unwrap();
    assert_eq!(updated.status, TaskStatus::Downloading);
    assert_eq!(updated.downloaded_bytes, 500);
    assert_eq!(updated.total_bytes, Some(1000));

    db.set_status(task.id, TaskStatus::Completed).await.unwrap();
    let done = db.get_task(task.id).await.unwrap().unwrap();
    assert_eq!(done.status, TaskStatus::Completed);
    assert!(done.completed_at.is_some());
}

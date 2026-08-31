use odm_core::{Db, TaskStatus};

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
    let category = db.resolve_category("https://example.com/video/Movie.MKV?x=1").await.unwrap();
    assert_eq!(category.unwrap().name, "Video");

    let none = db.resolve_category("https://example.com/page").await.unwrap();
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
async fn enqueue_dedupes_by_url() {
    let db = Db::open_in_memory().await.unwrap();
    let a = db.enqueue("https://example.com/f.zip", "/tmp/f.zip", Some("Compressed"), false).await.unwrap();
    let b = db.enqueue("https://example.com/f.zip", "/tmp/f.zip", Some("Compressed"), false).await.unwrap();
    assert_eq!(a.id, b.id);

    let all = db.list_tasks().await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn status_and_progress_updates_persist() {
    let db = Db::open_in_memory().await.unwrap();
    let task = db.enqueue("https://example.com/f.zip", "/tmp/f.zip", None, false).await.unwrap();

    db.set_status(task.id, TaskStatus::Downloading).await.unwrap();
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

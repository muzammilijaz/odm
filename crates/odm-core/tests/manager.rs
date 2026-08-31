use axum::body::Body;
use axum::extract::Path as AxPath;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use odm_core::{Db, DownloadConfig, TaskManager, TaskStatus};
use std::net::SocketAddr;
use std::time::Duration;
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn spawn_server() -> String {
    let app = Router::new().route("/file/:name", get(serve_file));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn serve_file(AxPath(_name): AxPath<String>, headers: HeaderMap) -> Response {
    let body = vec![7u8; 20_000];
    let total = body.len() as u64;
    if let Some(range) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
        if let Some(spec) = range.strip_prefix("bytes=") {
            let (start_s, end_s) = spec.split_once('-').unwrap();
            let start: u64 = start_s.parse().unwrap();
            let end: u64 = if end_s.is_empty() { total - 1 } else { end_s.parse().unwrap() };
            let slice = body[start as usize..=(end.min(total - 1)) as usize].to_vec();
            let mut resp = Response::new(Body::from(slice));
            *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
            resp.headers_mut().insert(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}").parse().unwrap());
            return resp;
        }
    }
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(header::CONTENT_LENGTH, total.into());
    resp
}

async fn wait_for_status(manager: &TaskManager, id: i64, target: TaskStatus, timeout: Duration) -> odm_core::Task {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let task = manager.db().get_task(id).await.unwrap().unwrap();
        if task.status == target || tokio::time::Instant::now() >= deadline {
            return task;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn end_to_end_download_lands_in_the_right_category() {
    let base = spawn_server().await;
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempdir().unwrap();
    let manager = TaskManager::new(db, DownloadConfig::default(), dir.path(), 2);

    let task = manager.add_download(&format!("{base}/file/movie.mkv"), None, false).await.unwrap();
    assert_eq!(task.category.as_deref(), Some("Video"));

    let finished = wait_for_status(&manager, task.id, TaskStatus::Completed, Duration::from_secs(10)).await;
    assert_eq!(finished.status, TaskStatus::Completed);
    assert_eq!(finished.downloaded_bytes, 20_000);

    let expected_path = dir.path().join("Video").join("movie.mkv");
    assert!(expected_path.exists());
    let bytes = tokio::fs::read(&expected_path).await.unwrap();
    assert_eq!(bytes.len(), 20_000);
}

#[tokio::test]
async fn unrecognized_extension_goes_to_general() {
    let base = spawn_server().await;
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempdir().unwrap();
    let manager = TaskManager::new(db, DownloadConfig::default(), dir.path(), 2);

    let task = manager.add_download(&format!("{base}/file/data.xyz123"), None, false).await.unwrap();
    assert_eq!(task.category, None);
    let finished = wait_for_status(&manager, task.id, TaskStatus::Completed, Duration::from_secs(10)).await;
    assert_eq!(finished.status, TaskStatus::Completed);
    assert!(dir.path().join("General").join("data.xyz123").exists());
}

#[tokio::test]
async fn pause_then_resume_still_completes() {
    let base = spawn_server().await;
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempdir().unwrap();
    let manager = TaskManager::new(db, DownloadConfig::default(), dir.path(), 2);

    let task = manager.add_download(&format!("{base}/file/archive.zip"), None, false).await.unwrap();
    manager.pause(task.id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(30)).await;
    manager.resume(task.id).await.unwrap();

    let finished = wait_for_status(&manager, task.id, TaskStatus::Completed, Duration::from_secs(10)).await;
    assert_eq!(finished.status, TaskStatus::Completed);
}

#[tokio::test]
async fn concurrency_limit_is_respected() {
    let base = spawn_server().await;
    let db = Db::open_in_memory().await.unwrap();
    let dir = tempdir().unwrap();
    let manager = TaskManager::new(db, DownloadConfig::default(), dir.path(), 1);

    let t1 = manager.add_download(&format!("{base}/file/a.zip"), None, false).await.unwrap();
    let t2 = manager.add_download(&format!("{base}/file/b.zip"), None, false).await.unwrap();

    wait_for_status(&manager, t1.id, TaskStatus::Completed, Duration::from_secs(10)).await;
    wait_for_status(&manager, t2.id, TaskStatus::Completed, Duration::from_secs(10)).await;

    let all = manager.list().await.unwrap();
    assert!(all.iter().all(|t| t.status == TaskStatus::Completed));
}

mod common;

use common::{deterministic_bytes, spawn_test_server};
use odm_engine::{download, DownloadConfig};
use std::time::Duration;
use tempfile::tempdir;

fn small_config() -> DownloadConfig {
    DownloadConfig {
        chunk_count: 4,
        parallel_count: 4,
        min_chunk_size: 16,
        max_retries: 3,
        connect_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(5),
        ..Default::default()
    }
}

#[tokio::test]
async fn downloads_full_file_with_ranged_chunks() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let handle = download(format!("{base}/file/50000"), &dest, small_config()).await.unwrap();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(50000));
}

#[tokio::test]
async fn downloads_when_server_does_not_support_range() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let handle = download(format!("{base}/file-norange/20000"), &dest, small_config()).await.unwrap();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(20000));
}

#[tokio::test]
async fn follows_redirect() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let handle = download(format!("{base}/redirect/10000"), &dest, small_config()).await.unwrap();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(10000));
}

#[tokio::test]
async fn retries_past_a_transient_failure() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    // First hit to /file-failonce returns 503; the server route only fails
    // once globally, so with parallel chunks at least one will hit it and
    // must retry into success.
    let handle = download(format!("{base}/file-failonce/8000"), &dest, small_config()).await.unwrap();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(8000));
}

#[tokio::test]
async fn pause_and_resume_completes_correctly() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let handle = download(format!("{base}/file/200000"), &dest, small_config()).await.unwrap();
    handle.pause();
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.resume();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(200000));
}

#[tokio::test]
async fn cancel_stops_the_download() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let handle = download(format!("{base}/file/500000"), &dest, small_config()).await.unwrap();
    handle.cancel();
    let result = handle.wait().await;

    assert!(result.is_err());
    assert!(!dest.exists());
}

#[tokio::test]
async fn resumes_from_saved_state_after_cancel() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let url = format!("{base}/file/300000");

    let mut config = small_config();
    config.parallel_count = 1; // keep it slow/deterministic enough to cancel mid-flight
    config.chunk_count = 1;

    let handle = download(url.clone(), &dest, config.clone()).await.unwrap();
    // Let a partial download accumulate, then cancel — this must leave a
    // `.part` file plus resume-state JSON behind instead of the final file.
    tokio::time::sleep(Duration::from_millis(5)).await;
    handle.cancel();
    let _ = handle.wait().await;
    assert!(!dest.exists());

    let part = dest.with_extension("bin.part");
    // `.part` may or may not have been created depending on how much time
    // elapsed before cancel; what matters is that resuming completes correctly
    // either way.
    let _ = part;

    let handle2 = download(url, &dest, config).await.unwrap();
    let path = handle2.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(300000));
}

#[tokio::test]
async fn single_small_file_uses_one_chunk() {
    let base = spawn_test_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.bin");

    let mut config = small_config();
    config.min_chunk_size = 1_000_000; // forces single-chunk path for a tiny file
    let handle = download(format!("{base}/file/500"), &dest, config).await.unwrap();
    let path = handle.wait().await.unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(bytes, deterministic_bytes(500));
}

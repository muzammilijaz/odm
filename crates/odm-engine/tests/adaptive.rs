use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use odm_engine::{detect_stream_kind, download_adaptive, ffmpeg, StreamKind};
use std::net::SocketAddr;
use tempfile::tempdir;
use tokio::net::TcpListener;

async fn ffmpeg_available() -> bool {
    tokio::process::Command::new(ffmpeg::resolve_ffmpeg_path())
        .arg("-version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn spawn_hls_server() -> String {
    let playlist = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXTINF:1.0,\nseg0.ts\n#EXTINF:1.0,\nseg1.ts\n#EXT-X-ENDLIST\n";
    let app = Router::new()
        .route(
            "/stream.m3u8",
            get(move || {
                let body = playlist.to_string();
                async move {
                    let mut resp = Response::new(Body::from(body));
                    resp.headers_mut().insert(
                        header::CONTENT_TYPE,
                        "application/vnd.apple.mpegurl".parse().unwrap(),
                    );
                    resp
                }
            }),
        )
        .route(
            "/seg0.ts",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "video/mp2t")],
                    vec![0u8; 188],
                )
            }),
        )
        .route(
            "/seg1.ts",
            get(|| async {
                (
                    StatusCode::OK,
                    [(header::CONTENT_TYPE, "video/mp2t")],
                    vec![1u8; 188],
                )
            }),
        );

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn detects_hls_by_extension() {
    let client = reqwest::Client::new();
    let kind = detect_stream_kind(&client, "https://example.com/video/stream.m3u8")
        .await
        .unwrap();
    assert_eq!(kind, Some(StreamKind::Hls));
}

#[tokio::test]
async fn detects_dash_by_extension() {
    let client = reqwest::Client::new();
    let kind = detect_stream_kind(&client, "https://example.com/video/manifest.mpd")
        .await
        .unwrap();
    assert_eq!(kind, Some(StreamKind::Dash));
}

#[tokio::test]
async fn downloads_and_remuxes_a_simple_hls_stream() {
    if !ffmpeg_available().await {
        eprintln!("skipping: ffmpeg not found on PATH (set ODM_FFMPEG_PATH or install ffmpeg)");
        return;
    }

    let base = spawn_hls_server().await;
    let dir = tempdir().unwrap();
    let dest = dir.path().join("out.ts");
    let client = reqwest::Client::new();

    let path = download_adaptive(
        &client,
        &format!("{base}/stream.m3u8"),
        &dest,
        StreamKind::Hls,
    )
    .await
    .unwrap();
    let meta = tokio::fs::metadata(&path).await.unwrap();
    assert!(meta.len() > 0, "remuxed output should be non-empty");
}

use axum::body::Body;
use axum::extract::Path as AxPath;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;

pub fn deterministic_bytes(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 251) as u8).collect()
}

/// Spins up a local HTTP server exposing a handful of endpoints that exercise
/// range support, redirects, truncation, and mid-stream failure.
pub async fn spawn_test_server() -> String {
    let fail_once_hit = Arc::new(AtomicUsize::new(0));

    let app = Router::new()
        .route("/file/:size", get(serve_file))
        .route("/file-norange/:size", get(serve_file_no_range))
        .route(
            "/file-failonce/:size",
            get({
                let hits = fail_once_hit.clone();
                move |path, headers| serve_file_fail_once(path, headers, hits)
            }),
        )
        .route("/redirect/:size", get(serve_redirect));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn serve_file(AxPath(size): AxPath<usize>, headers: HeaderMap) -> Response {
    respond_with_range(deterministic_bytes(size), &headers, true)
}

async fn serve_file_no_range(AxPath(size): AxPath<usize>, _headers: HeaderMap) -> Response {
    let body = deterministic_bytes(size);
    let mut resp = Response::new(Body::from(body.clone()));
    resp.headers_mut().insert(header::CONTENT_LENGTH, body.len().into());
    resp
}

async fn serve_file_fail_once(AxPath(size): AxPath<usize>, headers: HeaderMap, hits: Arc<AtomicUsize>) -> Response {
    // Hit 0 is the engine's own `Range: 0-0` probe request — let it succeed so
    // the download is scheduled at all. Fail hit 1 (the first real chunk
    // fetch) once, to exercise the per-chunk retry path.
    if hits.fetch_add(1, Ordering::SeqCst) == 1 {
        return (StatusCode::SERVICE_UNAVAILABLE, "try again").into_response();
    }
    respond_with_range(deterministic_bytes(size), &headers, true)
}

async fn serve_redirect(AxPath(size): AxPath<usize>) -> Response {
    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, format!("/file/{size}"))
        .body(Body::empty())
        .unwrap()
}

fn respond_with_range(body: Vec<u8>, headers: &HeaderMap, supports_range: bool) -> Response {
    let total = body.len() as u64;
    if supports_range {
        if let Some(range) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) {
            if let Some((start, end)) = parse_range(range, total) {
                let slice = body[start as usize..=end as usize].to_vec();
                let mut resp = Response::new(Body::from(slice));
                *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
                resp.headers_mut().insert(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{end}/{total}").parse().unwrap(),
                );
                resp.headers_mut().insert(header::CONTENT_LENGTH, (end - start + 1).into());
                return resp;
            }
        }
    }
    let mut resp = Response::new(Body::from(body));
    resp.headers_mut().insert(header::CONTENT_LENGTH, total.into());
    resp
}

fn parse_range(range: &str, total: u64) -> Option<(u64, u64)> {
    let spec = range.strip_prefix("bytes=")?;
    let (start_s, end_s) = spec.split_once('-')?;
    let start: u64 = start_s.parse().ok()?;
    let end: u64 = if end_s.is_empty() { total - 1 } else { end_s.parse().ok()? };
    Some((start, end.min(total - 1)))
}

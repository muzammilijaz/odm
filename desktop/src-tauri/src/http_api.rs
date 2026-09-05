//! Local loopback HTTP API (127.0.0.1 only) — the browser-extension
//! native-messaging host forwards to this instead of talking to the download
//! engine directly, giving the extension a normal REST surface rather than a
//! bespoke IPC protocol.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use odm_core::TaskManager;
use serde::Deserialize;
use tower_http::cors::{Any, CorsLayer};

/// Bound to a fixed loopback port; only `127.0.0.1` connections are accepted
/// (the listener is bound to that address specifically, not `0.0.0.0`), so
/// this is reachable only from the same machine — the browser extension's
/// native-messaging host and nothing else.
pub const DEFAULT_PORT: u16 = 38019;

pub async fn serve(manager: TaskManager, port: u16) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/api/downloads", get(list_downloads).post(add_download))
        .route("/api/video-qualities", post(probe_video_qualities))
        .route("/api/downloads/:id/pause", post(pause_download))
        .route("/api/downloads/:id/resume", post(resume_download))
        .route("/api/downloads/:id/cancel", post(cancel_download))
        .route("/api/categories", get(list_categories))
        .layer(cors)
        .with_state(manager);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            let _ = axum::serve(listener, app).await;
        }
        Err(e) => {
            eprintln!("ODM local API failed to bind {addr}: {e}");
        }
    }
}

#[derive(Deserialize)]
struct AddDownloadBody {
    url: String,
    filename: Option<String>,
    #[serde(default)]
    playlist: bool,
    quality: Option<serde_json::Value>,
    fallback_url: Option<String>,
    fallback_audio: Option<String>,
}

async fn add_download(
    State(manager): State<TaskManager>,
    Json(body): Json<AddDownloadBody>,
) -> impl IntoResponse {
    let quality = body.quality.as_ref().and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    });
    match manager
        .add_download_with_fallback(
            &body.url,
            body.filename.as_deref(),
            body.playlist,
            quality.as_deref(),
            body.fallback_url.as_deref(),
            body.fallback_audio.as_deref(),
        )
        .await
    {
        Ok(task) => (StatusCode::OK, Json(task)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct ProbeVideoBody {
    url: String,
}

async fn probe_video_qualities(
    State(manager): State<TaskManager>,
    Json(body): Json<ProbeVideoBody>,
) -> impl IntoResponse {
    match manager.probe_video_qualities(&body.url).await {
        Ok(qualities) => (StatusCode::OK, Json(qualities)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn list_downloads(State(manager): State<TaskManager>) -> impl IntoResponse {
    match manager.list().await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn pause_download(
    State(manager): State<TaskManager>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    respond(manager.pause(id).await)
}

async fn resume_download(
    State(manager): State<TaskManager>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    respond(manager.resume(id).await)
}

async fn cancel_download(
    State(manager): State<TaskManager>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    respond(manager.cancel(id).await)
}

async fn list_categories(State(manager): State<TaskManager>) -> impl IntoResponse {
    match manager.db().list_categories().await {
        Ok(categories) => Json(categories).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn respond(result: odm_core::Result<()>) -> impl IntoResponse {
    match result {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

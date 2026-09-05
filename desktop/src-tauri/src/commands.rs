use crate::state::AppState;
use odm_core::{Category, Task};
use tauri::{AppHandle, Manager, State};

type CmdResult<T> = Result<T, String>;

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> CmdResult<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main ODM window not found".to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.unminimize().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn add_download(
    state: State<'_, AppState>,
    url: String,
    filename: Option<String>,
    playlist: Option<bool>,
    quality: Option<String>,
) -> CmdResult<Task> {
    state
        .manager
        .add_download(
            &url,
            filename.as_deref(),
            playlist.unwrap_or(false),
            quality.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_downloads(state: State<'_, AppState>) -> CmdResult<Vec<Task>> {
    state.manager.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.manager.pause(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn resume_download(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.manager.resume(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    state.manager.cancel(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_download(
    state: State<'_, AppState>,
    id: i64,
    delete_file: bool,
) -> CmdResult<()> {
    state
        .manager
        .remove(id, delete_file)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_download(
    state: State<'_, AppState>,
    id: i64,
    new_path: String,
) -> CmdResult<()> {
    state
        .manager
        .rename(id, &new_path)
        .await
        .map_err(|e| e.to_string())
}

/// Opens Windows' native "Open with" picker for a file -- there's no
/// dedicated API for this, but `rundll32 shell32.dll,OpenAs_RunDLL <path>`
/// is the standard trick every download manager (and Explorer itself) uses
/// under the hood.
#[tauri::command]
pub async fn open_with_dialog(path: String) -> CmdResult<()> {
    tokio::process::Command::new("rundll32")
        .args(["shell32.dll,OpenAs_RunDLL", &path])
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_categories(state: State<'_, AppState>) -> CmdResult<Vec<Category>> {
    state
        .manager
        .db()
        .list_categories()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_category(
    state: State<'_, AppState>,
    name: String,
    default_folder: String,
) -> CmdResult<()> {
    state
        .manager
        .db()
        .add_category(&name, &default_folder)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_category(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    state
        .manager
        .db()
        .remove_category(&name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_category_extension(
    state: State<'_, AppState>,
    category: String,
    extension: String,
) -> CmdResult<()> {
    state
        .manager
        .db()
        .add_extension(&category, &extension)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_category_extension(
    state: State<'_, AppState>,
    category: String,
    extension: String,
) -> CmdResult<()> {
    state
        .manager
        .db()
        .remove_extension(&category, &extension)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_ytdlp(state: State<'_, AppState>) -> CmdResult<String> {
    state
        .manager
        .update_ytdlp()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_setting(state: State<'_, AppState>, key: String) -> CmdResult<Option<String>> {
    state
        .manager
        .db()
        .get_setting(&key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_setting(state: State<'_, AppState>, key: String, value: String) -> CmdResult<()> {
    state
        .manager
        .db()
        .set_setting(&key, &value)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_cookies_file(state: State<'_, AppState>, path: String) -> CmdResult<String> {
    state
        .manager
        .import_cookies_file(&path)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_cookies_file(state: State<'_, AppState>) -> CmdResult<()> {
    state
        .manager
        .clear_cookies_file()
        .await
        .map_err(|e| e.to_string())
}

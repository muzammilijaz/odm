mod commands;
mod http_api;
mod state;

use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            state::set_bundled_binary_env_vars();
            let app_handle = app.handle().clone();
            tauri::async_runtime::block_on(async move {
                let db_path = state::app_data_dir().join("odm.sqlite3");
                let db = odm_core::Db::open(&db_path).await.expect("failed to open ODM database");
                let config = odm_core::DownloadConfig::default();
                let manager = odm_core::TaskManager::new(db, config, state::default_downloads_root(), 4);

                app_handle.manage(state::AppState { manager: manager.clone() });

                // Local loopback HTTP API — what the Phase 4 browser
                // extension's native-messaging host forwards requests to.
                let http_manager = manager.clone();
                tauri::async_runtime::spawn(http_api::serve(http_manager, http_api::DEFAULT_PORT));

                // Push the queue to the frontend on a short interval rather
                // than wiring a per-download event stream — simple and
                // sufficient for the current UI.
                let poll_manager = manager.clone();
                let poll_handle = app_handle.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        if let Ok(tasks) = poll_manager.list().await {
                            let _ = poll_handle.emit("downloads-updated", tasks);
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                });
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_download,
            commands::list_downloads,
            commands::pause_download,
            commands::resume_download,
            commands::cancel_download,
            commands::delete_download,
            commands::rename_download,
            commands::open_with_dialog,
            commands::list_categories,
            commands::add_category,
            commands::remove_category,
            commands::add_category_extension,
            commands::remove_category_extension,
            commands::update_ytdlp,
            commands::get_setting,
            commands::set_setting,
            commands::import_cookies_file,
            commands::clear_cookies_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

mod commands;
mod http_api;
mod state;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Passed to the exe by the Windows Registry "Run" entry the autostart
/// plugin creates, so `run()` can tell "the user double-clicked ODM" apart
/// from "Windows just booted and launched ODM in the background" and skip
/// showing the window for the latter.
const AUTOSTART_ARG: &str = "--minimized";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .setup(|app| {
            // Launch on Windows startup by default, straight into the tray
            // (see AUTOSTART_ARG handling below) -- this is meant to always
            // be on, not an opt-in setting.
            let _ = app.autolaunch().enable();

            // Keep ODM running in the background instead of fully quitting:
            // closing the window (or minimizing it) hides it and drops it to
            // the tray rather than ending the process, so in-progress
            // downloads keep running. The tray icon's "Show ODM" (or a
            // left-click on the icon) brings the window back.
            let show_item = MenuItem::with_id(app, "show", "Show ODM", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("app icon is configured in tauri.conf.json"))
                .tooltip("ODM — Open Download Manager")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        // Don't actually quit -- hide to tray so downloads
                        // in progress keep running in the background.
                        api.prevent_close();
                        let _ = win.hide();
                    }
                    WindowEvent::Resized(_) => {
                        if win.is_minimized().unwrap_or(false) {
                            let _ = win.hide();
                        }
                    }
                    _ => {}
                });

                // The window starts hidden (tauri.conf.json's "visible":
                // false) so an autostart launch never flashes it on screen --
                // only reveal it here for a normal, user-initiated launch.
                let launched_minimized = std::env::args().any(|a| a == AUTOSTART_ARG);
                if !launched_minimized {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            let app_handle = app.handle().clone();
            state::set_bundled_binary_env_vars(&app_handle);
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

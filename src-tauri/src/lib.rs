pub mod commands;
pub mod error;
pub mod game;
pub mod install;
pub mod logging;
pub mod manifest;
pub mod net;
pub mod remove;
pub mod secret;
pub mod service;
pub mod settings;
pub mod sources;
pub mod toc;
pub mod util;

use std::sync::Arc;
use std::time::Duration;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let paths = app.path();
            let data_dir = paths.app_data_dir()?;
            let local = paths.app_local_data_dir()?;
            let log_dir = local.join("logs");
            let cache_dir = local.join("cache");

            let verbose = settings::load(&data_dir).verbose_logging;
            let log = logging::init(&log_dir, verbose);
            let core = Core::new(data_dir, cache_dir, log_dir, log);
            app.manage(Arc::new(core));

            // The window starts hidden so there's no white flash; the UI shows it once
            // it has painted. This is a fallback in case the UI fails to load.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(4));
                if let Some(w) = handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        tracing::warn!("UI didn't show the window in time; showing it anyway");
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::detect_game,
            commands::set_game_root,
            commands::update_settings,
            commands::save_cf_key,
            commands::clear_cf_key,
            commands::list_installed,
            commands::search,
            commands::install,
            commands::import_zip,
            commands::check_updates,
            commands::delete_preview,
            commands::remove_addon,
            commands::undo_remove,
            commands::set_enabled,
            commands::set_pinned,
            commands::launch_game,
            commands::open_folder,
            commands::export_diagnostics,
            commands::log_from_ui,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Kobold Client");
}

use crate::service::Core;

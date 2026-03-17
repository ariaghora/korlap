mod commands;
mod state;

use state::AppState;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&data_dir)
                .map_err(|e| format!("Failed to create app data dir: {}", e))?;

            let mut app_state = AppState {
                repos: HashMap::new(),
                workspaces: HashMap::new(),
                agents: HashMap::new(),
                session_ids: HashMap::new(),
                data_dir,
            };

            if let Err(e) = app_state.load() {
                tracing::warn!("Failed to load persisted state: {}", e);
            }

            app.manage(Mutex::new(app_state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_repo,
            commands::remove_repo,
            commands::list_repos,
            commands::create_workspace,
            commands::archive_workspace,
            commands::list_workspaces,
            commands::send_message,
            commands::stop_agent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

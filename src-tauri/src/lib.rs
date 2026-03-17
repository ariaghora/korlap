mod commands;
mod state;
#[cfg(target_os = "macos")]
mod traffic;

use state::AppState;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
fn macos_major_version() -> Option<u32> {
    let output = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8_lossy(&output.stdout);
    version.trim().split('.').next()?.parse().ok()
}

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

            #[cfg(target_os = "macos")]
            {
                let main_window = app.get_webview_window("main").expect("main window");
                let y: f64 = if macos_major_version().unwrap_or(0) >= 26 {
                    22.0
                } else {
                    18.0
                };
                traffic::setup_traffic_light_positioner(main_window, 8.0, y);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_repo,
            commands::remove_repo,
            commands::list_repos,
            commands::create_workspace,
            commands::archive_workspace,
            commands::list_workspaces,
            commands::rename_branch,
            commands::get_changed_files,
            commands::get_diff,
            commands::run_script,
            commands::save_messages,
            commands::load_messages,
            commands::send_message,
            commands::stop_agent,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

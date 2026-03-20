mod commands;
mod mcp_api;
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
                repo_settings: HashMap::new(),
                data_dir,
                mcp_api_port: 0,
                terminals: HashMap::new(),
            };

            if let Err(e) = app_state.load() {
                tracing::warn!("Failed to load persisted state: {}", e);
            }

            let state = std::sync::Arc::new(Mutex::new(app_state));
            let port = mcp_api::start_api(app.handle().clone(), state.clone());
            state.lock().unwrap().mcp_api_port = port;

            // Tauri commands use State<'_, Arc<Mutex<AppState>>>
            app.manage(state);

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
            commands::remove_workspace,
            commands::list_workspaces,
            commands::rename_branch,
            commands::list_directory,
            commands::read_file,
            commands::write_file,
            commands::get_changed_files,
            commands::get_diff,
            commands::search_workspace_files,
            commands::search_repo_files,
            commands::grep_workspace,
            commands::grep_repo,
            commands::read_workspace_file,
            commands::read_repo_file,
            commands::run_script,
            commands::save_messages,
            commands::load_messages,
            commands::save_image,
            commands::send_message,
            commands::stop_agent,
            commands::open_terminal,
            commands::write_terminal,
            commands::resize_terminal,
            commands::close_terminal,
            commands::list_gh_profiles,
            commands::set_repo_profile,
            commands::get_repo_head,
            commands::checkout_default_branch,
            commands::git_commit,
            commands::git_push,
            commands::sync_main,
            commands::gh_pr_merge,
            commands::generate_commit_message,
            commands::get_pr_status,
            commands::get_pr_template,
            commands::get_repo_settings,
            commands::save_repo_settings,
            commands::save_todos,
            commands::load_todos,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

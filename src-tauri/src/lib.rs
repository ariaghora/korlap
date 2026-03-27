mod commands;
mod lsp;
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
                context_meta: HashMap::new(),
                context_agents: HashMap::new(),
                script_pids: HashMap::new(),
            };

            if let Err(e) = app_state.load() {
                tracing::warn!("Failed to load persisted state: {}", e);
            }

            let state = std::sync::Arc::new(Mutex::new(app_state));
            let lsp_manager = std::sync::Arc::new(Mutex::new(lsp::server::LspServerPool::new()));
            let port = mcp_api::start_api(
                app.handle().clone(),
                state.clone(),
                lsp_manager.clone(),
            );
            state.lock().unwrap().mcp_api_port = port;

            // Tauri commands use State<'_, Arc<Mutex<AppState>>>
            app.manage(state);
            app.manage(lsp_manager);

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
            // repo
            commands::repo::add_repo,
            commands::repo::remove_repo,
            commands::repo::list_repos,
            // workspace
            commands::workspace::create_workspace,
            commands::workspace::remove_workspace,
            commands::workspace::list_workspaces,
            commands::workspace::rename_branch,
            // files
            commands::files::list_directory,
            commands::files::read_file,
            commands::files::write_file,
            commands::files::search_workspace_files,
            commands::files::search_repo_files,
            commands::files::grep_workspace,
            commands::files::grep_repo,
            commands::files::read_workspace_file,
            commands::files::read_repo_file,
            commands::files::list_repo_directory,
            commands::files::write_repo_file,
            // git
            commands::git::get_changed_files,
            commands::git::get_diff,
            commands::git::get_repo_head,
            commands::git::checkout_default_branch,
            commands::git::git_commit,
            commands::git::git_push,
            commands::git::check_main_behind,
            commands::git::sync_main,
            commands::git::check_base_updates,
            commands::git::update_from_base,
            // github
            commands::github::list_gh_profiles,
            commands::github::set_repo_profile,
            commands::github::check_gh_cli,
            commands::github::gh_auth_login,
            commands::github::cancel_gh_auth_login,
            commands::github::list_gh_repos,
            commands::github::clone_repo,
            commands::github::create_gh_repo,
            commands::github::check_repo_gh_access,
            commands::github::get_pr_status,
            commands::github::get_pr_template,
            commands::github::gh_pr_merge,
            // agent
            commands::agent::list_models,
            commands::agent::send_message,
            commands::agent::stop_agent,
            commands::agent::generate_commit_message,
            commands::agent::suggest_replies,
            commands::agent::prioritize_todos,
            commands::agent::determine_dependencies,
            commands::agent::interpret_autopilot_command,
            // scripts
            commands::scripts::run_script,
            commands::scripts::stop_script,
            commands::scripts::run_repo_script,
            commands::scripts::stop_repo_script,
            // terminal
            commands::terminal::open_terminal,
            commands::terminal::write_terminal,
            commands::terminal::resize_terminal,
            commands::terminal::close_terminal,
            commands::terminal::open_repo_terminal,
            commands::terminal::write_repo_terminal,
            commands::terminal::resize_repo_terminal,
            commands::terminal::close_repo_terminal,
            // persistence
            commands::persistence::save_messages,
            commands::persistence::load_messages,
            commands::persistence::save_image,
            commands::persistence::get_repo_settings,
            commands::persistence::save_repo_settings,
            commands::persistence::save_todos,
            commands::persistence::load_todos,
            commands::persistence::test_mcp_server,
            // oauth
            commands::oauth::mcp_oauth_start,
            // staging
            commands::staging::create_staging_workspace,
            commands::staging::remove_staging_workspace,
            // context (warm knowledge base)
            commands::context::regenerate_hot,
            commands::context::get_context_meta,
            commands::context::save_context_scope,
            commands::context::build_knowledge_base,
            commands::context::stop_context_build,
            commands::context::check_invariants,
            commands::context::update_context_after_merge,
            commands::context::update_knowledge_base_incremental,
            commands::context::read_context_file,
            commands::context::write_context_file,
            commands::context::draft_contradiction_resolution,
            commands::context::resolve_contradiction,
            // lsp
            commands::lsp::lsp_start_server,
            commands::lsp::lsp_stop_server,
            commands::lsp::lsp_restart_server,
            commands::lsp::lsp_get_status,
            commands::lsp::lsp_hover,
            commands::lsp::lsp_goto_definition,
            commands::lsp::lsp_diagnostics,
            commands::lsp::lsp_rename,
            // system
            commands::system::get_system_resources,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

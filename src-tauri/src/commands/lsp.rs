use crate::lsp;
use crate::lsp::server::LspServerPool;
use crate::lsp::types::{config_for_extension, resolve_configs, LspServerKey};
use crate::state::AppState;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

// ── Helpers ─────────────────────────────────────────────────────────

/// Resolve workspace info from AppState (quick, just reads state).
fn resolve_ws(
    state: &Arc<Mutex<AppState>>,
    workspace_id: &str,
) -> Result<(String, PathBuf, PathBuf), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let ws = st.workspaces.get(workspace_id).ok_or("workspace not found")?;
    let repo = st.repos.get(&ws.repo_id).ok_or("repo not found")?;
    Ok((ws.repo_id.clone(), repo.path.clone(), ws.worktree_path.clone()))
}

/// Non-blocking: get an already-running LSP server handle.
/// Returns None if server not started or mutex busy.
fn get_running_server(
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspServerPool>>,
    workspace_id: &str,
    file_path: &str,
) -> Result<Option<(Arc<Mutex<crate::lsp::types::LspServerHandle>>, PathBuf, PathBuf, String)>, String>
{
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (repo_id, repo_path, worktree_path) = resolve_ws(state, workspace_id)?;

    let user_overrides = state
        .lock()
        .ok()
        .and_then(|st| st.repo_settings.get(&repo_id).map(|s| s.lsp_servers.clone()))
        .unwrap_or_default();
    let configs = resolve_configs(&user_overrides);

    let (server_id, config) = match config_for_extension(&configs, ext) {
        Some(v) => v,
        None => return Ok(None),
    };

    let language_id = config.language_id.clone();
    let key = LspServerKey {
        repo_id,
        server_id: server_id.to_string(),
    };

    let mut mgr = match lsp_mgr.try_lock() {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    let handle = match mgr.get_existing(&key) {
        Some(h) => h,
        None => return Ok(None),
    };
    drop(mgr);

    // Detect project subdirectory and register the corresponding worktree path
    let project_root = lsp::detect::detect_project_root(&repo_path, config);
    let worktree_project_dir = if let Some(ref pr) = project_root {
        if let Ok(subdir) = pr.strip_prefix(&repo_path) {
            if !subdir.as_os_str().is_empty() {
                worktree_path.join(subdir)
            } else {
                worktree_path.clone()
            }
        } else {
            worktree_path.clone()
        }
    } else {
        worktree_path.clone()
    };
    let _ = lsp::add_worktree(&handle, &worktree_project_dir);

    let abs_path = worktree_path.join(file_path);
    Ok(Some((handle, worktree_path, abs_path, language_id)))
}

// ── Blocking work functions (called inside spawn_blocking) ──────────

fn do_hover(
    state: Arc<Mutex<AppState>>,
    lsp_mgr: Arc<Mutex<LspServerPool>>,
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
) -> Result<Option<LspHoverResult>, String> {
    let Some((handle, _wt, abs_path, lang_id)) =
        get_running_server(&state, &lsp_mgr, &workspace_id, &file_path)?
    else {
        return Ok(None);
    };

    let result = lsp::hover(&handle, &abs_path, line.saturating_sub(1), character.saturating_sub(1), &lang_id)
        .map_err(|e| e.to_string())?;

    match lsp::extract_hover(&result) {
        Some((kind, text)) => Ok(Some(LspHoverResult { kind, text })),
        None => Ok(None),
    }
}

fn do_goto_definition(
    state: Arc<Mutex<AppState>>,
    lsp_mgr: Arc<Mutex<LspServerPool>>,
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
) -> Result<Option<LspLocation>, String> {
    let Some((handle, wt, abs_path, lang_id)) =
        get_running_server(&state, &lsp_mgr, &workspace_id, &file_path)?
    else {
        return Ok(None);
    };

    let result = lsp::goto_definition(&handle, &abs_path, line.saturating_sub(1), character.saturating_sub(1), &lang_id)
        .map_err(|e| e.to_string())?;

    let loc: Option<serde_json::Value> = match &result {
        serde_json::Value::Array(arr) => arr.first().cloned(),
        serde_json::Value::Object(_) => Some(result),
        _ => None,
    };

    let Some(loc) = loc else { return Ok(None) };

    let uri = loc.get("uri").and_then(|u: &serde_json::Value| u.as_str()).unwrap_or("");
    let range = loc.get("range").unwrap_or(&serde_json::Value::Null);
    let start = range.get("start").unwrap_or(&serde_json::Value::Null);
    let l = start.get("line").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32;
    let c = start.get("character").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32;

    let rel_path = lsp::server::uri_to_path(uri)
        .and_then(|p| p.strip_prefix(&wt).ok().map(|r| r.to_path_buf()))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| uri.to_string());

    Ok(Some(LspLocation { file_path: rel_path, line: l + 1, character: c + 1 }))
}

fn do_diagnostics(
    state: Arc<Mutex<AppState>>,
    lsp_mgr: Arc<Mutex<LspServerPool>>,
    workspace_id: String,
    file_path: String,
) -> Result<Vec<LspDiagnostic>, String> {
    let Some((handle, _wt, abs_path, lang_id)) =
        get_running_server(&state, &lsp_mgr, &workspace_id, &file_path)?
    else {
        return Ok(vec![]);
    };

    let result = lsp::get_diagnostics(&handle, &abs_path, &lang_id)
        .map_err(|e| e.to_string())?;

    let diagnostics = match result.as_array() {
        Some(arr) => arr,
        None => return Ok(vec![]),
    };

    let mut out = Vec::new();
    for diag in diagnostics {
        let range = diag.get("range").unwrap_or(&serde_json::Value::Null);
        let start = range.get("start").unwrap_or(&serde_json::Value::Null);
        let end = range.get("end").unwrap_or(&serde_json::Value::Null);

        let severity = diag.get("severity")
            .and_then(|s: &serde_json::Value| s.as_u64())
            .map(|s| match s { 1 => "error", 2 => "warning", 3 => "info", 4 => "hint", _ => "unknown" })
            .unwrap_or("unknown");

        out.push(LspDiagnostic {
            line: start.get("line").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32 + 1,
            character: start.get("character").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32 + 1,
            end_line: end.get("line").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32 + 1,
            end_character: end.get("character").and_then(|n: &serde_json::Value| n.as_u64()).unwrap_or(0) as u32 + 1,
            severity: severity.to_string(),
            message: diag.get("message").and_then(|m: &serde_json::Value| m.as_str()).unwrap_or("").to_string(),
            source: diag.get("source").and_then(|s: &serde_json::Value| s.as_str()).unwrap_or("").to_string(),
        });
    }

    Ok(out)
}

fn do_rename(
    state: Arc<Mutex<AppState>>,
    lsp_mgr: Arc<Mutex<LspServerPool>>,
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
    new_name: String,
) -> Result<RenameResult, String> {
    let Some((handle, wt, abs_path, lang_id)) =
        get_running_server(&state, &lsp_mgr, &workspace_id, &file_path)?
    else {
        return Err("LSP server not running. Open the file browser first to start the server.".to_string());
    };

    let edit = lsp::rename(
        &handle, &abs_path,
        line.saturating_sub(1), character.saturating_sub(1),
        &new_name, &lang_id,
    ).map_err(|e| e.to_string())?;

    if edit.is_null() {
        return Ok(RenameResult { files_changed: 0, edits_applied: 0, details: vec![] });
    }

    let summary = lsp::apply_workspace_edit(&edit, &wt)
        .map_err(|e| format!("Failed to apply rename: {}", e))?;

    let files_changed = summary.len() as u32;
    let edits_applied = summary.iter().map(|(_, n)| *n as u32).sum();
    let details = summary.into_iter().map(|(path, count)| RenameFileDetail {
        file_path: path,
        edit_count: count as u32,
    }).collect();

    Ok(RenameResult { files_changed, edits_applied, details })
}

// ── Types ───────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct RenameResult {
    pub files_changed: u32,
    pub edits_applied: u32,
    pub details: Vec<RenameFileDetail>,
}

#[derive(serde::Serialize, Clone)]
pub struct RenameFileDetail {
    pub file_path: String,
    pub edit_count: u32,
}

#[derive(serde::Serialize, Clone)]
pub struct LspLocation {
    pub file_path: String,
    pub line: u32,
    pub character: u32,
}

#[derive(serde::Serialize, Clone)]
pub struct LspHoverResult {
    /// "markdown" or "plaintext"
    pub kind: String,
    pub text: String,
}

#[derive(serde::Serialize, Clone)]
pub struct LspDiagnostic {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub severity: String,
    pub message: String,
    pub source: String,
}

// ── Tauri commands (all async, never block the UI) ──────────────────

/// Start LSP server(s) for a workspace in the background.
/// Returns immediately. Emits lsp-status events as server starts/ready.
#[tauri::command]
pub async fn lsp_start_server(
    workspace_id: String,
    app: AppHandle,
    state: State<'_, Arc<Mutex<AppState>>>,
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<(), String> {
    let (repo_id, repo_path, worktree_path) = resolve_ws(state.inner(), &workspace_id)?;

    let user_overrides = state
        .lock()
        .ok()
        .and_then(|st| st.repo_settings.get(&repo_id).map(|s| s.lsp_servers.clone()))
        .unwrap_or_default();
    let configs = resolve_configs(&user_overrides);
    let detected = lsp::detect::detect_servers(&repo_path, &configs);

    if detected.is_empty() {
        return Ok(());
    }

    let lsp_mgr = lsp_manager.inner().clone();

    for (server_id, config, project_root) in detected {
        let key = LspServerKey {
            repo_id: repo_id.clone(),
            server_id: server_id.to_string(),
        };

        // Skip if already running
        if let Ok(mgr) = lsp_mgr.try_lock() {
            if mgr.is_running(&key) {
                continue;
            }
        }

        let config_owned = config.clone();
        let project_root = project_root.clone();
        // Compute worktree project dir: if project is in a subdirectory (e.g., "backend/"),
        // register <worktree>/backend/ instead of just <worktree>/ so the LSP server
        // can find pyproject.toml, source roots, etc. in the worktree too.
        let worktree_project_dir = if let Ok(subdir) = project_root.strip_prefix(&repo_path) {
            if !subdir.as_os_str().is_empty() {
                worktree_path.join(subdir)
            } else {
                worktree_path.clone()
            }
        } else {
            worktree_path.clone()
        };
        let lsp_mgr = lsp_mgr.clone();
        let app = app.clone();

        std::thread::spawn(move || {
            // 1. Brief mutex hold: validate binary, check if already running
            let binary_path = {
                let mut mgr = match lsp_mgr.lock() {
                    Ok(m) => m,
                    Err(e) => {
                        let _ = app.emit("lsp-status", serde_json::json!({
                            "server_id": key.server_id,
                            "status": "error",
                            "message": format!("{}", e),
                        }));
                        return;
                    }
                };
                match mgr.prepare_start(&key, &config_owned) {
                    Ok(Some(path)) => path,
                    Ok(None) => {
                        // Already running
                        let _ = app.emit("lsp-status", serde_json::json!({
                            "server_id": key.server_id,
                            "status": "ready",
                            "message": format!("{} language server ready", key.server_id),
                        }));
                        return;
                    }
                    Err(e) => {
                        let _ = app.emit("lsp-status", serde_json::json!({
                            "server_id": key.server_id,
                            "status": "error",
                            "message": format!("{}", e),
                        }));
                        return;
                    }
                }
                // LspManager mutex released here
            };

            let _ = app.emit("lsp-status", serde_json::json!({
                "server_id": key.server_id,
                "status": "starting",
                "message": format!("Starting {} language server...", key.server_id),
            }));

            // 2. Start server WITHOUT holding LspManager mutex
            let result = lsp::server::start_server(&binary_path, &config_owned, &project_root);

            match result {
                Ok(handle) => {
                    let _ = lsp::add_worktree(&handle, &worktree_project_dir);

                    // 3. Brief mutex hold: insert handle — now visible to UI via get_existing
                    if let Ok(mut mgr) = lsp_mgr.lock() {
                        mgr.insert(key.clone(), handle.clone());
                    }

                    let _ = app.emit("lsp-status", serde_json::json!({
                        "server_id": key.server_id,
                        "status": "indexing",
                        "message": format!("{} indexing...", key.server_id),
                    }));

                    // 4. Wait for readiness WITHOUT holding mutex (10-60s)
                    let _ = lsp::server::wait_for_ready(&handle, &project_root);

                    let _ = app.emit("lsp-status", serde_json::json!({
                        "server_id": key.server_id,
                        "status": "ready",
                        "message": format!("{} ready", key.server_id),
                    }));
                }
                Err(e) => {
                    let _ = app.emit("lsp-status", serde_json::json!({
                        "server_id": key.server_id,
                        "status": "error",
                        "message": format!("{}", e),
                    }));
                }
            }
        });
    }

    Ok(())
}

/// Query current LSP server status. Called by frontend on mount to populate status bar.
#[tauri::command]
pub fn lsp_get_status(
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<Vec<serde_json::Value>, String> {
    let mgr = lsp_manager.lock().map_err(|e| e.to_string())?;
    Ok(mgr.list_running())
}

#[tauri::command]
pub async fn lsp_hover(
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
    state: State<'_, Arc<Mutex<AppState>>>,
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<Option<LspHoverResult>, String> {
    let s = state.inner().clone();
    let m = lsp_manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || do_hover(s, m, workspace_id, file_path, line, character))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn lsp_goto_definition(
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
    state: State<'_, Arc<Mutex<AppState>>>,
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<Option<LspLocation>, String> {
    let s = state.inner().clone();
    let m = lsp_manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || do_goto_definition(s, m, workspace_id, file_path, line, character))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn lsp_diagnostics(
    workspace_id: String,
    file_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<Vec<LspDiagnostic>, String> {
    let s = state.inner().clone();
    let m = lsp_manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || do_diagnostics(s, m, workspace_id, file_path))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn lsp_rename(
    workspace_id: String,
    file_path: String,
    line: u32,
    character: u32,
    new_name: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    lsp_manager: State<'_, Arc<Mutex<LspServerPool>>>,
) -> Result<RenameResult, String> {
    let s = state.inner().clone();
    let m = lsp_manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || do_rename(s, m, workspace_id, file_path, line, character, new_name))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

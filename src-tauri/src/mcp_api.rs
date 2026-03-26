/// Tiny HTTP API for MCP server callbacks.
/// Listens on localhost with a random port. The port is stored in AppState
/// so it can be passed to the MCP server via environment variable.

use crate::lsp;
use crate::lsp::server::LspManager;
use crate::lsp::types::{config_for_extension, resolve_configs, LspServerKey};
use crate::state::AppState;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub fn start_api(
    app: AppHandle,
    state: Arc<Mutex<AppState>>,
    lsp_manager: Arc<Mutex<LspManager>>,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind MCP API port");
    let port = listener.local_addr().expect("No local addr").port();

    tracing::info!("MCP API listening on 127.0.0.1:{}", port);

    let app_clone = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let state = state.clone();
            let app = app_clone.clone();
            let lsp = lsp_manager.clone();

            std::thread::spawn(move || {
                if let Err(e) = handle_request(&mut stream, &state, &app, &lsp) {
                    tracing::debug!("MCP API request error: {}", e);
                }
            });
        }
    });

    port
}

fn handle_request(
    stream: &mut std::net::TcpStream,
    state: &Arc<Mutex<AppState>>,
    app: &AppHandle,
    lsp_mgr: &Arc<Mutex<LspManager>>,
) -> Result<(), String> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    // Read request line
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|e| e.to_string())?;

    // Read headers to get content-length
    let mut content_length: usize = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).map_err(|e| e.to_string())?;
        if header.trim().is_empty() {
            break;
        }
        if let Some(val) = header.strip_prefix("Content-Length: ") {
            content_length = val.trim().parse().unwrap_or(0);
        }
        if let Some(val) = header.strip_prefix("content-length: ") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    }

    let parts: Vec<&str> = request_line.trim().split(' ').collect();
    let method = parts.first().unwrap_or(&"");
    let path = parts.get(1).unwrap_or(&"");

    let (status, response_body) = match (*method, *path) {
        ("POST", "/rename-branch") => handle_rename_branch(&body, state, app),
        ("GET", "/workspace-info") => handle_workspace_info(&request_line, state),
        ("POST", "/notify") => handle_notify(&body, app),
        // LSP routes
        ("POST", "/lsp/goto-definition") => handle_lsp_goto_definition(&body, state, lsp_mgr, app),
        ("POST", "/lsp/references") => handle_lsp_references(&body, state, lsp_mgr, app),
        ("POST", "/lsp/hover") => handle_lsp_hover(&body, state, lsp_mgr, app),
        ("POST", "/lsp/workspace-symbols") => handle_lsp_workspace_symbols(&body, state, lsp_mgr, app),
        ("POST", "/lsp/diagnostics") => handle_lsp_diagnostics(&body, state, lsp_mgr, app),
        ("POST", "/lsp/rename") => handle_lsp_rename(&body, state, lsp_mgr, app),
        _ => ("404 Not Found".to_string(), r#"{"error":"not found"}"#.to_string()),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        response_body.len(),
        response_body,
    );

    stream.write_all(response.as_bytes()).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())?;

    Ok(())
}

fn handle_rename_branch(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    app: &AppHandle,
) -> (String, String) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return ("400 Bad Request".into(), r#"{"error":"invalid json"}"#.into());
    };

    let workspace_id = match v.get("workspace_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return ("400 Bad Request".into(), r#"{"error":"missing workspace_id"}"#.into()),
    };

    let new_name = match v.get("new_name").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return ("400 Bad Request".into(), r#"{"error":"missing new_name"}"#.into()),
    };

    let mut st = match state.lock() {
        Ok(st) => st,
        Err(e) => return ("500 Internal Server Error".into(), format!(r#"{{"error":"{}"}}"#, e)),
    };

    let ws = match st.workspaces.get(&workspace_id) {
        Some(ws) => ws,
        None => return ("404 Not Found".into(), r#"{"error":"workspace not found"}"#.into()),
    };

    let worktree_path = ws.worktree_path.clone();
    let fallback_branch = ws.branch.clone();

    if let Err(e) = crate::state::rename_git_branch(&worktree_path, &new_name, &fallback_branch) {
        return ("500 Internal Server Error".into(), format!(r#"{{"error":"{}"}}"#, e));
    }

    if let Some(ws) = st.workspaces.get_mut(&workspace_id) {
        ws.branch = new_name.clone();
        ws.name = new_name.clone();
        let _ = st.save_workspaces();
    }

    // Emit event so the frontend updates immediately
    if let Some(ws) = st.workspaces.get(&workspace_id) {
        let _ = app.emit("workspace-updated", ws.clone());
    }

    (
        "200 OK".into(),
        format!(r#"{{"ok":true,"branch":"{}","name":"{}"}}"#, new_name, new_name),
    )
}

fn handle_workspace_info(
    request_line: &str,
    state: &Arc<Mutex<AppState>>,
) -> (String, String) {
    // Parse workspace_id from query string: GET /workspace-info?workspace_id=xxx
    let workspace_id = request_line
        .split('?')
        .nth(1)
        .and_then(|qs| {
            qs.split('&').find_map(|param| {
                let mut kv = param.split('=');
                if kv.next()? == "workspace_id" {
                    kv.next().map(|v| v.split_whitespace().next().unwrap_or(v).to_string())
                } else {
                    None
                }
            })
        });

    let workspace_id = match workspace_id {
        Some(id) => id,
        None => return ("400 Bad Request".into(), r#"{"error":"missing workspace_id"}"#.into()),
    };

    let st = match state.lock() {
        Ok(st) => st,
        Err(e) => return ("500 Internal Server Error".into(), format!(r#"{{"error":"{}"}}"#, e)),
    };

    let ws = match st.workspaces.get(&workspace_id) {
        Some(ws) => ws,
        None => return ("404 Not Found".into(), r#"{"error":"workspace not found"}"#.into()),
    };

    let json = serde_json::to_string(ws).unwrap_or_else(|_| "{}".to_string());
    ("200 OK".into(), json)
}

fn handle_notify(body: &[u8], app: &AppHandle) -> (String, String) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return ("400 Bad Request".into(), r#"{"error":"invalid json"}"#.into());
    };

    // Emit a notification event to the frontend
    let _ = app.emit("agent-notify", v);
    ("200 OK".into(), r#"{"ok":true}"#.into())
}

// ── LSP handlers ────────────────────────────────────────────────────

/// Resolve workspace_id → (repo_id, repo_path, worktree_path).
fn resolve_workspace(
    state: &Arc<Mutex<AppState>>,
    workspace_id: &str,
) -> Result<(String, PathBuf, PathBuf), String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    let ws = st
        .workspaces
        .get(workspace_id)
        .ok_or_else(|| "workspace not found".to_string())?;
    let repo = st
        .repos
        .get(&ws.repo_id)
        .ok_or_else(|| "repo not found".to_string())?;
    Ok((ws.repo_id.clone(), repo.path.clone(), ws.worktree_path.clone()))
}

/// Parse common LSP request fields from JSON body.
fn parse_lsp_body(body: &[u8]) -> Result<(String, String, u32, u32), (String, String)> {
    let v = serde_json::from_slice::<serde_json::Value>(body)
        .map_err(|_| ("400 Bad Request".to_string(), r#"{"error":"invalid json"}"#.to_string()))?;

    let workspace_id = v
        .get("workspace_id")
        .and_then(|s| s.as_str())
        .ok_or_else(|| {
            (
                "400 Bad Request".to_string(),
                r#"{"error":"missing workspace_id"}"#.to_string(),
            )
        })?
        .to_string();

    let file_path = v
        .get("file_path")
        .and_then(|s| s.as_str())
        .ok_or_else(|| {
            (
                "400 Bad Request".to_string(),
                r#"{"error":"missing file_path"}"#.to_string(),
            )
        })?
        .to_string();

    // 1-based from agent, convert to 0-based for LSP
    let line = v
        .get("line")
        .and_then(|n| n.as_u64())
        .unwrap_or(1)
        .saturating_sub(1) as u32;
    let character = v
        .get("character")
        .and_then(|n| n.as_u64())
        .unwrap_or(1)
        .saturating_sub(1) as u32;

    Ok((workspace_id, file_path, line, character))
}

/// Get or start an LSP server and ensure the worktree is registered.
/// Returns (handle, worktree_path, abs_file_path, language_id).
/// Emits `lsp-status` events to the frontend for UI indicators.
fn get_lsp_server(
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    workspace_id: &str,
    file_path: &str,
    app: &AppHandle,
) -> Result<(Arc<Mutex<crate::lsp::types::LspServerHandle>>, PathBuf, PathBuf, String), (String, String)>
{
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let (repo_id, repo_path, worktree_path) =
        resolve_workspace(state, workspace_id).map_err(|e| {
            ("404 Not Found".to_string(), format!(r#"{{"error":"{}"}}"#, e))
        })?;

    // Resolve LSP configs: built-in defaults merged with user overrides from RepoSettings
    let user_overrides = state
        .lock()
        .ok()
        .and_then(|st| st.repo_settings.get(&repo_id).map(|s| s.lsp_servers.clone()))
        .unwrap_or_default();
    let configs = resolve_configs(&user_overrides);

    let (server_id, config) = config_for_extension(&configs, ext).ok_or_else(|| {
        (
            "400 Bad Request".to_string(),
            format!(r#"{{"error":"no LSP server configured for .{} files"}}"#, ext),
        )
    })?;

    let language_id = config.language_id.clone();
    let config_owned = config.clone();

    let key = LspServerKey {
        repo_id,
        server_id: server_id.to_string(),
    };

    // Check if server already running (skip UI notification)
    let already_running = {
        let mgr = lsp_mgr.lock().map_err(|e| {
            ("500 Internal Server Error".to_string(), format!(r#"{{"error":"{}"}}"#, e))
        })?;
        mgr.is_running(&key)
    };

    if !already_running {
        let _ = app.emit("lsp-status", serde_json::json!({
            "server_id": key.server_id,
            "status": "starting",
            "message": format!("Starting {} language server...", key.server_id),
        }));
    }

    let (handle_arc, just_started) = {
        let mut mgr = lsp_mgr.lock().map_err(|e| {
            ("500 Internal Server Error".to_string(), format!(r#"{{"error":"{}"}}"#, e))
        })?;
        match mgr.get_or_start(&key, &config_owned, &repo_path) {
            Ok(v) => v,
            Err(e) => {
                let _ = app.emit("lsp-status", serde_json::json!({
                    "server_id": key.server_id,
                    "status": "error",
                    "message": format!("{}", e),
                }));
                return Err((
                    "500 Internal Server Error".to_string(),
                    format!(r#"{{"error":"{}"}}"#, e),
                ));
            }
        }
        // LspManager mutex released here
    };

    // For the MCP path (agents), wait for indexing to complete before querying
    if just_started {
        let _ = app.emit("lsp-status", serde_json::json!({
            "server_id": key.server_id,
            "status": "indexing",
            "message": format!("{} indexing...", key.server_id),
        }));
        let _ = lsp::server::wait_for_ready(&handle_arc, &repo_path);
        let _ = app.emit("lsp-status", serde_json::json!({
            "server_id": key.server_id,
            "status": "ready",
            "message": format!("{} ready", key.server_id),
        }));
    }

    // Ensure worktree is registered as a workspace folder
    let _ = lsp::add_worktree(&handle_arc, &worktree_path);

    let abs_path = worktree_path.join(file_path);
    Ok((handle_arc, worktree_path, abs_path, language_id))
}

fn handle_lsp_goto_definition(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    app: &AppHandle,
) -> (String, String) {
    let (workspace_id, file_path, line, character) = match parse_lsp_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let (handle, worktree_path, abs_path, language) =
        match get_lsp_server(state, lsp_mgr, &workspace_id, &file_path, app) {
            Ok(v) => v,
            Err(e) => return e,
        };

    match lsp::goto_definition(&handle, &abs_path, line, character, &language) {
        Ok(result) => {
            let formatted = lsp::format_locations(&result, &worktree_path);
            let body = serde_json::json!({ "text": formatted }).to_string();
            ("200 OK".into(), body)
        }
        Err(e) => (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"{}"}}"#, e),
        ),
    }
}

fn handle_lsp_references(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    app: &AppHandle,
) -> (String, String) {
    let (workspace_id, file_path, line, character) = match parse_lsp_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let (handle, worktree_path, abs_path, language) =
        match get_lsp_server(state, lsp_mgr, &workspace_id, &file_path, app) {
            Ok(v) => v,
            Err(e) => return e,
        };

    match lsp::find_references(&handle, &abs_path, line, character, &language, true)
    {
        Ok(result) => {
            let formatted = lsp::format_locations(&result, &worktree_path);
            let body = serde_json::json!({ "text": formatted }).to_string();
            ("200 OK".into(), body)
        }
        Err(e) => (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"{}"}}"#, e),
        ),
    }
}

fn handle_lsp_hover(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    app: &AppHandle,
) -> (String, String) {
    let (workspace_id, file_path, line, character) = match parse_lsp_body(body) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let (handle, _worktree_path, abs_path, language) =
        match get_lsp_server(state, lsp_mgr, &workspace_id, &file_path, app) {
            Ok(v) => v,
            Err(e) => return e,
        };

    match lsp::hover(&handle, &abs_path, line, character, &language) {
        Ok(result) => {
            let formatted = lsp::format_hover(&result);
            let body = serde_json::json!({ "text": formatted }).to_string();
            ("200 OK".into(), body)
        }
        Err(e) => (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"{}"}}"#, e),
        ),
    }
}

fn handle_lsp_workspace_symbols(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    _app: &AppHandle,
) -> (String, String) {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(_) => {
            return (
                "400 Bad Request".into(),
                r#"{"error":"invalid json"}"#.into(),
            )
        }
    };

    let workspace_id = match v.get("workspace_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                "400 Bad Request".into(),
                r#"{"error":"missing workspace_id"}"#.into(),
            )
        }
    };

    let query = v
        .get("query")
        .and_then(|s| s.as_str())
        .unwrap_or("");

    let (repo_id, repo_path, worktree_path) =
        match resolve_workspace(state, &workspace_id) {
            Ok(v) => v,
            Err(e) => {
                return (
                    "404 Not Found".into(),
                    format!(r#"{{"error":"{}"}}"#, e),
                )
            }
        };

    // Resolve configs and detect which servers apply to this repo
    let user_overrides = state
        .lock()
        .ok()
        .and_then(|st| st.repo_settings.get(&repo_id).map(|s| s.lsp_servers.clone()))
        .unwrap_or_default();
    let configs = resolve_configs(&user_overrides);
    let detected = lsp::detect::detect_servers(&repo_path, &configs);

    if detected.is_empty() {
        return (
            "400 Bad Request".into(),
            r#"{"error":"no LSP servers detected for this repo"}"#.into(),
        );
    }

    // Query all detected language servers and merge results
    let mut all_results = Vec::new();
    for (server_id, config, project_root) in detected {
        let key = LspServerKey {
            repo_id: repo_id.clone(),
            server_id: server_id.to_string(),
        };
        let handle_arc = {
            let mut mgr = match lsp_mgr.lock() {
                Ok(m) => m,
                Err(e) => {
                    return (
                        "500 Internal Server Error".into(),
                        format!(r#"{{"error":"{}"}}"#, e),
                    )
                }
            };
            match mgr.get_or_start(&key, config, &project_root) {
                Ok((h, _)) => h,
                Err(_) => continue, // skip servers whose binary isn't installed
            }
        };
        let _ = lsp::add_worktree(&handle_arc, &worktree_path);

        if let Ok(result) = lsp::workspace_symbols(&handle_arc, query) {
            if let Some(arr) = result.as_array() {
                all_results.extend(arr.clone());
            }
        }
    }

    let merged = serde_json::Value::Array(all_results);
    let formatted = lsp::format_symbols(&merged, &worktree_path);
    let body = serde_json::json!({ "text": formatted }).to_string();
    ("200 OK".into(), body)
}

fn handle_lsp_diagnostics(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    app: &AppHandle,
) -> (String, String) {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(_) => {
            return (
                "400 Bad Request".into(),
                r#"{"error":"invalid json"}"#.into(),
            )
        }
    };

    let workspace_id = match v.get("workspace_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                "400 Bad Request".into(),
                r#"{"error":"missing workspace_id"}"#.into(),
            )
        }
    };

    let file_path = match v.get("file_path").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                "400 Bad Request".into(),
                r#"{"error":"missing file_path"}"#.into(),
            )
        }
    };

    let (handle, _worktree_path, abs_path, language) =
        match get_lsp_server(state, lsp_mgr, &workspace_id, &file_path, app) {
            Ok(v) => v,
            Err(e) => return e,
        };

    match lsp::get_diagnostics(&handle, &abs_path, &language) {
        Ok(result) => {
            let formatted = lsp::format_diagnostics(&result);
            let body = serde_json::json!({ "text": formatted }).to_string();
            ("200 OK".into(), body)
        }
        Err(e) => (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"{}"}}"#, e),
        ),
    }
}

fn handle_lsp_rename(
    body: &[u8],
    state: &Arc<Mutex<AppState>>,
    lsp_mgr: &Arc<Mutex<LspManager>>,
    app: &AppHandle,
) -> (String, String) {
    let v = match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => v,
        Err(_) => return ("400 Bad Request".into(), r#"{"error":"invalid json"}"#.into()),
    };

    let workspace_id = match v.get("workspace_id").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return ("400 Bad Request".into(), r#"{"error":"missing workspace_id"}"#.into()),
    };
    let file_path = match v.get("file_path").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return ("400 Bad Request".into(), r#"{"error":"missing file_path"}"#.into()),
    };
    let new_name = match v.get("new_name").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => return ("400 Bad Request".into(), r#"{"error":"missing new_name"}"#.into()),
    };
    let line = v.get("line").and_then(|n| n.as_u64()).unwrap_or(1).saturating_sub(1) as u32;
    let character = v.get("character").and_then(|n| n.as_u64()).unwrap_or(1).saturating_sub(1) as u32;

    let (handle, worktree_path, abs_path, language) =
        match get_lsp_server(state, lsp_mgr, &workspace_id, &file_path, app) {
            Ok(v) => v,
            Err(e) => return e,
        };

    // Get the workspace edit from LSP
    let edit = match lsp::rename(&handle, &abs_path, line, character, &new_name, &language) {
        Ok(e) => e,
        Err(e) => return (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"{}"}}"#, e),
        ),
    };

    if edit.is_null() {
        return ("200 OK".into(), r#"{"text":"Symbol cannot be renamed at this position."}"#.into());
    }

    // Apply the edits to files on disk
    match lsp::apply_workspace_edit(&edit, &worktree_path) {
        Ok(summary) => {
            let formatted = lsp::format_rename_result(&summary);
            let body = serde_json::json!({ "text": formatted }).to_string();
            ("200 OK".into(), body)
        }
        Err(e) => (
            "500 Internal Server Error".into(),
            format!(r#"{{"error":"Failed to apply rename: {}"}}"#, e),
        ),
    }
}

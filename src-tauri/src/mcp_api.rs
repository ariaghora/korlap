/// Tiny HTTP API for MCP server callbacks.
/// Listens on localhost with a random port. The port is stored in AppState
/// so it can be passed to the MCP server via environment variable.

use crate::state::{AppState, WorkspaceStatus};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub fn start_api(app: AppHandle, state: Arc<Mutex<AppState>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind MCP API port");
    let port = listener.local_addr().expect("No local addr").port();

    tracing::info!("MCP API listening on 127.0.0.1:{}", port);

    let app_clone = app.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let state = state.clone();
            let app = app_clone.clone();

            std::thread::spawn(move || {
                if let Err(e) = handle_request(&mut stream, &state, &app) {
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

    if ws.status == WorkspaceStatus::Archived {
        return ("400 Bad Request".into(), r#"{"error":"cannot rename archived workspace"}"#.into());
    }

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

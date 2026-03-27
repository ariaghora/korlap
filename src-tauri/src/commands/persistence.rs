use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

// ── Repo Settings ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_repo_settings(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<crate::state::RepoSettings, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    Ok(st
        .repo_settings
        .get(&repo_id)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub fn save_repo_settings(
    repo_id: String,
    settings: crate::state::RepoSettings,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.repo_settings.insert(repo_id, settings);
    st.save_repo_settings()?;
    Ok(())
}

// ── Message persistence ──────────────────────────────────────────────

#[tauri::command]
pub fn save_messages(
    workspace_id: String,
    messages: serde_json::Value,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let msg_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.messages_dir()
    };
    std::fs::create_dir_all(&msg_dir).map_err(|e| e.to_string())?;
    let msg_file = msg_dir.join(format!("{}.json", workspace_id));
    let data = serde_json::to_string(&messages).map_err(|e| e.to_string())?;
    std::fs::write(&msg_file, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_messages(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let msg_file = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.messages_dir().join(format!("{}.json", workspace_id))
    };

    if !msg_file.exists() {
        return Ok(serde_json::json!([]));
    }

    let data = std::fs::read_to_string(&msg_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

// ── Todo persistence ─────────────────────────────────────────────────

#[tauri::command]
pub fn save_todos(
    repo_id: String,
    todos: serde_json::Value,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let todos_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.todos_dir()
    };
    std::fs::create_dir_all(&todos_dir).map_err(|e| e.to_string())?;
    let todos_file = todos_dir.join(format!("{}.json", repo_id));
    let data = serde_json::to_string(&todos).map_err(|e| e.to_string())?;
    std::fs::write(&todos_file, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn load_todos(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<serde_json::Value, String> {
    let todos_file = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.todos_dir().join(format!("{}.json", repo_id))
    };

    if !todos_file.exists() {
        return Ok(serde_json::json!([]));
    }

    let data = std::fs::read_to_string(&todos_file).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

// ── MCP server test ──────────────────────────────────────────────────

#[tauri::command]
pub async fn test_mcp_server(config: crate::state::McpServerConfig) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::Duration;

    if config.server_type == "sse" {
        // SSE: HTTP reachability check via curl
        let url = config.url.trim();
        if url.is_empty() {
            return Err("No URL configured".into());
        }
        let mut cmd = Command::new("curl");
        cmd.args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "--max-time", "5", url]);
        super::helpers::inject_shell_env(&mut cmd);
        let output = cmd.output().map_err(|e| format!("Failed to run curl: {}", e))?;
        if output.status.success() {
            let code = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(format!("Server reachable (HTTP {})", code))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(format!("Connection failed: {}", if stderr.is_empty() { "unknown error".into() } else { stderr }))
        }
    } else {
        // stdio: spawn process and send MCP initialize handshake
        let command = config.command.trim();
        if command.is_empty() {
            return Err("No command configured".into());
        }
        let mut cmd = Command::new(command);
        cmd.args(&config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        super::helpers::inject_shell_env(&mut cmd);
        cmd.envs(&config.env);

        let mut child = cmd.spawn().map_err(|e| format!("Failed to start '{}': {}", command, e))?;

        // Write MCP initialize request
        let init_msg = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"korlap","version":"0.1.0"}}}"#;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "{}", init_msg);
            // Don't close stdin — MCP servers expect it to stay open
        }

        // Read response with 5s timeout
        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(line) = line {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        let _ = tx.send(trimmed);
                        return;
                    }
                } else {
                    return;
                }
            }
        });

        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(response) => {
                let _ = child.kill();
                let _ = child.wait();
                // Parse response
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
                    if let Some(err) = json.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("unknown");
                        return Err(format!("Server error: {}", msg));
                    }
                    if let Some(info) = json.pointer("/result/serverInfo") {
                        let name = info.get("name").and_then(|n| n.as_str()).unwrap_or("unknown");
                        let version = info.get("version").and_then(|v| v.as_str()).unwrap_or("?");
                        return Ok(format!("{} v{}", name, version));
                    }
                    Ok("Connected (no server info in response)".into())
                } else {
                    Ok("Connected (non-JSON response)".into())
                }
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                // Check stderr for clues
                if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut buf);
                    if !buf.trim().is_empty() {
                        return Err(format!("Timed out (5s). Stderr: {}", buf.trim().chars().take(200).collect::<String>()));
                    }
                }
                Err("Timed out waiting for response (5s)".into())
            }
        }
    }
}

// ── Image commands ───────────────────────────────────────────────────

/// Save base64-encoded image data to the app data directory.
/// Returns the absolute path to the saved image.
/// Images are stored under `<data_dir>/images/<workspace_id>/` — never in the worktree.
#[tauri::command]
pub fn save_image(
    workspace_id: String,
    data: String,
    extension: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let images_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.data_dir.join("images").join(&workspace_id)
    };

    // Decode base64
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid base64 data: {}", e))?;

    std::fs::create_dir_all(&images_dir)
        .map_err(|e| format!("Failed to create images dir: {}", e))?;

    let ext = if extension.is_empty() { "png" } else { &extension };
    let filename = format!("{}.{}", Uuid::new_v4(), ext);
    let file_path = images_dir.join(&filename);

    std::fs::write(&file_path, &bytes)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(file_path.to_string_lossy().to_string())
}

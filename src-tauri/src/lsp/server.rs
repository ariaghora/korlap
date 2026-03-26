use super::types::*;
use crate::commands::helpers::inject_shell_env;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(60);

// ── LspManager ──────────────────────────────────────────────────────

/// Manages all running LSP servers. Keyed by (repo_id, language).
/// Lives in its own Arc<Mutex<>> — NOT inside AppState — so LSP I/O
/// never blocks Tauri commands.
pub struct LspManager {
    servers: HashMap<LspServerKey, Arc<Mutex<LspServerHandle>>>,
}

impl LspManager {
    pub fn new() -> Self {
        LspManager {
            servers: HashMap::new(),
        }
    }

    /// Check if a server is already running for this key.
    pub fn is_running(&self, key: &LspServerKey) -> bool {
        if let Some(handle_arc) = self.servers.get(key) {
            if let Ok(mut handle) = handle_arc.lock() {
                return matches!(handle.child.try_wait(), Ok(None));
            }
        }
        false
    }

    /// Get an already-running server. Returns None if not started or dead.
    /// Non-blocking — never spawns a new server.
    pub fn get_existing(
        &mut self,
        key: &LspServerKey,
    ) -> Option<Arc<Mutex<LspServerHandle>>> {
        let handle_arc = self.servers.get(key)?;
        let mut handle = handle_arc.lock().ok()?;
        match handle.child.try_wait() {
            Ok(None) => Some(Arc::clone(handle_arc)), // still alive
            _ => {
                drop(handle);
                self.servers.remove(key);
                None
            }
        }
    }

    /// Get or start an LSP server for the given key + config.
    /// Returns (handle, just_started). If just_started is true, the caller
    /// should call `wait_for_ready` outside the mutex before querying.
    pub fn get_or_start(
        &mut self,
        key: &LspServerKey,
        config: &LspServerConfig,
        repo_root: &Path,
    ) -> Result<(Arc<Mutex<LspServerHandle>>, bool), LspError> {
        // Check if server exists and is still alive
        if let Some(handle_arc) = self.servers.get(key) {
            let mut handle = handle_arc
                .lock()
                .map_err(|e| LspError::Transport(e.to_string()))?;
            match handle.child.try_wait() {
                Ok(Some(_)) => {
                    drop(handle);
                    self.servers.remove(key);
                }
                Ok(None) => {
                    return Ok((Arc::clone(handle_arc), false));
                }
                Err(e) => {
                    tracing::warn!("Error checking LSP server status: {}", e);
                    drop(handle);
                    self.servers.remove(key);
                }
            }
        }

        let binary_path = super::detect::validate_binary(config)
            .map_err(LspError::BinaryNotFound)?;

        let handle_arc = start_server(&binary_path, config, repo_root)?;
        self.servers
            .insert(key.clone(), Arc::clone(&handle_arc));
        Ok((handle_arc, true))
    }

    /// Validate binary and insert a placeholder. Returns binary path if server
    /// needs starting, or None if already running. Holds mutex only briefly.
    pub fn prepare_start(
        &mut self,
        key: &LspServerKey,
        config: &LspServerConfig,
    ) -> Result<Option<std::path::PathBuf>, LspError> {
        // Already running?
        if let Some(handle_arc) = self.servers.get(key) {
            if let Ok(mut handle) = handle_arc.lock() {
                match handle.child.try_wait() {
                    Ok(None) => return Ok(None), // already running
                    _ => {}
                }
            }
            self.servers.remove(key);
        }

        let binary_path = super::detect::validate_binary(config)
            .map_err(LspError::BinaryNotFound)?;
        Ok(Some(binary_path))
    }

    /// List all running servers with their IDs and status.
    pub fn list_running(&self) -> Vec<serde_json::Value> {
        self.servers
            .iter()
            .filter_map(|(key, handle_arc)| {
                let mut handle = handle_arc.lock().ok()?;
                if matches!(handle.child.try_wait(), Ok(None)) {
                    Some(serde_json::json!({
                        "server_id": key.server_id,
                        "status": if handle.initialized { "ready" } else { "starting" },
                    }))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Insert a started server handle into the manager.
    pub fn insert(&mut self, key: LspServerKey, handle: Arc<Mutex<LspServerHandle>>) {
        self.servers.insert(key, handle);
    }

    /// Remove a worktree from all servers for a given repo.
    /// Shuts down servers with no remaining workspace folders.
    pub fn remove_worktree(&mut self, repo_id: &str, worktree_path: &Path) {
        let keys_to_check: Vec<LspServerKey> = self
            .servers
            .keys()
            .filter(|k| k.repo_id == repo_id)
            .cloned()
            .collect();

        let mut keys_to_remove = Vec::new();
        for key in keys_to_check {
            if let Some(handle_arc) = self.servers.get(&key) {
                let should_remove = if let Ok(mut handle) = handle_arc.lock() {
                    let uri = path_to_uri(worktree_path);
                    let name = worktree_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let _ = send_notification_inner(
                        &mut handle,
                        "workspace/didChangeWorkspaceFolders",
                        serde_json::json!({
                            "event": {
                                "added": [],
                                "removed": [{ "uri": uri, "name": name }]
                            }
                        }),
                    );
                    handle.workspace_folders.retain(|p| p != worktree_path);
                    if handle.workspace_folders.is_empty() {
                        let _ = shutdown_server(&mut handle);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if should_remove {
                    keys_to_remove.push(key);
                }
            }
        }
        for key in keys_to_remove {
            self.servers.remove(&key);
        }
    }

    /// Shut down all servers for a repo.
    pub fn shutdown_repo(&mut self, repo_id: &str) {
        let keys: Vec<LspServerKey> = self
            .servers
            .keys()
            .filter(|k| k.repo_id == repo_id)
            .cloned()
            .collect();

        for key in keys {
            if let Some(handle_arc) = self.servers.remove(&key) {
                if let Ok(mut handle) = handle_arc.lock() {
                    let _ = shutdown_server(&mut handle);
                }
            }
        }
    }

    /// Shut down all servers (called on app exit).
    pub fn shutdown_all(&mut self) {
        let keys: Vec<LspServerKey> = self.servers.keys().cloned().collect();
        for key in keys {
            if let Some(handle_arc) = self.servers.remove(&key) {
                if let Ok(mut handle) = handle_arc.lock() {
                    let _ = shutdown_server(&mut handle);
                }
            }
        }
    }
}

// ── Server lifecycle ────────────────────────────────────────────────

pub fn start_server(
    binary_path: &Path,
    config: &LspServerConfig,
    repo_root: &Path,
) -> Result<Arc<Mutex<LspServerHandle>>, LspError> {
    let mut cmd = std::process::Command::new(binary_path);
    for arg in &config.args {
        cmd.arg(arg);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.current_dir(repo_root);
    inject_shell_env(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| {
        LspError::Transport(format!(
            "Failed to spawn {}: {}",
            binary_path.display(),
            e
        ))
    })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| LspError::Transport("No stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| LspError::Transport("No stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| LspError::Transport("No stderr".into()))?;

    tracing::info!(
        "Started LSP server: {} (pid {}) for {}",
        config.command,
        child.id(),
        repo_root.display()
    );

    let handle = LspServerHandle {
        child,
        stdin: std::io::BufWriter::new(stdin),
        next_id: 1,
        pending: HashMap::new(),
        workspace_folders: vec![repo_root.to_path_buf()],
        initialized: false,
        open_documents: std::collections::HashSet::new(),
        diagnostics_cache: HashMap::new(),
    };

    let handle_arc = Arc::new(Mutex::new(handle));

    // Spawn stdout reader thread
    let reader_handle = Arc::clone(&handle_arc);
    std::thread::spawn(move || {
        reader_loop(stdout, reader_handle);
    });

    // Drain stderr (log, don't accumulate)
    let cmd_name = config.command.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(l) => tracing::debug!("[{}] {}", cmd_name, l),
                Err(_) => break,
            }
        }
    });

    // Initialize handshake
    do_initialize(&handle_arc, repo_root)?;

    Ok(handle_arc)
}

fn do_initialize(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    root_path: &Path,
) -> Result<(), LspError> {
    let root_uri = path_to_uri(root_path);
    let root_name = root_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let init_params = serde_json::json!({
        "processId": std::process::id(),
        "rootUri": root_uri,
        "capabilities": {
            "textDocument": {
                "definition": { "dynamicRegistration": false },
                "references": { "dynamicRegistration": false },
                "hover": {
                    "dynamicRegistration": false,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "publishDiagnostics": { "relatedInformation": true },
                "synchronization": {
                    "didOpen": true,
                    "didClose": true,
                    "dynamicRegistration": false,
                }
            },
            "workspace": {
                "workspaceFolders": true,
                "symbol": { "dynamicRegistration": false },
                "didChangeWorkspaceFolders": { "dynamicRegistration": false },
            }
        },
        "workspaceFolders": [{
            "uri": root_uri,
            "name": root_name,
        }]
    });

    let result = send_request(handle_arc, "initialize", init_params, INITIALIZE_TIMEOUT)?;

    {
        let mut handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;
        handle.initialized = true;
        let _ = result; // capabilities stored if needed later
    }

    // Send initialized notification
    send_notification(handle_arc, "initialized", serde_json::json!({}))?;

    // NOTE: do NOT wait_for_ready here. start_server must return fast.
    // Callers that need readiness (MCP path, lsp_start_server) call
    // wait_for_ready separately, outside any mutex.

    Ok(())
}

/// Poll the server until it responds successfully to a query.
/// Any successful response (even empty) means the server is ready.
/// Gives up after READY_TIMEOUT and proceeds anyway.
pub fn wait_for_ready(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    root_path: &Path,
) -> Result<(), LspError> {
    const READY_TIMEOUT: Duration = Duration::from_secs(60);
    const POLL_INTERVAL: Duration = Duration::from_secs(2);

    let start = std::time::Instant::now();

    tracing::info!("Waiting for LSP server to index {}...", root_path.display());

    loop {
        if start.elapsed() > READY_TIMEOUT {
            tracing::warn!(
                "LSP server did not become ready within {}s — proceeding anyway",
                READY_TIMEOUT.as_secs()
            );
            return Ok(());
        }

        // workspace/symbol with empty query — any successful response means ready
        match send_request(
            handle_arc,
            "workspace/symbol",
            serde_json::json!({ "query": "" }),
            Duration::from_secs(10),
        ) {
            Ok(_) => {
                tracing::info!(
                    "LSP server ready ({:.1}s)",
                    start.elapsed().as_secs_f64()
                );
                return Ok(());
            }
            Err(LspError::Timeout) | Err(LspError::ServerError { .. }) => {
                // Still indexing — keep waiting
            }
            Err(e) => return Err(e),
        }

        std::thread::sleep(POLL_INTERVAL);
    }
}

fn shutdown_server(handle: &mut LspServerHandle) -> Result<(), LspError> {
    // Best-effort shutdown request
    let id = handle.next_id;
    handle.next_id += 1;
    let msg = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "shutdown",
        "params": null
    });
    let _ = write_message(&mut handle.stdin, &msg);

    // Exit notification
    let exit_msg = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "exit"
    });
    let _ = write_message(&mut handle.stdin, &exit_msg);

    // Brief grace period, then force kill
    std::thread::sleep(Duration::from_millis(500));
    let _ = handle.child.kill();
    let _ = handle.child.wait();

    Ok(())
}

// ── Reader thread ───────────────────────────────────────────────────

fn reader_loop(stdout: std::process::ChildStdout, handle: Arc<Mutex<LspServerHandle>>) {
    let mut reader = BufReader::new(stdout);

    loop {
        // Parse Content-Length header
        let mut content_length: Option<usize> = None;
        loop {
            let mut header_line = String::new();
            match reader.read_line(&mut header_line) {
                Ok(0) => return, // EOF
                Err(_) => return,
                Ok(_) => {}
            }
            let trimmed = header_line.trim();
            if trimmed.is_empty() {
                break; // End of headers
            }
            if let Some(val) = trimmed
                .strip_prefix("Content-Length: ")
                .or_else(|| trimmed.strip_prefix("content-length: "))
            {
                content_length = val.parse().ok();
            }
        }

        let Some(len) = content_length else {
            continue;
        };

        // Read body
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            return; // EOF or error
        }

        let Ok(msg) = serde_json::from_slice::<serde_json::Value>(&body) else {
            tracing::warn!("LSP: invalid JSON in message body");
            continue;
        };

        // Dispatch: response (has "id" + "result"/"error") vs notification (no "id" but has "method")
        if let Some(id) = msg.get("id").and_then(|v| v.as_i64()) {
            // Response to a request we sent
            if msg.get("method").is_some() {
                // Server-initiated request (e.g. window/workDoneProgress/create) — ignore
                continue;
            }
            if let Ok(mut h) = handle.lock() {
                if let Some(sender) = h.pending.remove(&id) {
                    if let Some(error) = msg.get("error") {
                        let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
                        let message = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let _ = sender.send(Err(LspError::ServerError { code, message }));
                    } else {
                        let result = msg
                            .get("result")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null);
                        let _ = sender.send(Ok(result));
                    }
                }
            }
        } else if let Some(method) = msg.get("method").and_then(|m| m.as_str()) {
            handle_server_notification(method, msg.get("params"), &handle);
        }
    }
}

fn handle_server_notification(
    method: &str,
    params: Option<&serde_json::Value>,
    handle: &Arc<Mutex<LspServerHandle>>,
) {
    match method {
        "textDocument/publishDiagnostics" => {
            if let Some(params) = params {
                if let Some(uri) = params.get("uri").and_then(|u| u.as_str()) {
                    let diagnostics = params
                        .get("diagnostics")
                        .cloned()
                        .unwrap_or(serde_json::json!([]));
                    if let Ok(mut h) = handle.lock() {
                        h.diagnostics_cache.insert(uri.to_string(), diagnostics);
                    }
                }
            }
        }
        "window/logMessage" | "window/showMessage" => {
            if let Some(params) = params {
                let msg = params
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");
                tracing::debug!("LSP: {}", msg);
            }
        }
        _ => {}
    }
}

// ── Request/notification transport ──────────────────────────────────

/// Send a JSON-RPC request and wait for the response.
/// Holds the server handle Mutex only briefly to write the message,
/// then blocks on mpsc::Receiver without holding any locks.
pub fn send_request(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    method: &str,
    params: serde_json::Value,
    timeout: Duration,
) -> LspResult {
    let receiver = {
        let mut handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;

        let id = handle.next_id;
        handle.next_id += 1;

        let message = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        write_message(&mut handle.stdin, &message)?;

        let (tx, rx) = std::sync::mpsc::channel();
        handle.pending.insert(id, tx);

        rx
        // handle Mutex dropped here
    };

    // Wait without holding any lock
    receiver
        .recv_timeout(timeout)
        .map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => LspError::Timeout,
            std::sync::mpsc::RecvTimeoutError::Disconnected => LspError::ServerDead,
        })?
}

/// Send a JSON-RPC notification (no response expected).
pub fn send_notification(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    method: &str,
    params: serde_json::Value,
) -> Result<(), LspError> {
    let mut handle = handle_arc
        .lock()
        .map_err(|e| LspError::Transport(e.to_string()))?;
    send_notification_inner(&mut handle, method, params)
}

fn send_notification_inner(
    handle: &mut LspServerHandle,
    method: &str,
    params: serde_json::Value,
) -> Result<(), LspError> {
    let message = serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    });
    write_message(&mut handle.stdin, &message)
}

/// Write a Content-Length-framed JSON-RPC message.
fn write_message(
    writer: &mut std::io::BufWriter<std::process::ChildStdin>,
    message: &serde_json::Value,
) -> Result<(), LspError> {
    let body =
        serde_json::to_string(message).map_err(|e| LspError::Transport(e.to_string()))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .map_err(|e| LspError::Transport(e.to_string()))?;
    writer
        .write_all(body.as_bytes())
        .map_err(|e| LspError::Transport(e.to_string()))?;
    writer
        .flush()
        .map_err(|e| LspError::Transport(e.to_string()))?;
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────

pub fn path_to_uri(path: &Path) -> String {
    // Percent-encode path components for valid file:// URIs.
    // Spaces → %20, etc. LSP servers expect properly encoded URIs.
    let encoded: String = path
        .to_string_lossy()
        .bytes()
        .map(|b| match b {
            // RFC 3986 unreserved + '/' (path separator)
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect();
    format!("file://{}", encoded)
}

pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    // Percent-decode: %20 → space, etc.
    let mut decoded = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                decoded.push(byte);
                i += 3;
                continue;
            }
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    Some(PathBuf::from(String::from_utf8_lossy(&decoded).into_owned()))
}

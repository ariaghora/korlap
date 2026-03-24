use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;

/// Composite key for the terminals HashMap: `{workspace_id}:{terminal_id}`
fn terminal_key(workspace_id: &str, terminal_id: &str) -> String {
    format!("{}:{}", workspace_id, terminal_id)
}

/// Remove and kill all terminals belonging to a workspace.
/// Call while holding the lock.
pub fn kill_workspace_terminals(
    terminals: &mut std::collections::HashMap<String, crate::state::TerminalHandle>,
    workspace_id: &str,
) {
    let prefix = format!("{}:", workspace_id);
    let keys: Vec<String> = terminals
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .cloned()
        .collect();
    for key in keys {
        if let Some(mut handle) = terminals.remove(&key) {
            let _ = handle.child.kill();
        }
    }
}

#[tauri::command]
pub fn open_terminal(
    workspace_id: String,
    terminal_id: String,
    on_data: Channel<Vec<u8>>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let key = terminal_key(&workspace_id, &terminal_id);
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        if st.terminals.contains_key(&key) {
            return Ok(()); // Already open
        }
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    use portable_pty::{CommandBuilder, PtySize, native_pty_system};

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.arg("-l"); // Login shell: sources .zprofile/.zshrc for proper prompt & config
    cmd.cwd(&worktree_path);

    // Terminal identity — critical for readline/zle to handle backspace, arrow keys, etc.
    // Tauri is a GUI app so TERM is not in the parent environment.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("SHELL", &shell);

    // Locale — prevents garbled output for UTF-8 content
    let lang = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".to_string());
    cmd.env("LANG", &lang);
    cmd.env("LC_ALL", &lang);

    // Inject shell env for SSH, PATH, etc.
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        cmd.env("SSH_AUTH_SOCK", sock);
    } else if let Ok(output) = std::process::Command::new("launchctl")
        .args(["getenv", "SSH_AUTH_SOCK"])
        .output()
    {
        let sock = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !sock.is_empty() {
            cmd.env("SSH_AUTH_SOCK", sock);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        cmd.env("HOME", home);
    }
    if let Ok(output) = std::process::Command::new("zsh")
        .args(["-l", "-c", "echo $PATH"])
        .output()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            cmd.env("PATH", path);
        }
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    // Drop slave — parent only needs the master
    drop(pair.slave);

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to get PTY writer: {}", e))?;

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to get PTY reader: {}", e))?;

    // Store handle
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        st.terminals.insert(
            key.clone(),
            crate::state::TerminalHandle {
                writer,
                child,
                master: pair.master,
            },
        );
    }

    // Stream PTY output to frontend via Channel
    let log_key = key.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match std::io::Read::read(&mut reader, &mut buf) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let _ = on_data.send(buf[..n].to_vec());
                }
                Err(_) => break,
            }
        }
        tracing::info!("Terminal reader exited for {}", log_key);
    });

    Ok(())
}

#[tauri::command]
pub fn write_terminal(
    workspace_id: String,
    terminal_id: String,
    data: Vec<u8>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let key = terminal_key(&workspace_id, &terminal_id);
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let handle = st
        .terminals
        .get_mut(&key)
        .ok_or("No terminal open for this workspace")?;

    std::io::Write::write_all(&mut handle.writer, &data)
        .map_err(|e| format!("Failed to write to PTY: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn resize_terminal(
    workspace_id: String,
    terminal_id: String,
    rows: u16,
    cols: u16,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let key = terminal_key(&workspace_id, &terminal_id);
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let handle = st
        .terminals
        .get_mut(&key)
        .ok_or("No terminal open for this workspace")?;

    handle
        .master
        .resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to resize PTY: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn close_terminal(
    workspace_id: String,
    terminal_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let key = terminal_key(&workspace_id, &terminal_id);
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(mut handle) = st.terminals.remove(&key) {
        let _ = handle.child.kill();
        let _ = handle.child.wait();
    }
    Ok(())
}

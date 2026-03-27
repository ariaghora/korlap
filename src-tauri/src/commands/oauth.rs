//! MCP OAuth authentication via Claude CLI.
//!
//! Instead of reimplementing OAuth ourselves, we spawn `claude` interactively
//! with a temporary MCP config containing just the target server. Claude CLI
//! handles the full OAuth flow (discovery, DCR, PKCE, browser, token exchange)
//! and caches tokens in its own credential store. Future agent runs (even in
//! `-p` pipe mode) reuse the cached tokens for the same server URL.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use super::helpers::{get_shell_env, inject_shell_env};

/// Spawn `claude` interactively with a temp MCP config to trigger OAuth for a remote server.
/// Claude CLI handles the entire OAuth flow and caches the token.
/// Returns a status message on success.
#[tauri::command]
pub async fn mcp_oauth_start(url: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || do_claude_oauth(&url))
        .await
        .map_err(|e| format!("OAuth task failed: {}", e))?
}

fn do_claude_oauth(mcp_url: &str) -> Result<String, String> {
    let url = mcp_url.trim();
    if url.is_empty() {
        return Err("No URL configured".into());
    }

    // Write a temp MCP config with just this one server
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "oauth-target": {
                "type": "sse",
                "url": url
            }
        }
    });

    let tmp_dir = std::env::temp_dir().join("korlap-oauth");
    let _ = std::fs::create_dir_all(&tmp_dir);
    let config_path = tmp_dir.join("mcp-auth.json");
    std::fs::write(
        &config_path,
        serde_json::to_string(&mcp_config).unwrap_or_default(),
    )
    .map_err(|e| format!("Failed to write temp MCP config: {}", e))?;

    // Resolve claude binary
    let claude_bin = get_shell_env()
        .claude_path
        .as_deref()
        .unwrap_or("claude");

    // Spawn claude interactively (NOT -p) with the MCP config.
    // Send a prompt that forces MCP tool listing, which triggers the OAuth flow.
    let mut cmd = Command::new(claude_bin);
    cmd.args(["--mcp-config", &config_path.to_string_lossy()]);
    cmd.args(["--allowedTools", ""]); // don't let it run anything dangerous
    cmd.args(["-p", "List all available MCP tools from the oauth-target server. Just list the tool names, nothing else."]);
    cmd.args(["--output-format", "text"]);
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    inject_shell_env(&mut cmd);

    tracing::info!("OAuth: spawning claude to authenticate with {}", url);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn claude: {}", e))?;

    // Read stdout in background with a timeout.
    // Claude will handle OAuth (open browser), then respond with tool list.
    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let (tx, rx) = mpsc::channel::<String>();

    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        let mut output = String::new();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    output.push_str(&l);
                    output.push('\n');
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(output);
    });

    // Wait up to 2 minutes for the OAuth flow to complete
    match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(output) => {
            let _ = child.wait();
            let _ = std::fs::remove_file(&config_path);

            let trimmed = output.trim();
            if trimmed.is_empty() {
                // Check stderr for errors
                if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut buf);
                    if !buf.trim().is_empty() {
                        return Err(buf.trim().chars().take(500).collect::<String>());
                    }
                }
                Err("Claude returned empty output — authentication may have failed".into())
            } else {
                tracing::info!("OAuth: claude auth completed successfully");
                Ok(format!("Authenticated — tokens cached by Claude CLI"))
            }
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&config_path);
            Err("Authentication timed out (2 minutes). Did you complete the browser flow?".into())
        }
    }
}

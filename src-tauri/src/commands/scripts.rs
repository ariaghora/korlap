use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::State;

use super::helpers::inject_shell_env;

#[derive(Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum ScriptEvent {
    #[serde(rename = "output")]
    Output { data: String },
    #[serde(rename = "exit")]
    Exit { code: Option<i32> },
}

#[tauri::command]
pub fn run_script(
    workspace_id: String,
    command: String,
    on_event: Channel<ScriptEvent>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let mut cmd = std::process::Command::new("zsh");
    cmd.args(["-c", &command]);
    cmd.current_dir(&worktree_path);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    inject_shell_env(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run script: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    // Read stdout and stderr concurrently to avoid pipe buffer deadlock.
    // If the child fills the stderr pipe buffer (~64KB) while we're blocked
    // reading stdout, both sides deadlock. This is common with cargo/rustc
    // which write all output to stderr.
    std::thread::spawn(move || {
        use std::io::BufRead;

        let stderr_channel = on_event.clone();
        let stderr_handle = std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let _ = stderr_channel.send(ScriptEvent::Output {
                            data: line + "\n",
                        });
                    }
                    Err(_) => break,
                }
            }
        });

        let stdout_reader = std::io::BufReader::new(stdout);
        for line in stdout_reader.lines() {
            match line {
                Ok(line) => {
                    let _ = on_event.send(ScriptEvent::Output {
                        data: line + "\n",
                    });
                }
                Err(_) => break,
            }
        }

        let _ = stderr_handle.join();
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = on_event.send(ScriptEvent::Exit { code });
    });

    Ok(())
}

use crate::state::{AgentHandle, AppState, RepoInfo, WorkspaceInfo, WorkspaceStatus};
use std::io::BufRead;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Git helpers ──────────────────────────────────────────────────────

fn detect_default_branch(repo_path: &Path) -> Result<String, String> {
    // Try origin HEAD first
    let output = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        let refname = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    // Fall back: check which of main/master exists
    for candidate in ["main", "master"] {
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", candidate])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?;
        if output.status.success() {
            return Ok(candidate.to_string());
        }
    }

    // Last resort: current branch
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }

    Err("Could not detect default branch".into())
}

fn repo_display_name(repo_path: &Path) -> String {
    repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string())
}

// ── Random workspace names ───────────────────────────────────────────

const ADJECTIVES: &[&str] = &[
    "swift", "calm", "bright", "gentle", "quiet", "bold", "keen", "warm",
    "cool", "wild", "deep", "soft", "sharp", "fresh", "still", "true",
    "pure", "rare", "wise", "fair", "clear", "proud", "quick", "neat",
    "slim", "vast", "vivid", "lucid", "amber", "misty",
];

const NOUNS: &[&str] = &[
    "oak", "elm", "pine", "fern", "moss", "reed", "sage", "mint",
    "jade", "onyx", "ruby", "opal", "hawk", "dove", "wolf", "bear",
    "fox", "lynx", "hare", "wren", "lark", "crow", "orca", "puma",
    "coral", "pearl", "ember", "dusk", "dawn", "vale",
];

fn random_workspace_name() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    let mut hasher = DefaultHasher::new();
    seed.hash(&mut hasher);
    let h = hasher.finish();

    let adj = ADJECTIVES[(h as usize) % ADJECTIVES.len()];
    let noun = NOUNS[((h >> 16) as usize) % NOUNS.len()];
    format!("{}-{}", adj, noun)
}

// ── Agent event types (sent to frontend via Channel) ─────────────────

#[derive(Clone, serde::Serialize)]
pub struct ToolUseInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_preview: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        text: String,
        tool_uses: Vec<ToolUseInfo>,
    },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    Error { message: String },
}

fn parse_stream_line(
    line: &str,
    on_event: &Channel<AgentEvent>,
    session_id: &mut Option<String>,
) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    let Some(msg_type) = v.get("type").and_then(|t| t.as_str()) else {
        return;
    };

    match msg_type {
        "system" => {
            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                *session_id = Some(sid.to_string());
            }
        }
        "assistant" => {
            let Some(message) = v.get("message") else {
                return;
            };
            let Some(content) = message.get("content").and_then(|c| c.as_array()) else {
                return;
            };

            let mut text_parts = Vec::new();
            let mut tool_uses = Vec::new();

            for block in content {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let input_preview = block.get("input").and_then(|input| {
                            // Show file_path for file ops, command for Bash
                            if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
                                Some(fp.to_string())
                            } else if let Some(cmd) =
                                input.get("command").and_then(|c| c.as_str())
                            {
                                Some(cmd.chars().take(80).collect())
                            } else if let Some(pattern) =
                                input.get("pattern").and_then(|p| p.as_str())
                            {
                                Some(pattern.to_string())
                            } else {
                                None
                            }
                        });

                        tool_uses.push(ToolUseInfo {
                            name,
                            input_preview,
                        });
                    }
                    _ => {}
                }
            }

            let text = text_parts.join("\n");
            if !text.is_empty() || !tool_uses.is_empty() {
                let _ = on_event.send(AgentEvent::AssistantMessage { text, tool_uses });
            }
        }
        "result" => {
            if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
                *session_id = Some(sid.to_string());
            }
            let _ = on_event.send(AgentEvent::Done);
        }
        _ => {}
    }
}

// ── Repository commands ──────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct RepoDetail {
    #[serde(flatten)]
    pub info: RepoInfo,
    pub display_name: String,
    pub default_branch: String,
}

#[tauri::command]
pub fn add_repo(path: String, state: State<'_, Mutex<AppState>>) -> Result<RepoDetail, String> {
    let path = std::path::PathBuf::from(&path);
    let path = path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    AppState::is_git_repo(&path)?;

    let default_branch = detect_default_branch(&path)?;
    let display_name = repo_display_name(&path);

    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Deduplicate by path
    if let Some(existing) = state.repos.values().find(|r| r.path == path) {
        return Ok(RepoDetail {
            info: existing.clone(),
            display_name,
            default_branch,
        });
    }

    let repo = RepoInfo {
        id: Uuid::new_v4().to_string(),
        path,
        gh_profile: None,
    };

    state.repos.insert(repo.id.clone(), repo.clone());
    state.save_repos()?;

    tracing::info!("Added repo {} at {}", repo.id, repo.path.display());
    Ok(RepoDetail {
        info: repo,
        display_name,
        default_branch,
    })
}

#[tauri::command]
pub fn remove_repo(repo_id: String, state: State<'_, Mutex<AppState>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.repos.remove(&repo_id).ok_or("Repo not found")?;
    state.workspaces.retain(|_, w| w.repo_id != repo_id);
    state.save_repos()?;
    Ok(())
}

#[tauri::command]
pub fn list_repos(state: State<'_, Mutex<AppState>>) -> Result<Vec<RepoDetail>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let mut details = Vec::new();
    for repo in state.repos.values() {
        let default_branch = detect_default_branch(&repo.path).unwrap_or_default();
        let display_name = repo_display_name(&repo.path);
        details.push(RepoDetail {
            info: repo.clone(),
            display_name,
            default_branch,
        });
    }
    Ok(details)
}

// ── Workspace commands ───────────────────────────────────────────────

#[tauri::command]
pub fn create_workspace(
    repo_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<WorkspaceInfo, String> {
    let (repo_path, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), repo.gh_profile.clone())
    };

    let base_branch = detect_default_branch(&repo_path)?;

    // Generate a unique name (retry if branch already exists)
    let mut name = random_workspace_name();
    for attempt in 0..10 {
        let branch = format!("conductor/{}", name);
        let check = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &branch])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?;

        if !check.status.success() {
            break; // branch doesn't exist, good to use
        }

        if attempt == 9 {
            return Err("Could not generate a unique workspace name after 10 attempts".into());
        }

        name = format!(
            "{}-{}",
            random_workspace_name(),
            &Uuid::new_v4().to_string()[..4]
        );
    }

    let id = Uuid::new_v4().to_string();
    let branch = format!("conductor/{}", name);

    // Worktree lives in app data dir, not in the managed repo
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.worktree_dir().join(&id)
    };

    std::fs::create_dir_all(worktree_path.parent().unwrap_or(&worktree_path))
        .map_err(|e| e.to_string())?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &branch])
        .arg(&worktree_path)
        .arg(&base_branch)
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to run git worktree add: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", stderr.trim()));
    }

    let ws = WorkspaceInfo {
        id: id.clone(),
        name,
        branch,
        worktree_path,
        repo_id: repo_id.clone(),
        gh_profile,
        status: WorkspaceStatus::Waiting,
        created_at: now_unix(),
    };

    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.workspaces.insert(id, ws.clone());
    st.save_workspaces()?;

    tracing::info!("Created workspace {} ({})", ws.name, ws.id);
    Ok(ws)
}

#[tauri::command]
pub fn archive_workspace(
    workspace_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let (worktree_path, repo_path) = {
        let mut st = state.lock().map_err(|e| e.to_string())?;

        // Kill agent if running
        if let Some(mut handle) = st.agents.remove(&workspace_id) {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }

        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        if ws.status == WorkspaceStatus::Archived {
            return Ok(());
        }
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), repo.path.clone())
    };

    let output = std::process::Command::new("git")
        .args(["worktree", "remove", "--force"])
        .arg(&worktree_path)
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to remove worktree: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree remove failed: {}", stderr.trim()));
    }

    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(ws) = st.workspaces.get_mut(&workspace_id) {
        ws.status = WorkspaceStatus::Archived;
    }
    st.save_workspaces()?;

    tracing::info!("Archived workspace {}", workspace_id);
    Ok(())
}

#[tauri::command]
pub fn list_workspaces(
    repo_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<WorkspaceInfo>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state
        .workspaces
        .values()
        .filter(|w| w.repo_id == repo_id)
        .cloned()
        .collect())
}

// ── Git commands ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_diff(
    workspace_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, String> {
    let (repo_path, branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), ws.branch.clone())
    };

    let base_branch = detect_default_branch(&repo_path)?;

    let output = std::process::Command::new("git")
        .args(["diff", &format!("{}..{}", base_branch, branch)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ── Script commands ──────────────────────────────────────────────────

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
    state: State<'_, Mutex<AppState>>,
) -> Result<(), String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let mut child = std::process::Command::new("sh")
        .args(["-c", &command])
        .current_dir(&worktree_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run script: {}", e))?;

    let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
    let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;

    std::thread::spawn(move || {
        use std::io::BufRead;

        // Read stdout
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

        // Read remaining stderr
        let mut stderr_buf = String::new();
        let mut stderr_reader = std::io::BufReader::new(stderr);
        let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut stderr_buf);
        if !stderr_buf.is_empty() {
            let _ = on_event.send(ScriptEvent::Output { data: stderr_buf });
        }

        let code = child.wait().ok().and_then(|s| s.code());
        let _ = on_event.send(ScriptEvent::Exit { code });
    });

    Ok(())
}

// ── Message persistence ──────────────────────────────────────────────

#[tauri::command]
pub fn save_messages(
    workspace_id: String,
    messages: serde_json::Value,
    state: State<'_, Mutex<AppState>>,
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
    state: State<'_, Mutex<AppState>>,
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

// ── Agent commands ───────────────────────────────────────────────────

#[tauri::command]
pub fn send_message(
    workspace_id: String,
    prompt: String,
    on_event: Channel<AgentEvent>,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    let (worktree_path, gh_profile, repo_id) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        if st.agents.contains_key(&workspace_id) {
            return Err("Agent is already processing a message".into());
        }
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        if ws.status == WorkspaceStatus::Archived {
            return Err("Cannot send message to archived workspace".into());
        }
        (
            ws.worktree_path.clone(),
            ws.gh_profile.clone(),
            ws.repo_id.clone(),
        )
    };

    // Get session_id for resume (if continuing a conversation)
    let session_id = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.session_ids.get(&workspace_id).cloned()
    };

    // Get GH token per-profile (never switch global auth)
    let gh_token = if let Some(ref profile) = gh_profile {
        let output = std::process::Command::new("gh")
            .args(["auth", "token", "--user", profile])
            .output();
        match output {
            Ok(o) if o.status.success() => {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            }
            _ => {
                tracing::warn!("Could not get GH token for profile {:?}", profile);
                None
            }
        }
    } else {
        None
    };

    // Build claude command
    let mut cmd = std::process::Command::new("claude");
    cmd.arg("-p").arg(&prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    cmd.arg("--dangerously-skip-permissions");

    if let Some(ref sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }

    cmd.current_dir(&worktree_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    if let Some(token) = gh_token {
        cmd.env("GH_TOKEN", token);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn claude: {}", e))?;

    // Take stdout before storing child handle
    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture claude stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture claude stderr")?;

    // Store child handle for stop_agent
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        st.agents
            .insert(workspace_id.clone(), AgentHandle { child });
        if let Some(ws) = st.workspaces.get_mut(&workspace_id) {
            ws.status = WorkspaceStatus::Running;
        }
        st.save_workspaces()?;
    }

    let _ = app.emit(
        "agent-status",
        serde_json::json!({
            "workspace_id": workspace_id,
            "status": "running"
        }),
    );

    // Read stdout in background thread
    let ws_id = workspace_id.clone();
    let app_clone = app.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut new_session_id: Option<String> = None;

        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    parse_stream_line(&line, &on_event, &mut new_session_id);
                }
                Ok(_) => {} // empty line, skip
                Err(e) => {
                    tracing::debug!("stdout read error for {}: {}", ws_id, e);
                    break;
                }
            }
        }

        // Read any stderr for error reporting
        let stderr_output = {
            let mut buf = String::new();
            let mut stderr_reader = std::io::BufReader::new(stderr);
            let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut buf);
            buf
        };

        if !stderr_output.trim().is_empty() {
            tracing::debug!("claude stderr for {}: {}", ws_id, stderr_output.trim());
        }

        // Clean up state
        let state: State<'_, Mutex<AppState>> = app_clone.state();
        if let Ok(mut st) = state.lock() {
            // Wait for child to finish
            if let Some(mut handle) = st.agents.remove(&ws_id) {
                let _ = handle.child.wait();
            }

            // Store session_id for future resume
            if let Some(sid) = new_session_id {
                st.session_ids.insert(ws_id.clone(), sid);
            }

            // Update workspace status
            if let Some(ws) = st.workspaces.get_mut(&ws_id) {
                ws.status = WorkspaceStatus::Waiting;
                let repo_id = ws.repo_id.clone();
                let _ = st.save_workspaces();
            }
        }

        let _ = app_clone.emit(
            "agent-status",
            serde_json::json!({
                "workspace_id": ws_id,
                "status": "waiting"
            }),
        );

        tracing::info!("Agent finished for workspace {}", ws_id);
    });

    tracing::info!("Spawned agent for workspace {}", workspace_id);
    Ok(())
}

#[tauri::command]
pub fn stop_agent(
    workspace_id: String,
    state: State<'_, Mutex<AppState>>,
    app: AppHandle,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let mut handle = st
        .agents
        .remove(&workspace_id)
        .ok_or("No agent running for this workspace")?;

    handle
        .child
        .kill()
        .map_err(|e| format!("Failed to kill agent: {}", e))?;
    let _ = handle.child.wait();

    let repo_id = if let Some(ws) = st.workspaces.get_mut(&workspace_id) {
        ws.status = WorkspaceStatus::Waiting;
        Some(ws.repo_id.clone())
    } else {
        None
    };

    if let Some(repo_id) = repo_id {
        st.save_workspaces()?;
    }

    let _ = app.emit(
        "agent-status",
        serde_json::json!({
            "workspace_id": workspace_id,
            "status": "waiting"
        }),
    );

    tracing::info!("Stopped agent for workspace {}", workspace_id);
    Ok(())
}


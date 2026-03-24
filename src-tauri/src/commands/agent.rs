use crate::state::{AgentHandle, AppState, WorkspaceStatus};
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use super::helpers::{detect_default_branch, get_shell_env, inject_shell_env, strip_ansi};

// ── Agent event types (sent to frontend via Channel) ─────────────────

#[derive(Clone, serde::Serialize)]
pub struct ToolUseInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_string: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_string: Option<String>,
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "assistant_message")]
    AssistantMessage {
        text: String,
        tool_uses: Vec<ToolUseInfo>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thinking: Option<String>,
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
    worktree_path: &str,
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
            let mut thinking_parts = Vec::new();

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

                        let file_path = block
                            .get("input")
                            .and_then(|input| input.get("file_path"))
                            .and_then(|f| f.as_str())
                            .map(|s| {
                                let with_slash = format!("{}/", worktree_path);
                                s.replace(&with_slash, "./").replace(worktree_path, ".")
                            });

                        let input_preview = extract_input_preview(block, &name, worktree_path);

                        // Extract old_string/new_string for Edit tool calls
                        let (old_string, new_string) = if name == "Edit" || name == "edit" {
                            let old = block
                                .get("input")
                                .and_then(|input| input.get("old_string"))
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string());
                            let new = block
                                .get("input")
                                .and_then(|input| input.get("new_string"))
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string());
                            (old, new)
                        } else {
                            (None, None)
                        };

                        // ExitPlanMode carries the full plan in input.plan —
                        // extract it as a text block so the plan renders in the chat.
                        if name == "ExitPlanMode" {
                            if let Some(plan) = block
                                .get("input")
                                .and_then(|input| input.get("plan"))
                                .and_then(|p| p.as_str())
                            {
                                let plan = plan.trim();
                                if !plan.is_empty() {
                                    text_parts.push(plan.to_string());
                                }
                            }
                        }

                        tool_uses.push(ToolUseInfo {
                            name,
                            input_preview,
                            file_path,
                            old_string,
                            new_string,
                        });
                    }
                    Some("thinking") => {
                        if let Some(t) = block.get("thinking").and_then(|t| t.as_str()) {
                            if !t.is_empty() {
                                thinking_parts.push(t.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }

            let text = text_parts.join("\n");
            let thinking = if thinking_parts.is_empty() {
                None
            } else {
                Some(thinking_parts.join("\n"))
            };
            if !text.is_empty() || !tool_uses.is_empty() || thinking.is_some() {
                let _ = on_event.send(AgentEvent::AssistantMessage { text, tool_uses, thinking });
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

/// Extract a human-readable preview from a tool_use input block.
fn extract_input_preview(
    block: &serde_json::Value,
    name: &str,
    worktree_path: &str,
) -> Option<String> {
    block.get("input").and_then(|input| {
        let strip = |s: &str| -> String {
            let with_slash = format!("{}/", worktree_path);
            s.replace(&with_slash, "./").replace(worktree_path, ".")
        };
        // AskUserQuestion: pass the raw questions JSON so the frontend
        // can render interactive options
        if name == "AskUserQuestion" {
            if let Some(questions) = input.get("questions") {
                return Some(questions.to_string());
            }
        }
        // TodoWrite: pass the full todos array for rich progress rendering
        if name == "TodoWrite" {
            if let Some(todos) = input.get("todos") {
                return Some(todos.to_string());
            }
        }
        if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
            Some(strip(fp))
        } else if let Some(cmd) = input.get("command").and_then(|c| c.as_str()) {
            // Strip worktree path AND collapse redundant "cd" prefixes
            let cleaned = strip(cmd);
            let cleaned = if cleaned.starts_with("cd ./ && ") {
                cleaned[9..].to_string()
            } else if cleaned.starts_with("cd . && ") {
                cleaned[8..].to_string()
            } else if cleaned.starts_with("cd ./ ; ") {
                cleaned[8..].to_string()
            } else if cleaned.starts_with("cd . ; ") {
                cleaned[7..].to_string()
            } else if cleaned == "cd ." || cleaned == "cd ./" {
                return None;
            } else {
                cleaned
            };
            Some(cleaned.chars().take(120).collect())
        } else if let Some(pattern) = input.get("pattern").and_then(|p| p.as_str()) {
            Some(strip(pattern))
        } else if let Some(query) = input.get("query").and_then(|q| q.as_str()) {
            Some(strip(query).chars().take(120).collect())
        } else if let Some(desc) = input.get("description").and_then(|d| d.as_str()) {
            Some(strip(desc).chars().take(120).collect())
        } else if let Some(skill) = input.get("skill").and_then(|s| s.as_str()) {
            Some(strip(skill))
        } else if let Some(url) = input.get("url").and_then(|u| u.as_str()) {
            Some(url.chars().take(120).collect())
        } else {
            // Fallback: use first short string value from input
            input.as_object().and_then(|obj| {
                obj.values()
                    .filter_map(|v| v.as_str())
                    .find(|s| !s.is_empty() && s.len() < 200)
                    .map(|s| strip(s).chars().take(120).collect())
            })
        }
    })
}

// ── Agent commands ───────────────────────────────────────────────────

#[tauri::command]
pub fn send_message(
    workspace_id: String,
    prompt: String,
    on_event: Channel<AgentEvent>,
    plan_mode: Option<bool>,
    thinking_mode: Option<bool>,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let plan_mode = plan_mode.unwrap_or(false);
    let thinking_mode = thinking_mode.unwrap_or(false);
    let (worktree_path, gh_profile, repo_id, ws_branch, repo_path, user_system_prompt, context_dir) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        if st.agents.contains_key(&workspace_id) {
            return Err("Agent is already processing a message".into());
        }
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let user_sp = st.repo_settings
            .get(&ws.repo_id)
            .map(|s| s.system_prompt.clone())
            .unwrap_or_default();
        let ctx_dir = st.context_dir(&ws.repo_id);
        (
            ws.worktree_path.clone(),
            repo.gh_profile.clone(),
            ws.repo_id.clone(),
            ws.branch.clone(),
            repo.path.clone(),
            user_sp,
            ctx_dir,
        )
    };

    // Get session_id for resume (if continuing a conversation)
    let session_id = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.session_ids.get(&workspace_id).cloned()
    };

    // Get GH token per-profile (never switch global auth)
    let gh_token = if let Some(ref profile) = gh_profile {
        let mut gh_auth_cmd = std::process::Command::new("gh");
        gh_auth_cmd.args(["auth", "token", "--user", profile]);
        inject_shell_env(&mut gh_auth_cmd);
        let output = gh_auth_cmd.output();
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

    // Prepare MCP config — written to app data dir, not the worktree
    let (mcp_api_port, data_dir) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        (st.mcp_api_port, st.data_dir.clone())
    };

    // Resolve MCP server script: dev source tree first (compile-time, no I/O), then bundled.
    let mcp_dir = data_dir.join("mcp");
    let mcp_server_path = {
        let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("src-mcp")
            .join("server.ts");
        if dev_path.exists() {
            Some(dev_path)
        } else {
            let bundled = mcp_dir.join("server.ts");
            if bundled.exists() { Some(bundled) } else { None }
        }
    };
    let mcp_config_path = if let Some(ref mcp_server_path) = mcp_server_path {
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "korlap": {
                    "type": "stdio",
                    "command": "bun",
                    "args": ["run", mcp_server_path.to_string_lossy()],
                    "env": {
                        "KORLAP_API_PORT": mcp_api_port.to_string(),
                        "KORLAP_WORKSPACE_ID": workspace_id.clone()
                    }
                }
            }
        });
        let _ = std::fs::create_dir_all(&mcp_dir);
        let config_path = mcp_dir.join(format!("{}.json", workspace_id));
        let _ = std::fs::write(
            &config_path,
            serde_json::to_string(&mcp_config).unwrap_or_default(),
        );
        Some(config_path)
    } else {
        None
    };

    // Build claude command — use resolved absolute path when available
    let claude_bin = get_shell_env()
        .claude_path
        .as_deref()
        .unwrap_or("claude");
    let mut cmd = std::process::Command::new(claude_bin);
    cmd.arg("-p").arg(&prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    // Permission mode: plan = read-only, auto = full autonomy within directory bounds
    if plan_mode {
        cmd.args(["--permission-mode", "plan"]);
        cmd.args(["--allowedTools", "mcp__korlap__rename_branch,WebSearch,WebFetch"]);
    } else {
        cmd.args(["--permission-mode", "auto"]);
    }

    // Grant agent access to the images directory so it can read pasted images
    let images_dir = data_dir.join("images");
    cmd.arg("--add-dir").arg(&images_dir);

    // Thinking mode: use high effort for deeper reasoning
    if thinking_mode {
        cmd.args(["--effort", "high"]);
    }

    if let Some(ref sid) = session_id {
        cmd.arg("--resume").arg(sid);
    } else {
        // Inject system prompt only on first message (resume inherits it)
        let base_branch = detect_default_branch(&repo_path)
            .unwrap_or_else(|_| "main".to_string());
        let wt_display = worktree_path.to_string_lossy();
        let repo_display = repo_path.to_string_lossy();
        let mut system_prompt = format!(
            "You are working inside Korlap, a Mac app that runs coding agents in parallel.\n\
             Your working directory is already set to the workspace. Do not cd into it — you are already there.\n\
             Target branch: {ws_branch}\n\
             Base branch: {base_branch}\n\
             \n\
             CRITICAL — workspace isolation:\n\
             • Your workspace is a git worktree at: {wt_display}\n\
             • The main repository lives at: {repo_display} — NEVER read, write, or cd into it.\n\
             • ALL file operations (Read, Edit, Write, Bash) MUST use paths under {wt_display}.\n\
             • The .git file in the worktree references the main repo — that is normal for worktrees. Do NOT follow it.\n\
             • If you discover paths outside {wt_display}, ignore them. You have no business there.\n\
             \n\
             You have access to Korlap tools via MCP. Use the rename_branch tool to give your branch a meaningful name based on the task. Use conventional prefixes: feat/, fix/, refactor/, chore/, docs/. Keep names concise (<30 chars).\n\
             IMPORTANT: Renaming the branch is your FIRST priority. Call rename_branch BEFORE reading files, writing code, or running any commands. Parse the user's request, pick a name, and rename immediately.\n\
             If the task scope changes mid-conversation, rename the branch again to reflect the new direction.\n\
             Keep all changes on the target branch. Do not modify other branches.",
        );
        // Inject warm context from knowledge base (if built)
        if context_dir.exists() {
            let max_context_chars: usize = 20_000;
            let mut injected = 0usize;

            // 1. Invariants — always inject in full (highest priority)
            if let Ok(inv) = std::fs::read_to_string(context_dir.join("invariants.md")) {
                let inv = inv.trim();
                if !inv.is_empty() && injected + inv.len() < max_context_chars {
                    system_prompt.push_str("\n\n## Repository Invariants (MUST follow)\n\n");
                    system_prompt.push_str(inv);
                    injected += inv.len();
                }
            }

            // 2. Hot context — live state (second priority)
            if let Ok(hot) = std::fs::read_to_string(context_dir.join("hot.md")) {
                let hot = hot.trim();
                if !hot.is_empty() && injected + hot.len() < max_context_chars {
                    system_prompt.push_str("\n\n");
                    system_prompt.push_str(hot);
                    injected += hot.len();
                }
            }

            // 3. Facts — abbreviated (third priority)
            if let Ok(facts) = std::fs::read_to_string(context_dir.join("facts.md")) {
                let facts = facts.trim();
                if !facts.is_empty() {
                    let abbreviated: String = facts.lines().take(80).collect::<Vec<_>>().join("\n");
                    if injected + abbreviated.len() < max_context_chars {
                        system_prompt.push_str("\n\n## Repository Facts\n\n");
                        system_prompt.push_str(&abbreviated);
                        injected += abbreviated.len();
                    }
                }
            }

            // 4. Context entries matching files mentioned in the prompt (lowest priority)
            if let Ok(index) = std::fs::read_to_string(context_dir.join("index.md")) {
                if let Ok(context_md) = std::fs::read_to_string(context_dir.join("context.md")) {
                    let relevant_ids = find_relevant_context_ids(&index, &prompt);
                    if !relevant_ids.is_empty() {
                        let relevant = super::context::extract_entries_by_id(&context_md, &relevant_ids);
                        if !relevant.is_empty() && injected + relevant.len() < max_context_chars {
                            system_prompt.push_str("\n\n## Relevant Context\n\n");
                            system_prompt.push_str(&relevant);
                            injected += relevant.len();
                        }
                    }
                }
            }
        }

        if !user_system_prompt.is_empty() {
            system_prompt.push_str("\n\nUser preferences:\n");
            system_prompt.push_str(&user_system_prompt);
        }
        cmd.arg("--system-prompt").arg(&system_prompt);
    }

    // Pass MCP config file to claude
    if let Some(ref config_path) = mcp_config_path {
        cmd.arg("--mcp-config").arg(config_path);
    }

    cmd.current_dir(&worktree_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    // Forward SSH agent and common env for git operations
    inject_shell_env(&mut cmd);

    if let Some(ref token) = gh_token {
        cmd.env("GH_TOKEN", token);
        cmd.env(
            "GIT_CONFIG_PARAMETERS",
            format!(
                "'url.https://oauth2:{}@github.com/.insteadOf=git@github.com:'",
                token
            ),
        );
    }

    // Snapshot main repo status BEFORE agent runs (for post-hoc contamination check)
    let repo_status_before = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&repo_path)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn claude: {}", e))?;

    // Take stdout/stderr before storing child handle
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
    let wt_path_str = worktree_path.to_string_lossy().to_string();
    let repo_path_for_thread = repo_path.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut new_session_id: Option<String> = None;

        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    parse_stream_line(&line, &on_event, &mut new_session_id, &wt_path_str);
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
        let state: State<'_, Arc<Mutex<AppState>>> = app_clone.state();
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

        // Post-hoc contamination check: did the agent modify the main repo?
        let repo_status_after = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&repo_path_for_thread)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();

        if repo_status_after != repo_status_before {
            tracing::error!(
                "CONTAMINATION DETECTED: agent {} modified main repo at {}",
                ws_id,
                repo_path_for_thread.display()
            );
            let _ = app_clone.emit(
                "agent-warning",
                serde_json::json!({
                    "workspace_id": ws_id,
                    "message": format!(
                        "Agent modified files in the main repository at {}. Please review and revert unintended changes.",
                        repo_path_for_thread.display()
                    ),
                }),
            );
        }

        tracing::info!("Agent finished for workspace {}", ws_id);
    });

    tracing::info!("Spawned agent for workspace {}", workspace_id);
    Ok(())
}

#[tauri::command]
pub fn stop_agent(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;

    // Idempotent: if no agent running, just return Ok
    if let Some(mut handle) = st.agents.remove(&workspace_id) {
        let _ = handle.child.kill();
        let _ = handle.child.wait();
    }

    if let Some(ws) = st.workspaces.get_mut(&workspace_id) {
        ws.status = WorkspaceStatus::Waiting;
    }
    st.save_workspaces()?;

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

// ── Warm context helpers ─────────────────────────────────────────────

/// Parse the file affinity section of index.md and find context entry IDs
/// relevant to file paths mentioned in the prompt.
fn find_relevant_context_ids(index_content: &str, prompt: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_affinity = false;

    for line in index_content.lines() {
        if line.contains("## File affinity") {
            in_affinity = true;
            continue;
        }
        if in_affinity {
            if line.starts_with("## ") {
                break;
            }
            // Format: "src/auth/*       → auth-a3f8c2"
            if let Some((glob_part, entry_id)) = line.split_once('→') {
                let glob_base = glob_part.trim().trim_end_matches('*').trim_end_matches('/');
                let entry_id = entry_id.trim().to_string();
                // Check if any path fragment from the affinity appears in the prompt
                if !glob_base.is_empty() && prompt.contains(glob_base) && !ids.contains(&entry_id)
                {
                    ids.push(entry_id);
                }
            }
        }
    }
    ids
}

// ── AI-powered utilities ─────────────────────────────────────────────

#[tauri::command]
pub async fn generate_commit_message(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let (worktree_path, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo.default_branch.clone().unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base)
    };

    tauri::async_runtime::spawn_blocking(move || {
        // Stage everything first
        let _ = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&worktree_path)
            .output();

        // Get staged diff
        let diff_output = std::process::Command::new("git")
            .args(["diff", "--cached"])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to get diff: {}", e))?;

        let mut diff = String::from_utf8_lossy(&diff_output.stdout).to_string();

        // If diff is too large, fall back to stat summary
        if diff.len() > 50_000 {
            let stat_output = std::process::Command::new("git")
                .args(["diff", "--cached", "--stat"])
                .current_dir(&worktree_path)
                .output()
                .map_err(|e| format!("Failed to get diff stat: {}", e))?;
            diff = format!(
                "[Diff too large for full context. Summary:]\n{}",
                String::from_utf8_lossy(&stat_output.stdout)
            );
        }

        // Also get the log of commits vs base for context
        let log_output = std::process::Command::new("git")
            .args(["log", "--oneline", &format!("origin/{}..HEAD", base_branch)])
            .current_dir(&worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();

        let prompt = if log_output.is_empty() {
            format!(
                "Write a conventional commit message for this diff. First line under 72 chars. \
                 Use conventional commit format (feat:, fix:, chore:, etc). \
                 Output ONLY the commit message, nothing else.\n\n{}",
                diff
            )
        } else {
            format!(
                "Write a conventional commit message for the latest staged changes. \
                 First line under 72 chars. Use conventional commit format (feat:, fix:, chore:, etc). \
                 Output ONLY the commit message, nothing else.\n\n\
                 Previous commits on this branch:\n{}\n\nStaged diff:\n{}",
                log_output, diff
            )
        };

        // Spawn claude CLI for one-shot message generation
        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.current_dir(&worktree_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd.spawn().map_err(|e| format!("Failed to spawn claude: {}", e))?;

        // Wait with 30s timeout
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Ok(output)) if output.status.success() => {
                let msg = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if msg.is_empty() {
                    Ok("chore: update files".to_string())
                } else {
                    Ok(msg)
                }
            }
            Ok(Ok(_output)) => {
                tracing::warn!("claude exited with non-zero for commit message generation");
                Ok("chore: update files".to_string())
            }
            Ok(Err(e)) => {
                tracing::warn!("claude failed for commit message: {}", e);
                Ok("chore: update files".to_string())
            }
            Err(_) => {
                tracing::warn!("claude timed out generating commit message");
                drop(handle);
                Ok("chore: update files".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn suggest_replies(text: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let prompt = format!(
            "Given this AI assistant message, suggest 2-4 short reply options that a user would \
             likely want to send back. Return ONLY a JSON array of short strings, nothing else. \
             Example: [\"Yes\", \"No, skip this\"]\n\nMessage:\n{}",
            text
        );

        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--model", "claude-haiku-4-5-20251001"]);
        cmd.args(["--max-turns", "1"]);
        cmd.args(["--max-tokens", "200"]);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // Strip markdown fences if the model wraps the JSON
                let json_str = raw
                    .strip_prefix("```json")
                    .or_else(|| raw.strip_prefix("```"))
                    .and_then(|s| s.strip_suffix("```"))
                    .map(|s| s.trim())
                    .unwrap_or(&raw);
                let suggestions: Vec<String> = serde_json::from_str(json_str)
                    .map_err(|e| format!("Failed to parse suggestions: {} — raw: {}", e, raw))?;
                Ok(suggestions)
            }
            Ok(Ok(_)) => Err("Claude exited with non-zero status".to_string()),
            Ok(Err(e)) => Err(format!("Claude failed: {}", e)),
            Err(_) => {
                // Kill the orphaned process tree to avoid leaks
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                Err("Timed out generating suggestions".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Autopilot utilities ──────────────────────────────────────────────

/// Strip markdown code fences from a JSON response.
fn strip_json_fences(raw: &str) -> &str {
    raw.strip_prefix("```json")
        .or_else(|| raw.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .map(|s| s.trim())
        .unwrap_or(raw)
}

#[tauri::command]
pub async fn prioritize_todos(
    todo_json: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<String>, String> {
    let data_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.data_dir.clone()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let system_prompt = "You are a task scheduler. Analyze these TODO items and return them \
            ordered by priority, considering dependencies. Return ONLY a JSON array of the todo \
            IDs in execution order. Example: [\"id1\", \"id2\", \"id3\"]";

        let prompt = todo_json;

        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--max-turns", "1"]);
        cmd.arg("--system-prompt").arg(system_prompt);
        cmd.current_dir(&data_dir);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = strip_ansi(
                    &String::from_utf8_lossy(&output.stdout).trim().to_string(),
                );
                let json_str = strip_json_fences(&raw);
                let ids: Vec<String> = serde_json::from_str(json_str)
                    .map_err(|e| format!("Failed to parse priority list: {} — raw: {}", e, raw))?;
                Ok(ids)
            }
            Ok(Ok(_)) => Err("Claude exited with non-zero status".to_string()),
            Ok(Err(e)) => Err(format!("Claude failed: {}", e)),
            Err(_) => {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                Err("Timed out prioritizing todos".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn determine_dependencies(
    todo_json: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let data_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.data_dir.clone()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let system_prompt = "You are a task dependency analyzer for a software project. Given a list of TODO items \
            (each with id, title, description), determine which tasks depend on other tasks. Task A depends on task B \
            if A requires B's code changes to exist first — e.g., \"write API tests\" depends on \"build the API endpoint\". \
            Only include direct dependencies, not transitive ones. Return ONLY a JSON object mapping task IDs to arrays \
            of dependency task IDs. Only include entries for tasks that have dependencies. \
            Example: {\"id2\": [\"id1\"], \"id3\": [\"id1\"]}. If no dependencies exist, return: {}";

        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&todo_json);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--max-turns", "1"]);
        cmd.arg("--system-prompt").arg(system_prompt);
        cmd.current_dir(&data_dir);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = strip_ansi(
                    &String::from_utf8_lossy(&output.stdout).trim().to_string(),
                );
                let json_str = strip_json_fences(&raw);
                // Validate it's valid JSON before returning
                let _: serde_json::Value = serde_json::from_str(json_str)
                    .map_err(|e| format!("Failed to parse dependencies: {} — raw: {}", e, raw))?;
                Ok(json_str.to_string())
            }
            Ok(Ok(_)) => Err("Claude exited with non-zero status".to_string()),
            Ok(Err(e)) => Err(format!("Claude failed: {}", e)),
            Err(_) => {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                Err("Timed out determining dependencies".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AutopilotAction {
    pub response: String,
    pub action_type: String,
    pub todo_ids: Vec<String>,
    pub reorder: Vec<String>,
}

#[tauri::command]
pub async fn interpret_autopilot_command(
    command: String,
    context_json: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<AutopilotAction, String> {
    let data_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.data_dir.clone()
    };

    tauri::async_runtime::spawn_blocking(move || {
        let system_prompt = "You are an autopilot orchestrator for a coding agent system. \
            The user may ask questions about workspace status OR give commands to manage automated TODO pickup. \
            Available action_types: 'pause', 'resume', 'skip_todo', 'prioritize', 'none'. \
            Use 'none' for status questions, informational queries, or anything that doesn't require an action. \
            You MUST ALWAYS respond with ONLY valid JSON — no markdown, no explanation outside JSON: \
            {\"response\": \"<human-readable reply>\", \"action_type\": \"<type>\", \
            \"todo_ids\": [\"<affected ids if applicable>\"], \"reorder\": [\"<new order if applicable>\"]}";

        let prompt = format!("Command: {}\n\nCurrent state:\n{}", command, context_json);

        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--max-turns", "1"]);
        cmd.arg("--system-prompt").arg(system_prompt);
        cmd.current_dir(&data_dir);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn claude: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = strip_ansi(
                    &String::from_utf8_lossy(&output.stdout).trim().to_string(),
                );
                let json_str = strip_json_fences(&raw);
                let action: AutopilotAction = serde_json::from_str(json_str)
                    .unwrap_or_else(|_| AutopilotAction {
                        response: raw,
                        action_type: "none".to_string(),
                        todo_ids: vec![],
                        reorder: vec![],
                    });
                Ok(action)
            }
            Ok(Ok(_)) => Err("Claude exited with non-zero status".to_string()),
            Ok(Err(e)) => Err(format!("Claude failed: {}", e)),
            Err(_) => {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                Err("Timed out interpreting autopilot command".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

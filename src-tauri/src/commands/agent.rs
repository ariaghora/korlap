use crate::git_provider::SharedProviderRegistry;
use crate::state::{effective_provider, AgentHandle, AgentProvider, AppState, SourcePr, WorkspaceStatus};
use std::io::BufRead;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};

use super::agent_backend::{
    build_mcp_server_map, claude_extract_usage, codex_extract_usage, codex_unregister_mcp_servers,
    get_backend, ParsedEvent, SessionContext,
};
use super::helpers::{detect_default_branch, get_shell_env, inject_shell_env, strip_ansi};

// ── Available models ─────────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct ModelOption {
    pub value: String,
    pub label: String,
}

#[tauri::command]
pub fn list_models(
    repo_id: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<ModelOption>, String> {
    let provider = if let Some(ref repo_id) = repo_id {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.repo_settings
            .get(repo_id)
            .map(|s| s.agent_provider)
            .unwrap_or_default()
    } else {
        AgentProvider::default()
    };
    Ok(get_backend(provider).list_models())
}

#[derive(Clone, serde::Serialize)]
pub struct ProviderInfo {
    pub provider: AgentProvider,
    pub supports_thinking: bool,
    pub supports_plan_mode: bool,
    pub models: Vec<ModelOption>,
}

#[tauri::command]
pub fn get_provider_info(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ProviderInfo, String> {
    let provider = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.repo_settings
            .get(&repo_id)
            .map(|s| s.agent_provider)
            .unwrap_or_default()
    };
    let backend = get_backend(provider);
    Ok(ProviderInfo {
        provider,
        supports_thinking: backend.supports_thinking(),
        supports_plan_mode: backend.supports_plan_mode(),
        models: backend.list_models(),
    })
}

#[tauri::command]
pub fn get_workspace_provider_info(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ProviderInfo, String> {
    let provider = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let settings = st.repo_settings.get(&ws.repo_id);
        let default_settings = crate::state::RepoSettings::default();
        effective_provider(ws, settings.unwrap_or(&default_settings))
    };
    let backend = get_backend(provider);
    Ok(ProviderInfo {
        provider,
        supports_thinking: backend.supports_thinking(),
        supports_plan_mode: backend.supports_plan_mode(),
        models: backend.list_models(),
    })
}

#[tauri::command]
pub fn switch_workspace_provider(
    workspace_id: String,
    provider: AgentProvider,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;

    // Don't allow switching while agent is running
    if st.agents.contains_key(&workspace_id) {
        return Err("Cannot switch provider while agent is running. Stop the agent first.".into());
    }

    let ws = st
        .workspaces
        .get_mut(&workspace_id)
        .ok_or("Workspace not found")?;
    ws.provider_override = Some(provider);

    // Clear session — the new provider can't resume the old one
    st.session_ids.remove(&workspace_id);

    st.save_workspaces()?;
    tracing::info!(
        "Switched workspace {} to provider {:?}",
        workspace_id,
        provider
    );
    Ok(())
}

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
    #[serde(rename = "usage")]
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        /// true = cumulative session total (from result event), replaces previous count
        /// false = single API call (from assistant event), added to running total
        cumulative: bool,
    },
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "error")]
    #[allow(dead_code)]
    Error { message: String },
}

// ── Agent commands ───────────────────────────────────────────────────

#[tauri::command]
pub fn send_message(
    workspace_id: String,
    prompt: String,
    on_event: Channel<AgentEvent>,
    plan_mode: Option<bool>,
    thinking_mode: Option<bool>,
    model: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
    app: AppHandle,
) -> Result<(), String> {
    let plan_mode = plan_mode.unwrap_or(false);
    let thinking_mode = thinking_mode.unwrap_or(false);

    // Extract all needed data from state in one lock
    let (
        worktree_path,
        gh_profile,
        ws_branch,
        repo_path,
        user_system_prompt,
        context_dir,
        is_custom_branch,
        user_mcp_servers,
        provider,
        mcp_api_port,
        data_dir,
        session_id,
        source_pr,
    ) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        if st.agents.contains_key(&workspace_id) {
            return Err("Agent is already processing a message".into());
        }
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let default_settings = crate::state::RepoSettings::default();
        let repo_settings = st.repo_settings.get(&ws.repo_id);
        let settings = repo_settings.unwrap_or(&default_settings);
        let user_sp = settings.system_prompt.clone();
        let mcp_servers = settings.mcp_servers.clone();
        let prov = effective_provider(ws, settings);
        let ctx_dir = st.context_dir(&ws.repo_id);
        let sid = st.session_ids.get(&workspace_id).cloned();
        (
            ws.worktree_path.clone(),
            repo.gh_profile.clone(),
            ws.branch.clone(),
            repo.path.clone(),
            user_sp,
            ctx_dir,
            ws.custom_branch,
            mcp_servers,
            prov,
            st.mcp_api_port,
            st.data_dir.clone(),
            sid,
            ws.source_pr.clone(),
        )
    };

    let backend = get_backend(provider);

    // Get token per-profile via the provider (never switch global auth)
    let git_provider = providers.for_repo(&repo_path);
    let gh_token = git_provider.resolve_token(&gh_profile);
    let git_auth_env = git_provider.build_auth_env_vars(&gh_token);

    // Resolve MCP server script path
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
            if bundled.exists() {
                Some(bundled)
            } else {
                None
            }
        }
    };

    // Build merged MCP server map (shared across providers)
    let mcp_servers = build_mcp_server_map(
        mcp_server_path.as_deref(),
        mcp_api_port,
        &workspace_id,
        plan_mode,
        &user_mcp_servers,
    );

    // Build system prompt (only on first message — resume inherits it)
    let system_prompt = if session_id.is_none() {
        Some(build_system_prompt(
            &worktree_path,
            &repo_path,
            &ws_branch,
            is_custom_branch,
            &context_dir,
            &prompt,
            &user_system_prompt,
            &source_pr,
        ))
    } else {
        None
    };

    let images_dir = data_dir.join("images");
    let ctx = SessionContext {
        prompt,
        worktree_path: worktree_path.clone(),
        repo_path,
        session_id,
        plan_mode,
        thinking_mode,
        model,
        system_prompt,
        mcp_servers,
        mcp_dir,
        workspace_id: workspace_id.clone(),
        images_dir,
        git_auth_env,
        disallowed_tools: super::agent_backend::DISALLOWED_WORKTREE_TOOLS,
    };

    let (mut cmd, mcp_cleanup_names) = backend.build_session_command(&ctx)?;
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn {}: {}", backend.binary_name(), e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("Failed to capture {} stdout", backend.binary_name()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("Failed to capture {} stderr", backend.binary_name()))?;

    // Store child handle
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

    // Read stdout in background thread — provider-agnostic via backend.parse_stream_line()
    let ws_id = workspace_id.clone();
    let app_clone = app.clone();
    let wt_path_str = worktree_path.to_string_lossy().to_string();
    let is_claude = provider == AgentProvider::Claude;
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut new_session_id: Option<String> = None;
        let mut any_event_sent = false;

        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    // Use the backend's parser
                    if let Some(event) = backend.parse_stream_line(&line, &wt_path_str) {
                        any_event_sent = true;
                        match event {
                            ParsedEvent::SessionId(sid) => {
                                new_session_id = Some(sid);
                            }
                            ParsedEvent::AssistantMessage {
                                text,
                                tool_uses,
                                thinking,
                            } => {
                                let _ = on_event.send(AgentEvent::AssistantMessage {
                                    text,
                                    tool_uses,
                                    thinking,
                                });
                            }
                            ParsedEvent::Usage {
                                input_tokens,
                                output_tokens,
                                cumulative,
                            } => {
                                let _ = on_event.send(AgentEvent::Usage {
                                    input_tokens,
                                    output_tokens,
                                    cumulative,
                                });
                            }
                            ParsedEvent::Done => {
                                let _ = on_event.send(AgentEvent::Done);
                            }
                        }
                    }

                    // Provider-specific: extract usage and session info that the
                    // trait parser doesn't emit (to keep ParsedEvent simple).
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        let msg_type = v.get("type").and_then(|t| t.as_str());
                        if is_claude {
                            match msg_type {
                                Some("assistant") => {
                                    if let Some(usage) =
                                        v.get("message").and_then(|m| m.get("usage"))
                                    {
                                        let (input, output) = claude_extract_usage(usage);
                                        if input > 0 || output > 0 {
                                            let _ = on_event.send(AgentEvent::Usage {
                                                input_tokens: input,
                                                output_tokens: output,
                                                cumulative: false,
                                            });
                                        }
                                    }
                                }
                                Some("result") => {
                                    if let Some(sid) =
                                        v.get("session_id").and_then(|s| s.as_str())
                                    {
                                        new_session_id = Some(sid.to_string());
                                    }
                                    if let Some(usage) = v.get("usage") {
                                        let (input, output) = claude_extract_usage(usage);
                                        if input > 0 || output > 0 {
                                            let _ = on_event.send(AgentEvent::Usage {
                                                input_tokens: input,
                                                output_tokens: output,
                                                cumulative: true,
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            // Codex: extract usage from turn.completed
                            if msg_type == Some("turn.completed") {
                                let (input, output) = codex_extract_usage(&v);
                                if input > 0 || output > 0 {
                                    let _ = on_event.send(AgentEvent::Usage {
                                        input_tokens: input,
                                        output_tokens: output,
                                        cumulative: true,
                                    });
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("stdout read error for {}: {}", ws_id, e);
                    break;
                }
            }
        }

        // Read stderr and surface errors to the user
        let stderr_output = {
            let mut buf = String::new();
            let mut stderr_reader = std::io::BufReader::new(stderr);
            let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut buf);
            buf
        };
        let stderr_trimmed = stderr_output.trim();
        if !stderr_trimmed.is_empty() {
            tracing::warn!("{} stderr for {}: {}", backend.binary_name(), ws_id, stderr_trimmed);
        }

        // If we got no events and there's stderr output, show it as an error in chat
        if !any_event_sent {
            let error_msg = if !stderr_trimmed.is_empty() {
                format!("{} error: {}", backend.binary_name(), stderr_trimmed)
            } else {
                format!("{} exited without producing any output", backend.binary_name())
            };
            let _ = on_event.send(AgentEvent::AssistantMessage {
                text: error_msg,
                tool_uses: vec![],
                thinking: None,
            });
            let _ = on_event.send(AgentEvent::Done);
        }

        // Clean up state
        let state: State<'_, Arc<Mutex<AppState>>> = app_clone.state();
        if let Ok(mut st) = state.lock() {
            if let Some(mut handle) = st.agents.remove(&ws_id) {
                let _ = handle.child.wait();
            }
            if let Some(sid) = new_session_id {
                st.session_ids.insert(ws_id.clone(), sid);
            }
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

        // Clean up Codex MCP server registrations from global config
        if !mcp_cleanup_names.is_empty() {
            let codex_bin = get_shell_env()
                .codex_path
                .as_deref()
                .unwrap_or("codex");
            codex_unregister_mcp_servers(codex_bin, &mcp_cleanup_names);
        }

        tracing::info!("Agent finished for workspace {}", ws_id);
    });

    tracing::info!("Spawned {} agent for workspace {}", backend.binary_name(), workspace_id);
    Ok(())
}

/// Build the system prompt for a new session (shared across providers).
fn build_system_prompt(
    worktree_path: &Path,
    repo_path: &Path,
    ws_branch: &str,
    is_custom_branch: bool,
    context_dir: &Path,
    prompt: &str,
    user_system_prompt: &str,
    source_pr: &Option<SourcePr>,
) -> String {
    let base_branch = if let Some(ref pr) = source_pr {
        pr.base_branch.clone()
    } else {
        detect_default_branch(repo_path).unwrap_or_else(|_| "main".to_string())
    };
    let wt_display = worktree_path.to_string_lossy();
    let repo_display = repo_path.to_string_lossy();
    let rename_instruction = if !is_custom_branch {
        "FIRST PRIORITY: Call rename_branch immediately with a conventional name (feat/, fix/, refactor/, chore/). \
         Rename again if scope changes."
    } else {
        "Branch was manually named. Do NOT rename unless explicitly asked."
    };
    let mut system_prompt = format!(
        "You are a coding agent inside Korlap, working in a git worktree.\n\
         Worktree: {wt_display} | Main repo: {repo_display} (DO NOT access)\n\
         Target branch: {ws_branch} | Base branch: {base_branch}\n\
         \n\
         ISOLATION: All file operations MUST stay under {wt_display}. \
         EnterWorktree/ExitWorktree are DISABLED. Do not use Agent with isolation:\"worktree\". \
         The .git file referencing the main repo is normal for worktrees — ignore it.\n\
         \n\
         {rename_instruction}\n\
         Keep all changes on target branch.\n\
         \n\
         LSP tools available (prefer over grep for precise navigation, 1-based positions):\n\
         lsp_goto_definition, lsp_find_references, lsp_hover, lsp_workspace_symbols, lsp_diagnostics, lsp_rename\n\
         After edits, call lsp_diagnostics. For renames, prefer lsp_rename over find-and-replace.\n\
         \n\
         TOKEN EFFICIENCY — minimize tool output to save context:\n\
         • Bash: pipe verbose commands through `head -80` or `tail -40` (e.g. `cargo check 2>&1 | head -80`)\n\
         • Read: use offset/limit to read only the section you need, not entire files\n\
         • Grep: set head_limit to cap results (e.g. 20). Never run unbounded searches.\n\
         • Git: use `git diff --stat` first; only read full diff for files you need to change\n\
         • Avoid re-reading files you already have in context",
    );

    // Inject warm context from knowledge base (if built)
    if context_dir.exists() {
        let max_context_chars: usize = 20_000;
        let mut injected = 0usize;

        if let Ok(inv) = std::fs::read_to_string(context_dir.join("invariants.md")) {
            let inv = inv.trim();
            if !inv.is_empty() && injected + inv.len() < max_context_chars {
                system_prompt.push_str("\n\n## Repository Invariants (MUST follow)\n\n");
                system_prompt.push_str(inv);
                injected += inv.len();
            }
        }

        if let Ok(hot) = std::fs::read_to_string(context_dir.join("hot.md")) {
            let hot = hot.trim();
            if !hot.is_empty() && injected + hot.len() < max_context_chars {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(hot);
                injected += hot.len();
            }
        }

        if let Ok(facts) = std::fs::read_to_string(context_dir.join("facts.md")) {
            let facts = facts.trim();
            if !facts.is_empty() {
                let abbreviated: String =
                    facts.lines().take(80).collect::<Vec<_>>().join("\n");
                if injected + abbreviated.len() < max_context_chars {
                    system_prompt.push_str("\n\n## Repository Facts\n\n");
                    system_prompt.push_str(&abbreviated);
                    injected += abbreviated.len();
                }
            }
        }

        if let Ok(index) = std::fs::read_to_string(context_dir.join("index.md")) {
            if let Ok(context_md) = std::fs::read_to_string(context_dir.join("context.md")) {
                let relevant_ids = find_relevant_context_ids(&index, prompt);
                if !relevant_ids.is_empty() {
                    let relevant =
                        super::context::extract_entries_by_id(&context_md, &relevant_ids);
                    if !relevant.is_empty() && injected + relevant.len() < max_context_chars {
                        system_prompt.push_str("\n\n## Relevant Context\n\n");
                        system_prompt.push_str(&relevant);
                    }
                }
            }
        }
    }

    // Inject PR review context if this workspace was created from a PR
    if let Some(ref pr) = source_pr {
        system_prompt.push_str(&format!(
            "\n\n## PR Review Context\n\n\
             You are reviewing PR #{}: {}\n\
             PR URL: {}\n\
             PR branch: {} → {}\n\n\
             This workspace contains the PR's code branched from its HEAD.\n\
             Your task is to review, test, and identify issues in this PR.\n\
             You can make changes, add tests, and fix issues. Commits stay on your review branch \
             and will NOT automatically update the PR.",
            pr.number, pr.title, pr.url, pr.branch, pr.base_branch,
        ));
    }

    if !user_system_prompt.is_empty() {
        system_prompt.push_str("\n\nUser preferences:\n");
        system_prompt.push_str(user_system_prompt);
    }

    system_prompt
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
        cmd.args(["--model", "claude-haiku-4-5-20251001"]);
        cmd.args(["--max-turns", "1"]);
        cmd.args(["--disallowedTools", super::agent_backend::DISALLOWED_WORKTREE_TOOLS]);
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
        cmd.args(["--model", "claude-haiku-4-5-20251001"]);
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
        cmd.args(["--model", "claude-haiku-4-5-20251001"]);
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
        cmd.args(["--model", "claude-haiku-4-5-20251001"]);
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

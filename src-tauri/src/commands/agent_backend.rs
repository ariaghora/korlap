use crate::commands::agent::{ModelOption, ToolUseInfo};
use crate::commands::helpers::{get_shell_env, inject_shell_env};
use crate::state::{AgentProvider, McpServerConfig};
use std::path::{Path, PathBuf};

// ── Shared types ─────────────────────────────────────────────────────

/// Everything a backend needs to build a session command.
pub struct SessionContext {
    pub prompt: String,
    pub worktree_path: PathBuf,
    pub repo_path: PathBuf,
    pub session_id: Option<String>,
    pub plan_mode: bool,
    pub thinking_mode: bool,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub mcp_servers: serde_json::Map<String, serde_json::Value>,
    pub mcp_dir: PathBuf,
    pub workspace_id: String,
    pub images_dir: PathBuf,
    /// Provider-agnostic auth env vars (e.g. GH_TOKEN, GIT_CONFIG_PARAMETERS).
    /// Built by the git service provider so agent processes authenticate correctly.
    pub git_auth_env: Vec<(String, String)>,
    pub disallowed_tools: &'static str,
}

/// Provider-agnostic parsed event from a single NDJSON line.
pub enum ParsedEvent {
    SessionId(String),
    AssistantMessage {
        text: String,
        tool_uses: Vec<ToolUseInfo>,
        thinking: Option<String>,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        cumulative: bool,
    },
    Done,
}

// ── Trait ─────────────────────────────────────────────────────────────

pub trait AgentBackend: Send + Sync {
    /// Build a Command for a streaming agent session.
    /// Returns (Command, cleanup_names) where cleanup_names are MCP server names
    /// to unregister after the process exits (Codex-specific, empty for Claude).
    fn build_session_command(
        &self,
        ctx: &SessionContext,
    ) -> Result<(std::process::Command, Vec<String>), String>;

    /// Parse one line of streaming output into a provider-agnostic event.
    fn parse_stream_line(
        &self,
        line: &str,
        worktree_path: &str,
    ) -> Option<ParsedEvent>;

    /// Available models for the model picker.
    fn list_models(&self) -> Vec<ModelOption>;

    /// Whether this provider supports extended thinking / reasoning display.
    fn supports_thinking(&self) -> bool;

    /// Whether this provider supports a read-only / plan mode.
    fn supports_plan_mode(&self) -> bool;

    /// CLI binary name (for error messages).
    fn binary_name(&self) -> &'static str;
}

pub fn get_backend(provider: AgentProvider) -> &'static dyn AgentBackend {
    match provider {
        AgentProvider::Claude => &ClaudeBackend,
        AgentProvider::Codex => &CodexBackend,
    }
}

// ── Shared helpers ───────────────────────────────────────────────────

/// Build the merged MCP server map (built-in korlap + user 3rd-party servers).
/// Returns a `serde_json::Map` ready for both Claude (JSON file) and Codex (TOML).
pub fn build_mcp_server_map(
    mcp_server_path: Option<&Path>,
    mcp_api_port: u16,
    workspace_id: &str,
    plan_mode: bool,
    user_mcp_servers: &std::collections::HashMap<String, McpServerConfig>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut servers = serde_json::Map::new();

    if let Some(mcp_server_path) = mcp_server_path {
        servers.insert(
            "korlap".to_string(),
            serde_json::json!({
                "type": "stdio",
                "command": "bun",
                "args": ["run", mcp_server_path.to_string_lossy()],
                "env": {
                    "KORLAP_API_PORT": mcp_api_port.to_string(),
                    "KORLAP_WORKSPACE_ID": workspace_id
                }
            }),
        );
    }

    // User-configured 3rd-party MCP servers (work mode only)
    if !plan_mode {
        for (name, config) in user_mcp_servers {
            if name == "korlap" {
                continue;
            }
            let entry = if config.server_type == "sse" {
                if config.url.is_empty() {
                    tracing::warn!("MCP server '{}' has empty URL, skipping", name);
                    continue;
                }
                let mut entry = serde_json::json!({ "type": "sse", "url": config.url });
                if !config.headers.is_empty() {
                    if let Ok(h) = serde_json::to_value(&config.headers) {
                        entry["headers"] = h;
                    }
                }
                entry
            } else {
                if config.command.is_empty() {
                    tracing::warn!("MCP server '{}' has empty command, skipping", name);
                    continue;
                }
                let mut entry = serde_json::json!({
                    "type": "stdio",
                    "command": config.command,
                    "args": config.args
                });
                if !config.env.is_empty() {
                    if let Ok(env_val) = serde_json::to_value(&config.env) {
                        entry["env"] = env_val;
                    }
                }
                entry
            };
            servers.insert(name.clone(), entry);
        }
    }

    servers
}

/// Strip worktree path from a file path for display purposes.
fn strip_worktree_prefix(s: &str, worktree_path: &str) -> String {
    let with_slash = format!("{}/", worktree_path);
    s.replace(&with_slash, "./").replace(worktree_path, ".")
}

// ── Claude backend ───────────────────────────────────────────────────

struct ClaudeBackend;

/// Tools blocked to prevent agent from escaping Korlap worktree isolation.
/// EnterWorktree creates worktrees from origin/<default> of the MAIN repo,
/// completely bypassing workspace isolation. LSP is disabled because Korlap
/// manages shared LSP servers centrally.
pub const DISALLOWED_WORKTREE_TOOLS: &str = "EnterWorktree,ExitWorktree,LSP";

impl AgentBackend for ClaudeBackend {
    fn build_session_command(
        &self,
        ctx: &SessionContext,
    ) -> Result<(std::process::Command, Vec<String>), String> {
        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&ctx.prompt);
        cmd.args(["--output-format", "stream-json", "--verbose"]);

        if ctx.plan_mode {
            cmd.args(["--permission-mode", "plan"]);
            cmd.args([
                "--allowedTools",
                "mcp__korlap__rename_branch,WebSearch,WebFetch,\
                mcp__korlap__lsp_goto_definition,mcp__korlap__lsp_find_references,\
                mcp__korlap__lsp_hover,mcp__korlap__lsp_workspace_symbols,\
                mcp__korlap__lsp_diagnostics",
            ]);
        } else {
            cmd.args(["--permission-mode", "bypassPermissions"]);
            cmd.args(["--disallowedTools", ctx.disallowed_tools]);
        }

        // Grant agent access to images directory
        cmd.arg("--add-dir").arg(&ctx.images_dir);

        // Model override
        if let Some(ref model_id) = ctx.model {
            if !model_id.is_empty() {
                cmd.args(["--model", model_id]);
            }
        }

        // Thinking mode
        if ctx.thinking_mode {
            cmd.args(["--effort", "high"]);
        }

        if let Some(ref sid) = ctx.session_id {
            cmd.arg("--resume").arg(sid);
        } else if let Some(ref sp) = ctx.system_prompt {
            cmd.arg("--system-prompt").arg(sp);
        }

        // Write MCP config JSON file and pass to claude
        if !ctx.mcp_servers.is_empty() {
            let mcp_config = serde_json::json!({ "mcpServers": ctx.mcp_servers });
            let _ = std::fs::create_dir_all(&ctx.mcp_dir);
            let config_path = ctx.mcp_dir.join(format!("{}.json", ctx.workspace_id));
            let _ = std::fs::write(
                &config_path,
                serde_json::to_string(&mcp_config).unwrap_or_default(),
            );
            cmd.arg("--mcp-config").arg(&config_path);
        }

        cmd.current_dir(&ctx.worktree_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        inject_shell_env(&mut cmd);

        for (key, value) in &ctx.git_auth_env {
            cmd.env(key, value);
        }

        Ok((cmd, Vec::new()))
    }

    fn parse_stream_line(
        &self,
        line: &str,
        worktree_path: &str,
    ) -> Option<ParsedEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let msg_type = v.get("type").and_then(|t| t.as_str())?;

        match msg_type {
            "system" => {
                let sid = v.get("session_id").and_then(|s| s.as_str())?;
                Some(ParsedEvent::SessionId(sid.to_string()))
            }
            "assistant" => {
                let message = v.get("message")?;
                let content = message.get("content").and_then(|c| c.as_array())?;

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
                                .map(|s| strip_worktree_prefix(s, worktree_path));

                            let input_preview =
                                claude_extract_input_preview(block, &name, worktree_path);

                            let (old_string, new_string) =
                                if name == "Edit" || name == "edit" {
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

                            // ExitPlanMode carries the full plan in input.plan
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
                            if let Some(t) =
                                block.get("thinking").and_then(|t| t.as_str())
                            {
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

                // Emit assistant message if there's anything to show
                let has_content =
                    !text.is_empty() || !tool_uses.is_empty() || thinking.is_some();
                if !has_content {
                    // Check for usage on this message even if no content
                    if let Some(usage) = message.get("usage") {
                        let (input, output) = claude_extract_usage(usage);
                        if input > 0 || output > 0 {
                            return Some(ParsedEvent::Usage {
                                input_tokens: input,
                                output_tokens: output,
                                cumulative: false,
                            });
                        }
                    }
                    return None;
                }

                // We need to return both the message and usage. Since ParsedEvent
                // is a single enum, we handle usage separately — the caller will
                // re-parse the line. Instead, let's just return the message here.
                // Usage from assistant events is handled by the caller checking
                // message.usage after getting AssistantMessage.
                Some(ParsedEvent::AssistantMessage {
                    text,
                    tool_uses,
                    thinking,
                })
            }
            "result" => {
                // Result event carries cumulative session usage + session_id
                // We return Done here; session_id and usage are extracted by
                // the caller via dedicated helpers.
                Some(ParsedEvent::Done)
            }
            _ => None,
        }
    }

    fn list_models(&self) -> Vec<ModelOption> {
        vec![
            ModelOption {
                value: String::new(),
                label: "Default".into(),
            },
            ModelOption {
                value: "sonnet".into(),
                label: "Sonnet".into(),
            },
            ModelOption {
                value: "opus".into(),
                label: "Opus".into(),
            },
            ModelOption {
                value: "haiku".into(),
                label: "Haiku".into(),
            },
        ]
    }

    fn supports_thinking(&self) -> bool {
        true
    }

    fn supports_plan_mode(&self) -> bool {
        true
    }

    fn binary_name(&self) -> &'static str {
        "claude"
    }
}

// ── Claude-specific helpers ──────────────────────────────────────────

/// Extract total input and output tokens from Claude's usage JSON.
/// Claude splits input across input_tokens, cache_creation_input_tokens,
/// and cache_read_input_tokens — sum all three for the real total.
pub fn claude_extract_usage(usage: &serde_json::Value) -> (u64, u64) {
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_create = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    (input + cache_create + cache_read, output)
}

/// Extract a human-readable preview from a Claude tool_use input block.
fn claude_extract_input_preview(
    block: &serde_json::Value,
    name: &str,
    worktree_path: &str,
) -> Option<String> {
    block.get("input").and_then(|input| {
        let strip = |s: &str| -> String { strip_worktree_prefix(s, worktree_path) };

        if name == "AskUserQuestion" {
            if let Some(questions) = input.get("questions") {
                return Some(questions.to_string());
            }
        }
        if name == "TodoWrite" {
            if let Some(todos) = input.get("todos") {
                return Some(todos.to_string());
            }
        }
        if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
            Some(strip(fp))
        } else if let Some(cmd) = input.get("command").and_then(|c| c.as_str()) {
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
            input.as_object().and_then(|obj| {
                obj.values()
                    .filter_map(|v| v.as_str())
                    .find(|s| !s.is_empty() && s.len() < 200)
                    .map(|s| strip(s).chars().take(120).collect())
            })
        }
    })
}

// ── Codex backend ────────────────────────────────────────────────────

struct CodexBackend;

impl AgentBackend for CodexBackend {
    fn build_session_command(
        &self,
        ctx: &SessionContext,
    ) -> Result<(std::process::Command, Vec<String>), String> {
        let codex_bin = get_shell_env()
            .codex_path
            .as_deref()
            .ok_or("Codex CLI not found. Install with: npm i -g @openai/codex")?;

        // Write system prompt to a temp file, referenced via -c model_instructions_file.
        // All config injected via -c flags — never override CODEX_HOME (breaks auth).
        let codex_data = ctx.mcp_dir.join("codex-data").join(&ctx.workspace_id);
        let _ = std::fs::create_dir_all(&codex_data);
        let instructions_path = codex_data.join("instructions.md");
        if let Some(ref sp) = ctx.system_prompt {
            if !sp.is_empty() {
                let _ = std::fs::write(&instructions_path, sp);
            }
        }

        let mut cmd = if let Some(ref sid) = ctx.session_id {
            // Resume existing session: codex exec resume <session-id> <prompt> --json
            let mut cmd = std::process::Command::new(codex_bin);
            cmd.args(["exec", "resume", sid]);
            cmd.arg("--json");
            cmd.arg("--dangerously-bypass-approvals-and-sandbox");
            cmd.arg(&ctx.prompt);
            cmd
        } else {
            // New session: codex exec --json <prompt>
            // --dangerously-bypass-approvals-and-sandbox is required for MCP tool calls
            // (--full-auto blocks them with "user cancelled MCP tool call").
            // The worktree is already isolated inside Korlap's data dir, so this is safe.
            let mut cmd = std::process::Command::new(codex_bin);
            cmd.args(["exec", "--json", "--dangerously-bypass-approvals-and-sandbox"]);

            if let Some(ref model_id) = ctx.model {
                if !model_id.is_empty() {
                    cmd.args(["--model", model_id]);
                }
            }

            cmd.arg(&ctx.prompt);
            cmd
        };

        // Inject system prompt via -c (file lives in app data dir, not worktree)
        if instructions_path.exists() {
            cmd.args([
                "-c",
                &format!(
                    "model_instructions_file={}",
                    instructions_path.to_string_lossy()
                ),
            ]);
        }

        // Only register the built-in "korlap" MCP server with Codex.
        // 3rd-party servers (Atlassian, etc.) are designed for Claude's --mcp-config
        // and often need OAuth flows that break non-interactive Codex exec.
        // Users can add those to Codex separately via `codex mcp add`.
        let korlap_only: serde_json::Map<String, serde_json::Value> = ctx
            .mcp_servers
            .iter()
            .filter(|(name, _)| name.as_str() == "korlap")
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let codex_mcp_names = codex_register_mcp_servers(codex_bin, &korlap_only, &ctx.workspace_id);

        cmd.current_dir(&ctx.worktree_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        inject_shell_env(&mut cmd);

        for (key, value) in &ctx.git_auth_env {
            cmd.env(key, value);
        }

        Ok((cmd, codex_mcp_names))
    }

    fn parse_stream_line(
        &self,
        line: &str,
        worktree_path: &str,
    ) -> Option<ParsedEvent> {
        // Actual Codex exec --json JSONL format (verified against real CLI output):
        //   {"type":"thread.started","thread_id":"..."}
        //   {"type":"turn.started"}
        //   {"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"..."}}
        //   {"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"...","aggregated_output":"...","exit_code":0,"status":"completed"}}
        //   {"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"...","status":"in_progress"}}
        //   {"type":"turn.completed","usage":{"input_tokens":N,"cached_input_tokens":N,"output_tokens":N}}
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let event_type = v.get("type").and_then(|t| t.as_str())?;

        match event_type {
            "thread.started" => {
                let thread_id = v.get("thread_id").and_then(|s| s.as_str())?;
                Some(ParsedEvent::SessionId(thread_id.to_string()))
            }
            "item.completed" => {
                let item = v.get("item")?;
                let item_type = item.get("type").and_then(|t| t.as_str())?;

                match item_type {
                    "agent_message" => {
                        // {"type":"item.completed","item":{"type":"agent_message","text":"Hello"}}
                        let text = item.get("text").and_then(|t| t.as_str())?.to_string();
                        if text.is_empty() {
                            return None;
                        }
                        Some(ParsedEvent::AssistantMessage {
                            text,
                            tool_uses: Vec::new(),
                            thinking: None,
                        })
                    }
                    "command_execution" => {
                        // {"type":"item.completed","item":{"type":"command_execution","command":"/bin/zsh -lc \"ls\"","aggregated_output":"...","exit_code":0}}
                        let command = item
                            .get("command")
                            .and_then(|c| c.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Strip the shell wrapper (e.g. `/bin/zsh -lc "actual cmd"`)
                        let display_cmd = command
                            .strip_prefix("/bin/zsh -lc \"")
                            .or_else(|| command.strip_prefix("/bin/bash -lc \""))
                            .and_then(|s| s.strip_suffix('"'))
                            .unwrap_or(&command);
                        let input_preview = Some(
                            strip_worktree_prefix(display_cmd, worktree_path)
                                .chars()
                                .take(120)
                                .collect(),
                        );

                        Some(ParsedEvent::AssistantMessage {
                            text: String::new(),
                            tool_uses: vec![ToolUseInfo {
                                name: "shell".to_string(),
                                input_preview,
                                file_path: None,
                                old_string: None,
                                new_string: None,
                            }],
                            thinking: None,
                        })
                    }
                    "mcp_tool_call" => {
                        // {"type":"item.completed","item":{"type":"mcp_tool_call","server":"korlap","tool":"rename_branch","arguments":{...},"result":{...}}}
                        let server = item.get("server").and_then(|s| s.as_str()).unwrap_or("");
                        let tool = item.get("tool").and_then(|t| t.as_str()).unwrap_or("unknown");
                        // Use the tool name directly (e.g. "rename_branch", "lsp_hover")
                        let name = tool.to_string();
                        let input_preview = item
                            .get("arguments")
                            .and_then(|a| a.as_object())
                            .and_then(|obj| {
                                obj.values()
                                    .filter_map(|v| v.as_str())
                                    .find(|s| !s.is_empty() && s.len() < 200)
                                    .map(|s| {
                                        strip_worktree_prefix(s, worktree_path)
                                            .chars()
                                            .take(120)
                                            .collect()
                                    })
                            });

                        // Check for errors
                        if let Some(err) = item.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
                            return Some(ParsedEvent::AssistantMessage {
                                text: format!("MCP {}.{} failed: {}", server, tool, err),
                                tool_uses: Vec::new(),
                                thinking: None,
                            });
                        }

                        Some(ParsedEvent::AssistantMessage {
                            text: String::new(),
                            tool_uses: vec![ToolUseInfo {
                                name,
                                input_preview,
                                file_path: None,
                                old_string: None,
                                new_string: None,
                            }],
                            thinking: None,
                        })
                    }
                    _ => None,
                }
            }
            "turn.completed" => {
                // Usage is extracted by the caller via codex_extract_usage()
                Some(ParsedEvent::Done)
            }
            "turn.failed" => {
                tracing::warn!("Codex turn failed: {}", line);
                Some(ParsedEvent::Done)
            }
            _ => None,
        }
    }

    fn list_models(&self) -> Vec<ModelOption> {
        vec![
            ModelOption {
                value: String::new(),
                label: "Default".into(),
            },
            ModelOption {
                value: "codex-mini".into(),
                label: "Codex Mini".into(),
            },
            ModelOption {
                value: "o3".into(),
                label: "o3".into(),
            },
            ModelOption {
                value: "o4-mini".into(),
                label: "o4 Mini".into(),
            },
        ]
    }

    fn supports_thinking(&self) -> bool {
        false
    }

    fn supports_plan_mode(&self) -> bool {
        true
    }

    fn binary_name(&self) -> &'static str {
        "codex"
    }
}

// ── Codex helpers ────────────────────────────────────────────────────

/// Register MCP servers with Codex CLI via `codex mcp add`.
/// Returns the list of registered server names (for cleanup after process exits).
pub fn codex_register_mcp_servers(
    codex_bin: &str,
    servers: &serde_json::Map<String, serde_json::Value>,
    workspace_id: &str,
) -> Vec<String> {
    let mut registered = Vec::new();
    for (name, config) in servers {
        let server_type = config
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("stdio");

        // Prefix with korlap_ and workspace ID to avoid collisions
        let codex_name = format!("korlap_{}_{}", &workspace_id[..8.min(workspace_id.len())], name);

        // Remove if already exists (idempotent)
        let _ = std::process::Command::new(codex_bin)
            .args(["mcp", "remove", &codex_name])
            .output();

        let mut add_cmd = std::process::Command::new(codex_bin);
        add_cmd.args(["mcp", "add", &codex_name]);
        inject_shell_env(&mut add_cmd);

        if server_type == "sse" {
            if let Some(url) = config.get("url").and_then(|u| u.as_str()) {
                add_cmd.args(["--url", url]);
            } else {
                continue;
            }
        } else {
            // Collect env vars
            if let Some(env) = config.get("env").and_then(|e| e.as_object()) {
                for (k, v) in env {
                    if let Some(val) = v.as_str() {
                        add_cmd.args(["--env", &format!("{}={}", k, val)]);
                    }
                }
            }

            // Add separator and command + args.
            // Codex sandbox can't resolve `bun` — use `npx tsx` as the runner instead.
            add_cmd.arg("--");
            let command = config.get("command").and_then(|c| c.as_str()).unwrap_or("");
            if command.is_empty() {
                continue;
            }
            let args: Vec<&str> = config
                .get("args")
                .and_then(|a| a.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            if command == "bun" && args.first() == Some(&"run") {
                // Replace: bun run <script> → npx tsx <script>
                add_cmd.args(["npx", "tsx"]);
                for arg in &args[1..] {
                    add_cmd.arg(arg);
                }
            } else {
                add_cmd.arg(command);
                for arg in &args {
                    add_cmd.arg(arg);
                }
            }
        }

        match add_cmd.output() {
            Ok(o) if o.status.success() => {
                tracing::info!("Registered Codex MCP server: {}", codex_name);
                registered.push(codex_name);
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("Failed to register Codex MCP server {}: {}", codex_name, stderr.trim());
            }
            Err(e) => {
                tracing::warn!("Failed to run codex mcp add: {}", e);
            }
        }
    }
    registered
}

/// Remove previously registered MCP servers from Codex CLI global config.
pub fn codex_unregister_mcp_servers(codex_bin: &str, names: &[String]) {
    for name in names {
        let _ = std::process::Command::new(codex_bin)
            .args(["mcp", "remove", name])
            .output();
        tracing::info!("Unregistered Codex MCP server: {}", name);
    }
}

/// Extract usage from a Codex turn.completed event.
/// Returns (input_tokens, output_tokens).
pub fn codex_extract_usage(v: &serde_json::Value) -> (u64, u64) {
    if let Some(usage) = v.get("usage") {
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cached = usage
            .get("cached_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        (input + cached, output)
    } else {
        (0, 0)
    }
}


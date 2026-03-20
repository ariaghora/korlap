use crate::state::{AgentHandle, AppState, RepoInfo, WorkspaceInfo, WorkspaceStatus};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::io::BufRead;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// PID of the in-flight `gh auth login` process, or 0 if none.
static GH_AUTH_PID: AtomicU32 = AtomicU32::new(0);
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

/// Cached shell env values (resolved once on first call).
fn get_shell_env() -> &'static ShellEnv {
    use std::sync::OnceLock;
    static ENV: OnceLock<ShellEnv> = OnceLock::new();
    ENV.get_or_init(|| {
        let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK").ok().or_else(|| {
            std::process::Command::new("launchctl")
                .args(["getenv", "SSH_AUTH_SOCK"])
                .output()
                .ok()
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() { None } else { Some(s) }
                })
        });

        let home = std::env::var("HOME").ok();

        // Use interactive login shell (-lic) so .zshrc is sourced — this is
        // where nvm/fnm/volta add their PATH entries.  Delimiters protect
        // against noisy .zshrc output (motd, nvm "now using", etc.).
        let delimiter = "__KORLAP_ENV__";
        let path = std::process::Command::new("zsh")
            .args([
                "-lic",
                &format!("echo {delimiter}; echo $PATH; echo {delimiter}"),
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next(); // noise before first delimiter
                let value = parts.next()?;  // the actual PATH
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });

        // Resolve absolute path to `claude` binary once, so we don't rely
        // on PATH lookup at every spawn (which can fail in sandboxed contexts).
        let claude_path = std::process::Command::new("zsh")
            .args(["-lic", &format!("echo {delimiter}; whence -p claude; echo {delimiter}")])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next();
                let value = parts.next()?;
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() || trimmed.contains("not found") {
                    None
                } else {
                    Some(trimmed)
                }
            });

        if claude_path.is_none() {
            tracing::warn!("Could not resolve `claude` binary path — agent spawn will likely fail");
        }

        ShellEnv { ssh_auth_sock, home, path, claude_path }
    })
}

struct ShellEnv {
    ssh_auth_sock: Option<String>,
    home: Option<String>,
    path: Option<String>,
    claude_path: Option<String>,
}

/// Inject essential shell environment vars that Tauri apps launched from
/// Finder/Dock don't inherit (SSH agent, PATH, HOME, etc.)
/// Strip ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (the terminator of an ANSI sequence)
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn inject_shell_env(cmd: &mut std::process::Command) {
    let env = get_shell_env();
    if let Some(ref sock) = env.ssh_auth_sock {
        cmd.env("SSH_AUTH_SOCK", sock);
    }
    if let Some(ref home) = env.home {
        cmd.env("HOME", home);
    }
    if let Some(ref path) = env.path {
        cmd.env("PATH", path);
    }
}

/// Resolve the GH token for a given profile via `gh auth token`.
/// Returns None if no profile is set or the token cannot be obtained.
fn resolve_gh_token(profile: &Option<String>) -> Option<String> {
    let profile = profile.as_ref()?;
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token", "--user", profile]);
    inject_shell_env(&mut cmd);
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Build a `git` Command with HTTPS URL rewriting for GH token auth.
fn git_cmd_with_auth(
    worktree_path: &std::path::Path,
    gh_token: &Option<String>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    if let Some(ref token) = gh_token {
        cmd.args([
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=git@github.com:",
                token
            ),
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=ssh://git@github.com/",
                token
            ),
        ]);
    }
    cmd.current_dir(worktree_path);
    inject_shell_env(&mut cmd);
    cmd
}

/// Build a `gh` Command with GH_TOKEN env injected.
fn gh_cmd_with_auth(
    worktree_path: &std::path::Path,
    gh_token: &Option<String>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("gh");
    cmd.current_dir(worktree_path);
    inject_shell_env(&mut cmd);
    if let Some(ref token) = gh_token {
        cmd.env("GH_TOKEN", token);
    }
    cmd
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

                        let input_preview = block.get("input").and_then(|input| {
                            let strip = |s: &str| -> String {
                                // Strip with trailing slash first, then without, to avoid orphaned "/"
                                let with_slash = format!("{}/", worktree_path);
                                s.replace(&with_slash, "./").replace(worktree_path, ".")
                            };
                            // AskUserQuestion: pass the raw questions JSON so the frontend
                            // can render interactive options
                            // Input shape: {"questions": [{"question": "...", "options": [...]}]}
                            if name == "AskUserQuestion" {
                                if let Some(questions) = input.get("questions") {
                                    return Some(questions.to_string());
                                }
                                // Fall through to generic extraction if structure unexpected
                            }
                            if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
                                Some(strip(fp))
                            } else if let Some(cmd) =
                                input.get("command").and_then(|c| c.as_str())
                            {
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
                                    // Standalone no-op cd — nothing useful to show
                                    return None;
                                } else {
                                    cleaned
                                };
                                Some(cleaned.chars().take(120).collect())
                            } else if let Some(pattern) =
                                input.get("pattern").and_then(|p| p.as_str())
                            {
                                Some(strip(pattern))
                            } else if let Some(query) =
                                input.get("query").and_then(|q| q.as_str())
                            {
                                Some(strip(query).chars().take(120).collect())
                            } else if let Some(desc) =
                                input.get("description").and_then(|d| d.as_str())
                            {
                                Some(strip(desc).chars().take(120).collect())
                            } else if let Some(skill) =
                                input.get("skill").and_then(|s| s.as_str())
                            {
                                Some(strip(skill))
                            } else if let Some(url) =
                                input.get("url").and_then(|u| u.as_str())
                            {
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
                        });

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

// ── Repository commands ──────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct RepoDetail {
    #[serde(flatten)]
    pub info: RepoInfo,
    pub display_name: String,
    pub default_branch: String,
}

#[tauri::command]
pub fn add_repo(path: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<RepoDetail, String> {
    let path = std::path::PathBuf::from(&path);
    register_repo(path, None, state)
}

#[tauri::command]
pub fn remove_repo(repo_id: String, state: State<'_, Arc<Mutex<AppState>>>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.repos.remove(&repo_id).ok_or("Repo not found")?;
    state.workspaces.retain(|_, w| w.repo_id != repo_id);
    state.save_repos()?;
    Ok(())
}

#[tauri::command]
pub fn list_repos(state: State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<RepoDetail>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let mut details = Vec::new();
    let mut needs_save = false;
    let repo_ids: Vec<String> = state.repos.keys().cloned().collect();
    for id in &repo_ids {
        let repo = state.repos.get(id).unwrap();
        let default_branch = if let Some(ref branch) = repo.default_branch {
            branch.clone()
        } else {
            // Backfill cache for repos saved before caching was added
            let branch = detect_default_branch(&repo.path).unwrap_or_default();
            let repo_mut = state.repos.get_mut(id).unwrap();
            repo_mut.default_branch = Some(branch.clone());
            needs_save = true;
            branch
        };
        let display_name = repo_display_name(&state.repos[id].path);
        details.push(RepoDetail {
            info: state.repos[id].clone(),
            display_name,
            default_branch,
        });
    }
    if needs_save {
        let _ = state.save_repos();
    }
    details.sort_by(|a, b| a.display_name.to_lowercase().cmp(&b.display_name.to_lowercase()));
    Ok(details)
}

// ── GitHub profile commands ──────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct GhProfile {
    pub login: String,
    pub active: bool,
}

#[tauri::command]
pub fn list_gh_profiles() -> Result<Vec<GhProfile>, String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "status", "--json", "hosts"]);
    inject_shell_env(&mut cmd);
    let output = cmd.output()
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if !output.status.success() {
        return Ok(vec![]); // gh not installed or not logged in
    }

    let v: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Failed to parse gh output: {}", e))?;

    let mut profiles = Vec::new();
    if let Some(hosts) = v.get("hosts").and_then(|h| h.as_object()) {
        for accounts in hosts.values() {
            if let Some(arr) = accounts.as_array() {
                for account in arr {
                    if let Some(login) = account.get("login").and_then(|l| l.as_str()) {
                        let active = account.get("active").and_then(|a| a.as_bool()).unwrap_or(false);
                        profiles.push(GhProfile {
                            login: login.to_string(),
                            active,
                        });
                    }
                }
            }
        }
    }

    Ok(profiles)
}

#[tauri::command]
pub fn set_repo_profile(
    repo_id: String,
    profile: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let repo = st.repos.get_mut(&repo_id).ok_or("Repo not found")?;
    repo.gh_profile = profile;
    st.save_repos()?;
    Ok(())
}

// ── GitHub onboarding commands ───────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct GhCliStatus {
    pub installed: bool,
    pub authenticated: bool,
    pub profiles: Vec<GhProfile>,
}

#[tauri::command]
pub fn check_gh_cli() -> Result<GhCliStatus, String> {
    // Check if gh is installed
    let mut cmd = std::process::Command::new("gh");
    cmd.arg("--version");
    inject_shell_env(&mut cmd);
    let version_result = cmd.output();

    let installed = match version_result {
        Ok(ref o) => o.status.success(),
        Err(_) => false,
    };

    if !installed {
        return Ok(GhCliStatus {
            installed: false,
            authenticated: false,
            profiles: vec![],
        });
    }

    // Reuse list_gh_profiles logic
    let profiles = list_gh_profiles()?;
    let authenticated = !profiles.is_empty();

    Ok(GhCliStatus {
        installed,
        authenticated,
        profiles,
    })
}

#[derive(Clone, serde::Serialize)]
pub struct GhAuthResult {
    pub code: String,
    pub url: String,
}

#[tauri::command]
pub async fn gh_auth_login(app_handle: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        use std::io::BufRead;

        let mut cmd = std::process::Command::new("gh");
        cmd.args([
            "auth", "login",
            "--hostname", "github.com",
            "--git-protocol", "https",
            "--web",
            "--scopes", "workflow",
        ]);
        inject_shell_env(&mut cmd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| format!("Failed to start gh auth login: {}", e))?;
        GH_AUTH_PID.store(child.id(), Ordering::SeqCst);

        // Send newline to skip "Press Enter" prompt
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            let _ = stdin.write_all(b"\n");
        }

        // Read stderr to capture the one-time code and URL.
        // gh outputs ANSI escape codes, so strip them before parsing.
        if let Some(stderr) = child.stderr.take() {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                let line = match line {
                    Ok(l) => strip_ansi(&l),
                    Err(_) => break,
                };
                // Extract URL (https://...) and open in browser
                if let Some(url) = line.split_whitespace().find(|w| w.starts_with("https://github.com/login")) {
                    let _ = std::process::Command::new("open").arg(url).spawn();
                }
                // Extract device code (pattern: XXXX-XXXX, alphanumeric)
                for word in line.split_whitespace() {
                    let parts: Vec<&str> = word.split('-').collect();
                    if parts.len() == 2
                        && parts[0].len() == 4
                        && parts[1].len() == 4
                        && parts[0].chars().all(|c| c.is_ascii_alphanumeric())
                        && parts[1].chars().all(|c| c.is_ascii_alphanumeric())
                    {
                        let _ = app_handle.emit("gh-auth-code", word.to_string());
                    }
                }
            }
        }

        let status = child.wait().map_err(|e| format!("gh auth login failed: {}", e))?;
        GH_AUTH_PID.store(0, Ordering::SeqCst);
        if !status.success() {
            return Err("GitHub authentication was cancelled or failed.".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn cancel_gh_auth_login() -> Result<(), String> {
    let pid = GH_AUTH_PID.swap(0, Ordering::SeqCst);
    if pid != 0 {
        let _ = std::process::Command::new("kill")
            .arg(pid.to_string())
            .output();
    }
    Ok(())
}

#[derive(Clone, serde::Serialize)]
pub struct GhRepoEntry {
    pub full_name: String,
    pub description: String,
    pub is_fork: bool,
    pub clone_url: String,
    pub updated_at: String,
}

#[tauri::command]
pub async fn list_gh_repos(profile: String, search: Option<String>) -> Result<Vec<GhRepoEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let token = resolve_gh_token(&Some(profile.clone()));

        let mut cmd = std::process::Command::new("gh");
        cmd.args([
            "repo",
            "list",
            &profile,
            "--json",
            "nameWithOwner,description,isFork,sshUrl,updatedAt",
            "--limit",
            "100",
            "--source",
        ]);
        inject_shell_env(&mut cmd);
        if let Some(ref t) = token {
            cmd.env("GH_TOKEN", t);
        }

        let output = cmd.output().map_err(|e| format!("Failed to run gh: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh repo list failed: {}", stderr));
        }

        let arr: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse gh output: {}", e))?;

        let search_lower = search.as_deref().unwrap_or("").to_lowercase();

        let mut repos: Vec<GhRepoEntry> = arr
            .into_iter()
            .filter_map(|v| {
                let full_name = v.get("nameWithOwner")?.as_str()?.to_string();
                let description = v
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_fork = v.get("isFork").and_then(|f| f.as_bool()).unwrap_or(false);
                let clone_url = v.get("sshUrl").and_then(|u| u.as_str()).unwrap_or("").to_string();
                let updated_at = v
                    .get("updatedAt")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();

                if !search_lower.is_empty()
                    && !full_name.to_lowercase().contains(&search_lower)
                    && !description.to_lowercase().contains(&search_lower)
                {
                    return None;
                }

                Some(GhRepoEntry {
                    full_name,
                    description,
                    is_fork,
                    clone_url,
                    updated_at,
                })
            })
            .collect();

        repos.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(repos)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn clone_repo(
    clone_url: String,
    repo_name: String,
    dest_path: Option<String>,
    profile: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<RepoDetail, String> {
    let token = resolve_gh_token(&Some(profile.clone()));

    // Determine destination
    let dest = if let Some(ref p) = dest_path {
        std::path::PathBuf::from(p)
    } else {
        // Default to ~/Developer/<repo-name>
        let home = std::env::var("HOME").map_err(|_| "Cannot determine HOME directory")?;
        std::path::PathBuf::from(home).join("Developer").join(&repo_name)
    };

    if dest.exists() {
        // If it already exists and is a git repo, just add it
        if dest.join(".git").exists() {
            return register_repo(dest, Some(profile), state);
        }
        return Err(format!("Destination already exists: {}", dest.display()));
    }

    // Create parent dir if needed
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Clone with token auth
    let mut cmd = std::process::Command::new("git");
    if let Some(ref t) = token {
        cmd.args([
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=git@github.com:",
                t
            ),
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=ssh://git@github.com/",
                t
            ),
        ]);
    }
    cmd.args(["clone", &clone_url, &dest.to_string_lossy()]);
    inject_shell_env(&mut cmd);

    let output = cmd.output().map_err(|e| format!("Failed to run git clone: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr));
    }

    register_repo(dest, Some(profile), state)
}

/// Shared helper to register a repo in app state (used by add_repo and clone_repo).
fn register_repo(
    path: std::path::PathBuf,
    gh_profile: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<RepoDetail, String> {
    let path = path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    AppState::is_git_repo(&path)?;

    let default_branch = detect_default_branch(&path)?;
    let display_name = repo_display_name(&path);

    let mut st = state.lock().map_err(|e| e.to_string())?;

    // Deduplicate by path
    if let Some(existing_id) = st.repos.values().find(|r| r.path == path).map(|r| r.id.clone()) {
        // Update gh_profile if provided
        if gh_profile.is_some() {
            if let Some(repo) = st.repos.get_mut(&existing_id) {
                repo.gh_profile = gh_profile;
            }
            st.save_repos()?;
        }
        let existing = st.repos[&existing_id].clone();
        return Ok(RepoDetail {
            info: existing,
            display_name,
            default_branch,
        });
    }

    let repo = RepoInfo {
        id: Uuid::new_v4().to_string(),
        path,
        gh_profile,
        default_branch: Some(default_branch.clone()),
    };

    st.repos.insert(repo.id.clone(), repo.clone());
    st.save_repos()?;

    tracing::info!("Registered repo {} at {}", repo.id, repo.path.display());
    Ok(RepoDetail {
        info: repo,
        display_name,
        default_branch,
    })
}

/// Extract "owner/repo" from a repo's remote origin URL.
/// Returns None if not a GitHub repo or if the remote can't be read.
fn extract_gh_nwo(path: &std::path::Path) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["remote", "get-url", "origin"]);
    cmd.current_dir(path);
    inject_shell_env(&mut cmd);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Match patterns:
    //   git@github.com:owner/repo.git
    //   https://github.com/owner/repo.git
    //   ssh://git@github.com/owner/repo.git
    let path_part = if let Some(rest) = url.strip_prefix("git@github.com:") {
        Some(rest)
    } else {
        url.split("github.com/").nth(1)
    };

    path_part.map(|p| p.trim_end_matches(".git").to_string())
}

/// Check which connected GH profile has access to the repo at `path`.
/// Returns the profile login that can access it, or None.
#[tauri::command]
pub async fn check_repo_gh_access(path: String, profiles: Vec<String>) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&path)
            .canonicalize()
            .map_err(|e| format!("Invalid path: {}", e))?;

        let nwo = match extract_gh_nwo(&path) {
            Some(nwo) => nwo,
            None => return Ok(None),
        };

        for profile in &profiles {
            let token = resolve_gh_token(&Some(profile.clone()));
            let mut cmd = std::process::Command::new("gh");
            cmd.args(["repo", "view", &nwo, "--json", "name"]);
            inject_shell_env(&mut cmd);
            if let Some(ref t) = token {
                cmd.env("GH_TOKEN", t);
            }
            let output = cmd.output().map_err(|e| format!("Failed to run gh: {}", e))?;
            if output.status.success() {
                return Ok(Some(profile.clone()));
            }
        }

        Ok(None)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Repo branch commands ────────────────────────────────────────────

#[tauri::command]
pub fn get_repo_head(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        repo.path.clone()
    };

    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to get HEAD branch: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tauri::command]
pub async fn checkout_default_branch(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (repo_path, default_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let branch = repo.default_branch.clone()
            .unwrap_or_else(|| "main".to_string());
        (repo.path.clone(), branch)
    };

    tauri::async_runtime::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["checkout", &default_branch])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to run git checkout: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Failed to checkout {}: {}",
                default_branch,
                stderr.trim()
            ));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Workspace commands ───────────────────────────────────────────────

#[tauri::command]
pub async fn create_workspace(
    repo_id: String,
    task_title: Option<String>,
    task_description: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<WorkspaceInfo, String> {
    let (repo_path, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), repo.gh_profile.clone())
    };

    // Resolve GH token early — if a profile is configured but the token can't be
    // obtained, fail immediately rather than silently branching off stale data.
    let gh_token = if let Some(ref profile) = gh_profile {
        let mut gh_auth_cmd = std::process::Command::new("gh");
        gh_auth_cmd.args(["auth", "token", "--user", profile]);
        inject_shell_env(&mut gh_auth_cmd);
        let output = gh_auth_cmd
            .output()
            .map_err(|e| format!("Failed to run gh: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "Cannot authenticate as GitHub profile '{}'. \
                 Fix your gh auth or change the repo's profile.\n{}",
                profile,
                stderr.trim()
            ));
        }
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    };

    let base_branch = detect_default_branch(&repo_path)?;

    // Fetch origin so we branch from the latest remote state.
    // When a gh_profile is set, rewrite SSH URLs to HTTPS with the token so
    // git authenticates as the selected profile, not the ambient SSH key.
    let mut fetch_cmd = std::process::Command::new("git");
    if let Some(ref token) = gh_token {
        fetch_cmd.args([
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=git@github.com:",
                token
            ),
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=ssh://git@github.com/",
                token
            ),
        ]);
    }
    fetch_cmd
        .args(["fetch", "origin", &base_branch])
        .current_dir(&repo_path);
    inject_shell_env(&mut fetch_cmd);
    let fetch_output = fetch_cmd
        .output()
        .map_err(|e| format!("Failed to run git fetch: {}", e))?;

    if !fetch_output.status.success() {
        let stderr = String::from_utf8_lossy(&fetch_output.stderr).to_string();
        let lower = stderr.to_lowercase();

        let hint = if lower.contains("repository not found") || lower.contains("could not read from remote") {
            if gh_profile.is_some() {
                "The configured GitHub profile may not have access to this repo. \
                 Try changing the profile in repo settings."
            } else {
                "No GitHub profile is set for this repo. \
                 Set one in repo settings so Korlap can authenticate."
            }
        } else if lower.contains("could not resolve host") {
            "Check your internet connection and try again."
        } else if lower.contains("permission denied") || lower.contains("authentication failed") {
            if gh_profile.is_some() {
                "Authentication failed. The token for this profile may be expired. \
                 Run `gh auth refresh` or change the profile in repo settings."
            } else {
                "Authentication failed. Set a GitHub profile in repo settings."
            }
        } else {
            "Check your git remote configuration and network connection."
        };

        return Err(format!(
            "Could not fetch from origin.\n{}\n\n{}",
            hint,
            stderr.trim()
        ));
    }

    let start_point = format!("origin/{}", base_branch);

    // Generate a unique name (retry if branch already exists)
    let mut name = random_workspace_name();
    let worktree_base = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.worktree_dir()
    };

    for attempt in 0..10 {
        let branch = format!("korlap/{}", name);
        let check = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &branch])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?;

        let folder_exists = worktree_base.join(&name).exists();

        if !check.status.success() && !folder_exists {
            break; // branch doesn't exist and folder is free, good to use
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
    let branch = format!("korlap/{}", name);

    // Worktree lives in app data dir, named after the workspace for human readability
    let worktree_path = worktree_base.join(&name);

    std::fs::create_dir_all(worktree_path.parent().unwrap_or(&worktree_path))
        .map_err(|e| e.to_string())?;

    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &branch])
        .arg(&worktree_path)
        .arg(&start_point)
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
        worktree_path: worktree_path.clone(),
        repo_id: repo_id.clone(),
        gh_profile,
        status: WorkspaceStatus::Waiting,
        created_at: now_unix(),
        task_title,
        task_description,
    };

    // Check if there's a setup script to run
    let setup_script = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.repo_settings
            .get(&repo_id)
            .map(|s| s.setup_script.clone())
            .unwrap_or_default()
    };

    if !setup_script.trim().is_empty() {
        tracing::info!("Running setup script for workspace {}", ws.name);
        let mut setup_cmd = std::process::Command::new("zsh");
        setup_cmd.args(["-c", &setup_script]);
        setup_cmd.current_dir(&worktree_path);
        setup_cmd.env("KORLAP_WORKSPACE_NAME", &ws.name);
        setup_cmd.env("KORLAP_WORKSPACE_PATH", &worktree_path.to_string_lossy().to_string());
        setup_cmd.env("KORLAP_ROOT_PATH", repo_path.to_string_lossy().to_string());
        setup_cmd.env("KORLAP_DEFAULT_BRANCH", &base_branch);
        inject_shell_env(&mut setup_cmd);
        let output = setup_cmd
            .output()
            .map_err(|e| format!("Setup script failed to start: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!("Setup script failed: {}", stderr.trim());
            // Don't fail workspace creation — just log the warning
        }
    }

    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.workspaces.insert(id, ws.clone());
    st.save_workspaces()?;

    tracing::info!("Created workspace {} ({})", ws.name, ws.id);
    Ok(ws)
}

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

#[tauri::command]
pub async fn remove_workspace(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (worktree_path, repo_path, ws_name, repo_id) = {
        let mut st = state.lock().map_err(|e| e.to_string())?;

        // Kill agent if running
        if let Some(mut handle) = st.agents.remove(&workspace_id) {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }

        // Kill terminal if running
        if let Some(mut term) = st.terminals.remove(&workspace_id) {
            let _ = term.child.kill();
        }

        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), repo.path.clone(), ws.name.clone(), ws.repo_id.clone())
    };

    // Run remove script if configured
    {
        let st = state.lock().map_err(|e| e.to_string())?;
        if let Some(settings) = st.repo_settings.get(&repo_id) {
            if !settings.remove_script.trim().is_empty() {
                tracing::info!("Running remove script for workspace {}", ws_name);
                let mut remove_cmd = std::process::Command::new("zsh");
                remove_cmd.args(["-c", &settings.remove_script]);
                remove_cmd.current_dir(&worktree_path);
                remove_cmd.env("KORLAP_WORKSPACE_NAME", &ws_name);
                remove_cmd.env("KORLAP_WORKSPACE_PATH", &worktree_path.to_string_lossy().to_string());
                remove_cmd.env("KORLAP_ROOT_PATH", repo_path.to_string_lossy().to_string());
                inject_shell_env(&mut remove_cmd);
                let _ = remove_cmd.output();
            }
        }
    }

    // Only try to remove if the worktree path still exists
    if worktree_path.exists() {
        let output = std::process::Command::new("git")
            .args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to remove worktree: {}", e))?;

        if !output.status.success() {
            // Try git worktree prune as fallback (cleans stale entries)
            let _ = std::process::Command::new("git")
                .args(["worktree", "prune"])
                .current_dir(&repo_path)
                .output();
        }
    } else {
        // Worktree already gone — prune stale git references
        let _ = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&repo_path)
            .output();
    }

    // Fully delete workspace: data files, session, and entry
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.delete_workspace_data(&workspace_id);
    st.workspaces.remove(&workspace_id);
    st.save_workspaces()?;

    tracing::info!("Removed workspace {}", workspace_id);
    Ok(())
}

#[tauri::command]
pub fn list_workspaces(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<WorkspaceInfo>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state
        .workspaces
        .values()
        .filter(|w| w.repo_id == repo_id)
        .cloned()
        .collect())
}

// ── Branch commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn rename_branch(
    workspace_id: String,
    new_name: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<WorkspaceInfo, String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let ws = st
        .workspaces
        .get(&workspace_id)
        .ok_or("Workspace not found")?;


    let worktree_path = ws.worktree_path.clone();
    let fallback_branch = ws.branch.clone();

    crate::state::rename_git_branch(&worktree_path, &new_name, &fallback_branch)?;

    let ws = st
        .workspaces
        .get_mut(&workspace_id)
        .ok_or("Workspace not found")?;
    ws.branch = new_name.clone();
    ws.name = new_name;
    let ws_clone = ws.clone();
    st.save_workspaces()?;

    tracing::info!("Renamed workspace {} to {}", workspace_id, ws_clone.name);
    Ok(ws_clone)
}

// ── File browser commands ────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,      // relative to worktree root
    pub is_dir: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn list_directory(
    workspace_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileEntry>, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let target = if relative_path.is_empty() {
        worktree_path.clone()
    } else {
        worktree_path.join(&relative_path)
    };

    // Security: ensure path doesn't escape worktree
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    let wt = worktree_canonical.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = Vec::new();
        let dir = std::fs::read_dir(&canonical)
            .map_err(|e| format!("Cannot read directory: {}", e))?;

        for entry in dir {
            let entry = entry.map_err(|e| format!("Error reading entry: {}", e))?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files/dirs (except common ones like .github)
            if name.starts_with('.') && name != ".github" && name != ".gitignore" {
                continue;
            }

            let metadata = entry
                .metadata()
                .map_err(|e| format!("Cannot stat {}: {}", name, e))?;

            let full_path = entry.path();
            let rel = full_path
                .strip_prefix(&wt)
                .unwrap_or(&full_path)
                .to_string_lossy()
                .to_string();

            entries.push(FileEntry {
                name,
                path: rel,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            });
        }

        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn read_file(
    workspace_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let target = worktree_path.join(&relative_path);
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| format!("Cannot stat file: {}", e))?;

        // Limit to 2MB to avoid UI freezes
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(format!(
                "File too large ({} bytes). Max 2MB for preview.",
                metadata.len()
            ));
        }

        std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Cannot read file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn read_repo_file(
    repo_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    let target = repo_path.join(&relative_path);
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let repo_canonical = repo_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve repo: {}", e))?;
    if !canonical.starts_with(&repo_canonical) {
        return Err("Path escapes repo boundary".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| format!("Cannot stat file: {}", e))?;

        // Limit to 2MB to avoid UI freezes
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(format!(
                "File too large ({} bytes). Max 2MB for preview.",
                metadata.len()
            ));
        }

        std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Cannot read file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn write_file(
    workspace_id: String,
    relative_path: String,
    content: String,
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

    let target = worktree_path.join(&relative_path);
    let canonical_parent = target
        .parent()
        .ok_or("Invalid file path")?
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical_parent.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    let write_target = canonical_parent.join(
        target
            .file_name()
            .ok_or("Invalid file name")?,
    );

    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&write_target, content)
            .map_err(|e| format!("Cannot write file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Git commands ─────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct ChangedFile {
    pub path: String,
    pub status: String, // "M", "A", "D", "R", "?"
    pub additions: i32,
    pub deletions: i32,
}

#[tauri::command]
pub async fn get_changed_files(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<ChangedFile>, String> {
    let (worktree_path, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo.default_branch.clone().unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base)
    };

    tauri::async_runtime::spawn_blocking(move || {
        // Compare against origin/<base> since workspaces branch from the remote tip.
        // Using the local base branch would show phantom diffs when local is behind remote.
        let remote_base = format!("origin/{}", base_branch);
        let merge_base = std::process::Command::new("git")
            .args(["merge-base", &remote_base, "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .ok()
            .and_then(|o| if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else { None })
            .unwrap_or_else(|| remote_base.clone());

        let output = std::process::Command::new("git")
            .args(["diff", "--numstat", &merge_base])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to run git diff: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git diff --numstat failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut files = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let additions = parts[0].parse::<i32>().unwrap_or(0);
                let deletions = parts[1].parse::<i32>().unwrap_or(0);
                let path = parts[2].to_string();
                let status = if additions > 0 && deletions > 0 {
                    "M"
                } else if additions > 0 {
                    "A"
                } else {
                    "D"
                };
                files.push(ChangedFile {
                    path,
                    status: status.to_string(),
                    additions,
                    deletions,
                });
            }
        }

        let untracked = std::process::Command::new("git")
            .args(["ls-files", "--others", "--exclude-standard"])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to list untracked files: {}", e))?;

        if untracked.status.success() {
            for line in String::from_utf8_lossy(&untracked.stdout).lines() {
                let path = line.trim().to_string();
                if !path.is_empty() {
                    let line_count = std::fs::read_to_string(worktree_path.join(&path))
                        .map(|c| c.lines().count() as i32)
                        .unwrap_or(0);
                    files.push(ChangedFile {
                        path,
                        status: "A".to_string(),
                        additions: line_count,
                        deletions: 0,
                    });
                }
            }
        }

        Ok(files)
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn get_diff(
    workspace_id: String,
    file_path: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let (worktree_path, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo.default_branch.clone().unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base)
    };

    tauri::async_runtime::spawn_blocking(move || {
        // Compare against origin/<base> since workspaces branch from the remote tip.
        let remote_base = format!("origin/{}", base_branch);
        let merge_base = std::process::Command::new("git")
            .args(["merge-base", &remote_base, "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .ok()
            .and_then(|o| if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else { None })
            .unwrap_or_else(|| remote_base.clone());

        let mut cmd = std::process::Command::new("git");
        cmd.args(["diff", &merge_base]);
        if let Some(ref fp) = file_path {
            cmd.arg("--").arg(fp);
        }
        cmd.current_dir(&worktree_path);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run git diff: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git diff failed: {}", stderr.trim()));
        }

        let diff_text = String::from_utf8_lossy(&output.stdout).to_string();

        if diff_text.trim().is_empty() {
            if let Some(ref fp) = file_path {
                let full_path = worktree_path.join(fp);
                if full_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&full_path) {
                        let mut result = format!("--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n", fp, content.lines().count());
                        for line in content.lines() {
                            result.push('+');
                            result.push_str(line);
                            result.push('\n');
                        }
                        return Ok(result);
                    }
                }
            }
        }

        Ok(diff_text)
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

// ── Direct git/gh commands ───────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct CommitResult {
    pub hash: String,
    pub message: String,
}

#[tauri::command]
pub async fn git_commit(
    workspace_id: String,
    message: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<CommitResult, String> {
    if message.trim().is_empty() {
        return Err("Commit message cannot be empty".into());
    }

    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let msg = message.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Stage all changes
        let add_output = std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to run git add: {}", e))?;
        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(format!("git add failed: {}", stderr.trim()));
        }

        // Check if there's anything to commit
        let diff_check = std::process::Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(&worktree_path)
            .status()
            .map_err(|e| format!("Failed to check staged changes: {}", e))?;
        if diff_check.success() {
            return Err("Nothing to commit — working tree is clean".into());
        }

        // Commit
        let commit_output = std::process::Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to run git commit: {}", e))?;
        if !commit_output.status.success() {
            let stderr = String::from_utf8_lossy(&commit_output.stderr);
            return Err(format!("git commit failed: {}", stderr.trim()));
        }

        // Get the short hash of the new commit
        let hash_output = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to get commit hash: {}", e))?;
        let hash = String::from_utf8_lossy(&hash_output.stdout).trim().to_string();

        Ok(CommitResult { hash, message: msg })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn git_push(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (worktree_path, branch, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), ws.branch.clone(), repo.gh_profile.clone())
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = git_cmd_with_auth(&worktree_path, &gh_token);
        cmd.args(["push", "-u", "origin", &branch]);

        let output = cmd.output().map_err(|e| format!("Failed to run git push: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git push failed: {}", stderr.trim()));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Sync main ──────────────────────────────────────────────────────

/// Fetch origin and return how many commits local default branch is behind.
#[tauri::command]
pub async fn check_main_behind(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<u32, String> {
    let (repo_path, gh_profile, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let branch = repo.default_branch.clone()
            .unwrap_or_else(|| "main".to_string());
        (repo.path.clone(), repo.gh_profile.clone(), branch)
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        // Fetch latest
        let mut fetch = git_cmd_with_auth(&repo_path, &gh_token);
        fetch.args(["fetch", "origin", &base_branch]);
        let output = fetch.output()
            .map_err(|e| format!("Failed to run git fetch: {}", e))?;
        if !output.status.success() {
            return Err("Could not fetch from origin".into());
        }

        // Count commits local is behind: main..origin/main
        let rev_list = std::process::Command::new("git")
            .args([
                "rev-list", "--count",
                &format!("{}..origin/{}", base_branch, base_branch),
            ])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to compare refs: {}", e))?;

        if !rev_list.status.success() {
            return Err("Failed to compare local and remote refs".into());
        }

        let count_str = String::from_utf8_lossy(&rev_list.stdout).trim().to_string();
        count_str.parse::<u32>().map_err(|_| "Failed to parse commit count".into())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn sync_main(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (repo_path, gh_profile, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let branch = repo.default_branch.clone()
            .unwrap_or_else(|| "main".to_string());
        (repo.path.clone(), repo.gh_profile.clone(), branch)
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        // 1. Fetch latest from origin
        let mut fetch = git_cmd_with_auth(&repo_path, &gh_token);
        fetch.args(["fetch", "origin", &base_branch]);
        let output = fetch
            .output()
            .map_err(|e| format!("Failed to run git fetch: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let lower = stderr.to_lowercase();
            let hint = if lower.contains("could not resolve host") {
                "Check your internet connection and try again."
            } else if lower.contains("permission denied") || lower.contains("authentication failed") {
                "Authentication failed. Check the GitHub profile in repo settings."
            } else {
                "Check your git remote configuration and network connection."
            };
            return Err(format!("Could not fetch from origin.\n{}\n\n{}", hint, stderr.trim()));
        }

        // 2. Check if HEAD is on the default branch
        let head_output = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(&repo_path)
            .output()
            .map_err(|e| format!("Failed to check HEAD: {}", e))?;
        let current_branch = String::from_utf8_lossy(&head_output.stdout).trim().to_string();

        if current_branch == base_branch {
            // HEAD is on the default branch — must update working tree too
            let output = std::process::Command::new("git")
                .args(["merge", "--ff-only", &format!("origin/{}", base_branch)])
                .current_dir(&repo_path)
                .output()
                .map_err(|e| format!("Failed to fast-forward: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to fast-forward local {}: {}", base_branch, stderr.trim()));
            }
        } else {
            // Default branch not checked out — safe to just move the ref
            let output = std::process::Command::new("git")
                .args([
                    "update-ref",
                    &format!("refs/heads/{}", base_branch),
                    &format!("refs/remotes/origin/{}", base_branch),
                ])
                .current_dir(&repo_path)
                .output()
                .map_err(|e| format!("Failed to update local ref: {}", e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("Failed to fast-forward local {}: {}", base_branch, stderr.trim()));
            }
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Base branch update detection & merge ─────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct BaseUpdateStatus {
    pub behind_by: i64,
}

#[tauri::command]
pub async fn check_base_updates(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<BaseUpdateStatus, String> {
    let (worktree_path, base_branch, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo.default_branch.clone().unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base, repo.gh_profile.clone())
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        // Fetch latest from origin for the base branch
        let mut fetch_cmd = git_cmd_with_auth(&worktree_path, &gh_token);
        fetch_cmd.args(["fetch", "origin", &base_branch]);
        fetch_cmd
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let _ = fetch_cmd.output(); // Best-effort; if offline we still compare stale refs

        let remote_base = format!("origin/{}", base_branch);

        // Find merge-base between HEAD and origin/<base>
        let merge_base_output = std::process::Command::new("git")
            .args(["merge-base", "HEAD", &remote_base])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to run git merge-base: {}", e))?;

        if !merge_base_output.status.success() {
            // No common ancestor — treat as 0 behind
            return Ok(BaseUpdateStatus { behind_by: 0 });
        }

        let merge_base = String::from_utf8_lossy(&merge_base_output.stdout)
            .trim()
            .to_string();

        // Count commits on origin/<base> since the merge-base
        let rev_list = std::process::Command::new("git")
            .args(["rev-list", "--count", &format!("{}..{}", merge_base, remote_base)])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to count commits: {}", e))?;

        let behind_by = if rev_list.status.success() {
            String::from_utf8_lossy(&rev_list.stdout)
                .trim()
                .parse::<i64>()
                .unwrap_or(0)
        } else {
            0
        };

        Ok(BaseUpdateStatus { behind_by })
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn update_from_base(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (worktree_path, base_branch, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo.default_branch.clone().unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base, repo.gh_profile.clone())
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        // Fetch latest
        let mut fetch_cmd = git_cmd_with_auth(&worktree_path, &gh_token);
        fetch_cmd.args(["fetch", "origin", &base_branch]);
        let fetch_output = fetch_cmd
            .output()
            .map_err(|e| format!("Failed to fetch: {}", e))?;
        if !fetch_output.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_output.stderr);
            return Err(format!(
                "git fetch failed: {}",
                stderr.trim()
            ));
        }

        // Merge origin/<base> into the workspace branch
        let remote_base = format!("origin/{}", base_branch);
        let merge_output = std::process::Command::new("git")
            .args(["merge", &remote_base, "--no-edit"])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to run git merge: {}", e))?;

        if !merge_output.status.success() {
            let stderr = String::from_utf8_lossy(&merge_output.stderr);
            let stdout = String::from_utf8_lossy(&merge_output.stdout);

            // Abort the failed merge to leave worktree clean
            let _ = std::process::Command::new("git")
                .args(["merge", "--abort"])
                .current_dir(&worktree_path)
                .output();

            if stderr.contains("CONFLICT") || stdout.contains("CONFLICT") {
                return Err(format!(
                    "Merge conflicts detected when updating from {}. The merge has been aborted.",
                    base_branch
                ));
            }
            return Err(format!("git merge failed: {}", stderr.trim()));
        }

        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn gh_pr_merge(
    workspace_id: String,
    pr_number: i64,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (worktree_path, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), repo.gh_profile.clone())
    };

    let gh_token = resolve_gh_token(&gh_profile);

    tauri::async_runtime::spawn_blocking(move || {
        let mut cmd = gh_cmd_with_auth(&worktree_path, &gh_token);
        cmd.args(["pr", "merge", &pr_number.to_string(), "--squash", "--delete-branch=false"]);

        let output = cmd.output().map_err(|e| format!("Failed to run gh pr merge: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh pr merge failed: {}", stderr.trim()));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

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
            Ok(Ok(output)) => {
                tracing::warn!("claude exited with non-zero for commit message generation");
                Ok("chore: update files".to_string())
            }
            Ok(Err(e)) => {
                tracing::warn!("claude failed for commit message: {}", e);
                Ok("chore: update files".to_string())
            }
            Err(_) => {
                tracing::warn!("claude timed out generating commit message");
                // Kill the timed-out process
                drop(handle);
                Ok("chore: update files".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── File search commands ─────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct FileSearchResult {
    /// Relative path from worktree root (e.g. "src/lib/ipc.ts")
    pub path: String,
    /// Just the filename (e.g. "ipc.ts")
    pub name: String,
    /// "file" or "folder"
    pub kind: String,
    /// Fuzzy match score (higher = better)
    pub score: i64,
}

#[tauri::command]
pub fn search_workspace_files(
    workspace_id: String,
    query: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileSearchResult>, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(vec![]);
    }

    let matcher = SkimMatcherV2::default();
    let mut results: Vec<FileSearchResult> = Vec::new();
    let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    let walker = ignore::WalkBuilder::new(&worktree_path)
        .hidden(true) // respect hidden
        .git_ignore(true) // respect .gitignore
        .git_global(true)
        .git_exclude(true)
        .max_depth(Some(12))
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let rel_path = match path.strip_prefix(&worktree_path) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip the root itself and .git
        let rel_str = rel_path.to_string_lossy();
        if rel_str.is_empty() || rel_str.starts_with(".git") {
            continue;
        }

        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        let name = rel_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Match against filename and full path — take the better score
        let score_name = matcher.fuzzy_match(&name, &query).unwrap_or(0);
        let score_path = matcher.fuzzy_match(&rel_str, &query).unwrap_or(0);
        let score = score_name.max(score_path);

        if score > 0 {
            if is_dir {
                let dir_key = rel_str.to_string();
                if !seen_dirs.insert(dir_key.clone()) {
                    continue;
                }
                results.push(FileSearchResult {
                    path: format!("{}/", rel_str),
                    name: format!("{}/", name),
                    kind: "folder".to_string(),
                    score,
                });
            } else {
                results.push(FileSearchResult {
                    path: rel_str.to_string(),
                    name,
                    kind: "file".to_string(),
                    score,
                });
            }
        }
    }

    // Sort by score descending, limit to top 20
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(20);

    Ok(results)
}

#[tauri::command]
pub fn search_repo_files(
    repo_id: String,
    query: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileSearchResult>, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(vec![]);
    }

    let matcher = SkimMatcherV2::default();
    let mut results: Vec<FileSearchResult> = Vec::new();
    let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    let walker = ignore::WalkBuilder::new(&repo_path)
        .hidden(true) // respect hidden
        .git_ignore(true) // respect .gitignore
        .git_global(true)
        .git_exclude(true)
        .max_depth(Some(12))
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let rel_path = match path.strip_prefix(&repo_path) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip the root itself and .git
        let rel_str = rel_path.to_string_lossy();
        if rel_str.is_empty() || rel_str.starts_with(".git") {
            continue;
        }

        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        let name = rel_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Match against filename and full path — take the better score
        let score_name = matcher.fuzzy_match(&name, &query).unwrap_or(0);
        let score_path = matcher.fuzzy_match(&rel_str, &query).unwrap_or(0);
        let score = score_name.max(score_path);

        if score > 0 {
            if is_dir {
                let dir_key = rel_str.to_string();
                if !seen_dirs.insert(dir_key.clone()) {
                    continue;
                }
                results.push(FileSearchResult {
                    path: format!("{}/", rel_str),
                    name: format!("{}/", name),
                    kind: "folder".to_string(),
                    score,
                });
            } else {
                results.push(FileSearchResult {
                    path: rel_str.to_string(),
                    name,
                    kind: "file".to_string(),
                    score,
                });
            }
        }
    }

    // Sort by score descending, limit to top 20
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(20);

    Ok(results)
}

// ── Grep (content search) ────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: u32,
    pub column: u32,
    pub line_content: String,
}

#[derive(Clone, serde::Serialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

#[tauri::command]
pub async fn grep_workspace(
    workspace_id: String,
    pattern: String,
    is_regex: bool,
    case_sensitive: bool,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<GrepResult, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let pattern_trimmed = pattern.trim().to_string();
    if pattern_trimmed.is_empty() {
        return Ok(GrepResult {
            matches: vec![],
            truncated: false,
        });
    }

    let wt = worktree_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use grep_matcher::Matcher;
        use grep_regex::RegexMatcherBuilder;
        use grep_searcher::sinks::UTF8;
        use grep_searcher::SearcherBuilder;
        use ignore::WalkBuilder;

        // Build the regex matcher with the same semantics as `rg`
        let escaped_pattern = if is_regex {
            pattern_trimmed.clone()
        } else {
            regex::escape(&pattern_trimmed)
        };

        let mut builder = RegexMatcherBuilder::new();
        if case_sensitive {
            // Strict case: defaults are fine (case_insensitive=false, case_smart=false)
        } else {
            // Smart case: case-insensitive unless pattern has uppercase
            builder.case_smart(true);
        }
        let matcher = builder
            .build(&escaped_pattern)
            .map_err(|e| format!("Invalid pattern: {}", e))?;

        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .build();

        let max_results: usize = 100;
        let max_matches_per_file: usize = 5;
        let max_line_len: usize = 500;
        let max_filesize: u64 = 1_048_576; // 1MB

        let mut results: Vec<GrepMatch> = Vec::new();
        let mut truncated = false;
        let worktree_prefix = wt.to_string_lossy().to_string();

        let walker = WalkBuilder::new(&wt)
            .hidden(true) // respect hidden files like rg default (skip .hidden)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .max_filesize(Some(max_filesize))
            .build();

        'outer: for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Skip directories and symlinks
            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => continue,
            };
            if !ft.is_file() {
                continue;
            }

            let path = entry.path().to_path_buf();

            let mut file_match_count = 0usize;

            let search_result = searcher.search_path(
                &matcher,
                &path,
                UTF8(|line_number, line_content| {
                    if results.len() >= max_results {
                        truncated = true;
                        return Ok(false); // stop searching
                    }
                    if file_match_count >= max_matches_per_file {
                        return Ok(false); // stop this file
                    }
                    file_match_count += 1;

                    let raw_path = path.to_string_lossy();
                    let rel_path = raw_path
                        .strip_prefix(&worktree_prefix)
                        .unwrap_or(&raw_path)
                        .trim_start_matches('/');

                    // Find column of first match in the line
                    let column = matcher
                        .find(line_content.as_bytes())
                        .ok()
                        .flatten()
                        .map(|m| m.start() as u32)
                        .unwrap_or(0);

                    let mut content = line_content.trim_end().to_string();
                    if content.len() > max_line_len {
                        content.truncate(max_line_len);
                        content.push_str("…");
                    }

                    results.push(GrepMatch {
                        path: rel_path.to_string(),
                        line_number: line_number as u32,
                        column,
                        line_content: content,
                    });

                    Ok(true)
                }),
            );

            // Silently skip files that can't be searched (binary, encoding issues, etc.)
            if let Err(_) = search_result {
                continue;
            }

            if truncated {
                break 'outer;
            }
        }

        Ok(GrepResult {
            matches: results,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn grep_repo(
    repo_id: String,
    pattern: String,
    is_regex: bool,
    case_sensitive: bool,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<GrepResult, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    let pattern_trimmed = pattern.trim().to_string();
    if pattern_trimmed.is_empty() {
        return Ok(GrepResult {
            matches: vec![],
            truncated: false,
        });
    }

    let wt = repo_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use grep_matcher::Matcher;
        use grep_regex::RegexMatcherBuilder;
        use grep_searcher::sinks::UTF8;
        use grep_searcher::SearcherBuilder;
        use ignore::WalkBuilder;

        // Build the regex matcher with the same semantics as `rg`
        let escaped_pattern = if is_regex {
            pattern_trimmed.clone()
        } else {
            regex::escape(&pattern_trimmed)
        };

        let mut builder = RegexMatcherBuilder::new();
        if case_sensitive {
            // Strict case: defaults are fine (case_insensitive=false, case_smart=false)
        } else {
            // Smart case: case-insensitive unless pattern has uppercase
            builder.case_smart(true);
        }
        let matcher = builder
            .build(&escaped_pattern)
            .map_err(|e| format!("Invalid pattern: {}", e))?;

        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .build();

        let max_results: usize = 100;
        let max_matches_per_file: usize = 5;
        let max_line_len: usize = 500;
        let max_filesize: u64 = 1_048_576; // 1MB

        let mut results: Vec<GrepMatch> = Vec::new();
        let mut truncated = false;
        let repo_prefix = wt.to_string_lossy().to_string();

        let walker = WalkBuilder::new(&wt)
            .hidden(true) // respect hidden files like rg default (skip .hidden)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .max_filesize(Some(max_filesize))
            .build();

        'outer: for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };

            // Skip directories and symlinks
            let ft = match entry.file_type() {
                Some(ft) => ft,
                None => continue,
            };
            if !ft.is_file() {
                continue;
            }

            let path = entry.path().to_path_buf();

            let mut file_match_count = 0usize;

            let search_result = searcher.search_path(
                &matcher,
                &path,
                UTF8(|line_number, line_content| {
                    if results.len() >= max_results {
                        truncated = true;
                        return Ok(false); // stop searching
                    }
                    if file_match_count >= max_matches_per_file {
                        return Ok(false); // stop this file
                    }
                    file_match_count += 1;

                    let raw_path = path.to_string_lossy();
                    let rel_path = raw_path
                        .strip_prefix(&repo_prefix)
                        .unwrap_or(&raw_path)
                        .trim_start_matches('/');

                    // Find column of first match in the line
                    let column = matcher
                        .find(line_content.as_bytes())
                        .ok()
                        .flatten()
                        .map(|m| m.start() as u32)
                        .unwrap_or(0);

                    let mut content = line_content.trim_end().to_string();
                    if content.len() > max_line_len {
                        content.truncate(max_line_len);
                        content.push_str("…");
                    }

                    results.push(GrepMatch {
                        path: rel_path.to_string(),
                        line_number: line_number as u32,
                        column,
                        line_content: content,
                    });

                    Ok(true)
                }),
            );

            // Silently skip files that can't be searched (binary, encoding issues, etc.)
            if let Err(_) = search_result {
                continue;
            }

            if truncated {
                break 'outer;
            }
        }

        Ok(GrepResult {
            matches: results,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn read_workspace_file(
    workspace_id: String,
    file_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let full_path = worktree_path.join(&file_path);

    // Security: ensure the path stays within the worktree
    let canonical = full_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let wt_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&wt_canonical) {
        return Err("Path escapes worktree boundary".into());
    }

    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    const MAX_LINES: usize = 100;
    let total_lines = content.lines().count();
    let total_bytes = content.len();

    if total_lines > MAX_LINES {
        let truncated: String = content.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
        Ok(format!(
            "{}\n\n... truncated ({} of {} lines shown, {} bytes total)",
            truncated, MAX_LINES, total_lines, total_bytes
        ))
    } else {
        Ok(content)
    }
}

// ── PR commands ──────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct PrStatus {
    pub state: String,         // "none", "open", "merged", "closed"
    pub url: String,
    pub number: i64,
    pub title: String,
    pub checks: String,        // "pending", "passing", "failing", "none"
    pub mergeable: String,       // "mergeable", "conflicting", "unknown"
    pub additions: i64,
    pub deletions: i64,
    pub ahead_by: i64,         // commits ahead of remote (unpushed)
}

#[tauri::command]
pub async fn get_pr_status(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<PrStatus, String> {
    let (worktree_path, branch, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st
            .repos
            .get(&ws.repo_id)
            .ok_or("Repo not found")?;
        (ws.worktree_path.clone(), ws.branch.clone(), repo.gh_profile.clone())
    };

    // Resolve GH token outside the lock to avoid blocking other commands
    let gh_token = if let Some(ref profile) = gh_profile {
        let mut gh_auth_cmd = std::process::Command::new("gh");
        gh_auth_cmd.args(["auth", "token", "--user", profile]);
        inject_shell_env(&mut gh_auth_cmd);
        gh_auth_cmd.output().ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        None
    };

    // Run gh in a blocking thread so it doesn't hold up the IPC queue
    tauri::async_runtime::spawn_blocking(move || {
        let mut gh_cmd = std::process::Command::new("gh");
        gh_cmd.args([
            "pr", "view", &branch,
            "--json", "state,url,number,title,statusCheckRollup,mergeable,additions,deletions",
        ]);
        gh_cmd.current_dir(&worktree_path);
        inject_shell_env(&mut gh_cmd);
        if let Some(ref token) = gh_token {
            gh_cmd.env("GH_TOKEN", token);
        }
        let output = gh_cmd.output()
            .map_err(|e| format!("Failed to run gh: {}", e))?;

        if !output.status.success() {
            return Ok(PrStatus {
                state: "none".into(),
                url: String::new(),
                number: 0,
                title: String::new(),
                checks: "none".into(),
                mergeable: "unknown".into(),
                additions: 0,
                deletions: 0,
                ahead_by: 0,
            });
        }

        let v: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse gh output: {}", e))?;

        let pr_state = v.get("state").and_then(|s| s.as_str()).unwrap_or("OPEN").to_lowercase();
        let url = v.get("url").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let number = v.get("number").and_then(|n| n.as_i64()).unwrap_or(0);
        let title = v.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let additions = v.get("additions").and_then(|n| n.as_i64()).unwrap_or(0);
        let deletions = v.get("deletions").and_then(|n| n.as_i64()).unwrap_or(0);
        let mergeable = v.get("mergeable").and_then(|s| s.as_str()).unwrap_or("UNKNOWN").to_lowercase();

        let checks = if let Some(checks_arr) = v.get("statusCheckRollup").and_then(|c| c.as_array()) {
            if checks_arr.is_empty() {
                "none".to_string()
            } else {
                let any_failing = checks_arr.iter().any(|c| {
                    let conclusion = c.get("conclusion").and_then(|s| s.as_str()).unwrap_or("");
                    conclusion == "FAILURE" || conclusion == "ERROR" || conclusion == "TIMED_OUT"
                });
                let all_done = checks_arr.iter().all(|c| {
                    let status = c.get("status").and_then(|s| s.as_str()).unwrap_or("");
                    status == "COMPLETED"
                });
                if any_failing {
                    "failing".to_string()
                } else if all_done {
                    "passing".to_string()
                } else {
                    "pending".to_string()
                }
            }
    } else {
        "none".to_string()
    };

        // Count unpushed commits: how far local branch is ahead of remote
        let ahead_by = {
            let rev_output = std::process::Command::new("git")
                .args(["rev-list", "--count", &format!("origin/{}..{}", branch, branch)])
                .current_dir(&worktree_path)
                .output();
            match rev_output {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().parse::<i64>().unwrap_or(0)
                }
                _ => 0,
            }
        };

        Ok(PrStatus {
            state: pr_state,
            url,
            number,
            title,
            checks,
            mergeable,
            additions,
            deletions,
            ahead_by,
        })
    }).await.map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn get_pr_template(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        repo.path.clone()
    };

    // Check common PR template locations
    let candidates = [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "docs/pull_request_template.md",
        "PULL_REQUEST_TEMPLATE.md",
    ];

    for candidate in candidates {
        let path = repo_path.join(candidate);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Ok(content);
            }
        }
    }

    Ok(String::new()) // No template found
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
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    inject_shell_env(&mut cmd);

    let mut child = cmd
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
    let (worktree_path, gh_profile, repo_id, ws_branch, repo_path, user_system_prompt) = {
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
        (
            ws.worktree_path.clone(),
            repo.gh_profile.clone(), // Always use repo's current profile, not stale workspace snapshot
            ws.repo_id.clone(),
            ws.branch.clone(),
            repo.path.clone(),
            user_sp,
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
        // Dev mode: resolve from CARGO_MANIFEST_DIR (src-tauri/../src-mcp/server.ts)
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
    // Permission mode: plan mode uses --permission-mode plan, otherwise bypass all
    if plan_mode {
        cmd.args(["--permission-mode", "plan"]);
        // Allow rename_branch to execute without permission in plan mode —
        // branch naming is a side-effect-free housekeeping action, not a code change
        cmd.args(["--allowedTools", "mcp__korlap__rename_branch,WebSearch,WebFetch"]);
    } else {
        cmd.arg("--dangerously-skip-permissions");
    }

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
        let mut system_prompt = format!(
            "You are working inside Korlap, a Mac app that runs coding agents in parallel.\n\
             Your working directory is already set to the workspace. Do not cd into it — you are already there.\n\
             Target branch: {}\n\
             Base branch: {}\n\
             You have access to Korlap tools via MCP. Use the rename_branch tool to give your branch a meaningful name based on the task. Use conventional prefixes: feat/, fix/, refactor/, chore/, docs/. Keep names concise (<30 chars).\n\
             IMPORTANT: Renaming the branch is your FIRST priority. Call rename_branch BEFORE reading files, writing code, or running any commands. Parse the user's request, pick a name, and rename immediately.\n\
             If the task scope changes mid-conversation, rename the branch again to reflect the new direction.\n\
             Keep all changes on the target branch. Do not modify other branches.",
            ws_branch,
            base_branch,
        );
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
        // Rewrite SSH git URLs to HTTPS with token auth so git push works
        // without needing the right SSH key for this specific account
        cmd.env(
            "GIT_CONFIG_PARAMETERS",
            format!(
                "'url.https://oauth2:{}@github.com/.insteadOf=git@github.com:'",
                token
            ),
        );
    }

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

// ── Terminal commands ────────────────────────────────────────────────

#[tauri::command]
pub fn open_terminal(
    workspace_id: String,
    on_data: Channel<Vec<u8>>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        if st.terminals.contains_key(&workspace_id) {
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
            workspace_id.clone(),
            crate::state::TerminalHandle {
                writer,
                child,
                master: pair.master,
            },
        );
    }

    // Stream PTY output to frontend via Channel
    let ws_id = workspace_id.clone();
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
        tracing::info!("Terminal reader exited for {}", ws_id);
    });

    Ok(())
}

#[tauri::command]
pub fn write_terminal(
    workspace_id: String,
    data: Vec<u8>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let handle = st
        .terminals
        .get_mut(&workspace_id)
        .ok_or("No terminal open for this workspace")?;

    std::io::Write::write_all(&mut handle.writer, &data)
        .map_err(|e| format!("Failed to write to PTY: {}", e))?;

    Ok(())
}

#[tauri::command]
pub fn resize_terminal(
    workspace_id: String,
    rows: u16,
    cols: u16,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let handle = st
        .terminals
        .get_mut(&workspace_id)
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
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(mut handle) = st.terminals.remove(&workspace_id) {
        let _ = handle.child.kill();
        let _ = handle.child.wait();
    }
    Ok(())
}

// ── Suggested replies via AI ─────────────────────────────────────────

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


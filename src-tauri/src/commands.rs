use crate::state::{AgentHandle, AppState, RepoInfo, WorkspaceInfo, WorkspaceStatus};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::io::BufRead;
use std::path::Path;
use std::sync::{Arc, Mutex};
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

        let path = std::process::Command::new("zsh")
            .args(["-l", "-c", "echo $PATH"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if s.is_empty() { None } else { Some(s) }
            });

        ShellEnv { ssh_auth_sock, home, path }
    })
}

struct ShellEnv {
    ssh_auth_sock: Option<String>,
    home: Option<String>,
    path: Option<String>,
}

/// Inject essential shell environment vars that Tauri apps launched from
/// Finder/Dock don't inherit (SSH agent, PATH, HOME, etc.)
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
                            .map(|s| s.replace(worktree_path, "."));

                        let input_preview = block.get("input").and_then(|input| {
                            let strip = |s: &str| -> String {
                                s.replace(worktree_path, ".")
                            };
                            if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
                                Some(strip(fp))
                            } else if let Some(cmd) =
                                input.get("command").and_then(|c| c.as_str())
                            {
                                // Strip worktree path AND collapse "cd <path> && " prefix
                                let cleaned = strip(cmd);
                                let cleaned = if cleaned.starts_with("cd . && ") {
                                    cleaned[8..].to_string()
                                } else if cleaned.starts_with("cd . ; ") {
                                    cleaned[7..].to_string()
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
        default_branch: Some(default_branch.clone()),
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

// ── Workspace commands ───────────────────────────────────────────────

#[tauri::command]
pub async fn create_workspace(
    repo_id: String,
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
pub async fn archive_workspace(
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
        if ws.status == WorkspaceStatus::Archived {
            return Ok(());
        }
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), repo.path.clone(), ws.name.clone(), ws.repo_id.clone())
    };

    // Run archive script if configured
    {
        let st = state.lock().map_err(|e| e.to_string())?;
        if let Some(settings) = st.repo_settings.get(&repo_id) {
            if !settings.archive_script.trim().is_empty() {
                tracing::info!("Running archive script for workspace {}", ws_name);
                let mut archive_cmd = std::process::Command::new("zsh");
                archive_cmd.args(["-c", &settings.archive_script]);
                archive_cmd.current_dir(&worktree_path);
                archive_cmd.env("KORLAP_WORKSPACE_NAME", &ws_name);
                archive_cmd.env("KORLAP_WORKSPACE_PATH", &worktree_path.to_string_lossy().to_string());
                archive_cmd.env("KORLAP_ROOT_PATH", repo_path.to_string_lossy().to_string());
                inject_shell_env(&mut archive_cmd);
                let _ = archive_cmd.output();
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

    if ws.status == WorkspaceStatus::Archived {
        return Err("Cannot rename an archived workspace".into());
    }

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
    let (worktree_path, branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        (ws.worktree_path.clone(), ws.branch.clone())
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

// ── Image commands ───────────────────────────────────────────────────

/// Save base64-encoded image data to a file in the workspace directory.
/// Returns the absolute path to the saved image.
#[tauri::command]
pub fn save_image(
    workspace_id: String,
    data: String,
    extension: String,
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

    // Decode base64
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|e| format!("Invalid base64 data: {}", e))?;

    // Save to .korlap-images/ inside the worktree (gitignored by convention)
    let images_dir = worktree_path.join(".korlap-images");
    std::fs::create_dir_all(&images_dir)
        .map_err(|e| format!("Failed to create images dir: {}", e))?;

    // Ensure .korlap-images is gitignored
    let gitignore_path = worktree_path.join(".gitignore");
    let needs_entry = if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
        !content.lines().any(|l| l.trim() == ".korlap-images/")
    } else {
        true
    };
    if needs_entry {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&gitignore_path)
            .map_err(|e| format!("Failed to update .gitignore: {}", e))?;
        use std::io::Write;
        writeln!(file, "\n.korlap-images/")
            .map_err(|e| format!("Failed to write .gitignore: {}", e))?;
    }

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
    let (worktree_path, gh_profile, repo_id, ws_branch, repo_path) = {
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
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (
            ws.worktree_path.clone(),
            repo.gh_profile.clone(), // Always use repo's current profile, not stale workspace snapshot
            ws.repo_id.clone(),
            ws.branch.clone(),
            repo.path.clone(),
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

    // Build claude command
    let mut cmd = std::process::Command::new("claude");
    cmd.arg("-p").arg(&prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    // Permission mode: plan mode uses --permission-mode plan, otherwise bypass all
    if plan_mode {
        cmd.args(["--permission-mode", "plan"]);
        // Allow rename_branch to execute without permission in plan mode —
        // branch naming is a side-effect-free housekeeping action, not a code change
        cmd.args(["--allowedTools", "mcp__korlap__rename_branch"]);
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
        let system_prompt = format!(
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
    cmd.cwd(&worktree_path);

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


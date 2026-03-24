use crate::state::AppState;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};

use super::helpers::{
    extract_gh_nwo, inject_shell_env, resolve_gh_token, strip_ansi, gh_cmd_with_auth,
};

/// PID of the in-flight `gh auth login` process, or 0 if none.
static GH_AUTH_PID: AtomicU32 = AtomicU32::new(0);

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

    let profiles = list_gh_profiles()?;
    let authenticated = !profiles.is_empty();

    Ok(GhCliStatus {
        installed,
        authenticated,
        profiles,
    })
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

/// Shared helper: clone a repo and register it in app state.
pub(super) fn clone_and_register(
    clone_url: &str,
    repo_name: &str,
    dest_path: Option<&str>,
    profile: &str,
    token: &Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<super::repo::RepoDetail, String> {
    let dest = if let Some(p) = dest_path {
        std::path::PathBuf::from(p)
    } else {
        let home = std::env::var("HOME").map_err(|_| "Cannot determine HOME directory")?;
        std::path::PathBuf::from(home).join("Developer").join(repo_name)
    };

    if dest.exists() {
        if dest.join(".git").exists() {
            return super::repo::register_repo(dest, Some(profile.to_string()), state);
        }
        return Err(format!("Destination already exists: {}", dest.display()));
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

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
    cmd.args(["clone", clone_url, &dest.to_string_lossy()]);
    inject_shell_env(&mut cmd);

    let output = cmd.output().map_err(|e| format!("Failed to run git clone: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr));
    }

    super::repo::register_repo(dest, Some(profile.to_string()), state)
}

#[tauri::command]
pub fn clone_repo(
    clone_url: String,
    repo_name: String,
    dest_path: Option<String>,
    profile: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<super::repo::RepoDetail, String> {
    let token = resolve_gh_token(&Some(profile.clone()));
    clone_and_register(
        &clone_url,
        &repo_name,
        dest_path.as_deref(),
        &profile,
        &token,
        state,
    )
}

// ── Create repo on GitHub ────────────────────────────────────────────

#[derive(Clone, serde::Deserialize)]
pub struct CreateRepoOptions {
    pub name: String,
    pub private: bool,
    pub description: Option<String>,
    pub add_readme: bool,
}

#[tauri::command]
pub fn create_gh_repo(
    options: CreateRepoOptions,
    profile: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<super::repo::RepoDetail, String> {
    let token = resolve_gh_token(&Some(profile.clone()));

    let full_name = format!("{}/{}", profile, options.name);

    // Create the repo on GitHub
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["repo", "create", &full_name]);
    if options.private {
        cmd.arg("--private");
    } else {
        cmd.arg("--public");
    }
    if let Some(ref desc) = options.description {
        if !desc.is_empty() {
            cmd.args(["-d", desc]);
        }
    }
    if options.add_readme {
        cmd.arg("--add-readme");
    }
    inject_shell_env(&mut cmd);
    if let Some(ref t) = token {
        cmd.env("GH_TOKEN", t);
    }

    let output = cmd.output().map_err(|e| format!("Failed to run gh: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!("Failed to create repository: {}", stderr));
    }

    // Clone the newly created repo
    let clone_url = format!("git@github.com:{}.git", full_name);
    clone_and_register(
        &clone_url,
        &options.name,
        None,
        &profile,
        &token,
        state,
    )
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
    let (worktree_path, fallback_branch, gh_profile) = {
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

    // Use actual git branch — metadata may be stale after a rename failure
    let branch = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&worktree_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or(fallback_branch);

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

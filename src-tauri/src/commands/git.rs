use crate::git_provider::SharedProviderRegistry;
use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::State;

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

// ── Git diff/changed files ───────────────────────────────────────────

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
        let base = ws.base_branch.clone()
            .or_else(|| repo.default_branch.clone())
            .unwrap_or_else(|| "main".to_string());
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
        let base = ws.base_branch.clone()
            .or_else(|| repo.default_branch.clone())
            .unwrap_or_else(|| "main".to_string());
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

// ── Direct git commands ──────────────────────────────────────────────

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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    let (worktree_path, fallback_branch, gh_profile, repo_path) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (ws.worktree_path.clone(), ws.branch.clone(), repo.gh_profile.clone(), repo.path.clone())
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let gh_token = provider.resolve_token(&gh_profile);

        // Use actual git branch — metadata may be stale after a rename failure
        let branch = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&worktree_path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or(fallback_branch);

        let mut cmd = provider.git_cmd_with_auth(&worktree_path, &gh_token);
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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<u32, String> {
    let (repo_path, gh_profile, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let branch = repo.default_branch.clone()
            .unwrap_or_else(|| "main".to_string());
        (repo.path.clone(), repo.gh_profile.clone(), branch)
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let gh_token = provider.resolve_token(&gh_profile);

        // Fetch latest
        let mut fetch = provider.git_cmd_with_auth(&repo_path, &gh_token);
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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    let (repo_path, gh_profile, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let branch = repo.default_branch.clone()
            .unwrap_or_else(|| "main".to_string());
        (repo.path.clone(), repo.gh_profile.clone(), branch)
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let gh_token = provider.resolve_token(&gh_profile);

        // 1. Fetch latest from origin
        let mut fetch = provider.git_cmd_with_auth(&repo_path, &gh_token);
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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<BaseUpdateStatus, String> {
    let (worktree_path, base_branch, gh_profile, repo_path) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = ws.base_branch.clone()
            .or_else(|| repo.default_branch.clone())
            .unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base, repo.gh_profile.clone(), repo.path.clone())
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let gh_token = provider.resolve_token(&gh_profile);

        // Fetch latest from origin for the base branch
        let mut fetch_cmd = provider.git_cmd_with_auth(&worktree_path, &gh_token);
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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    let (worktree_path, base_branch, gh_profile, repo_path) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st.workspaces.get(&workspace_id).ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = ws.base_branch.clone()
            .or_else(|| repo.default_branch.clone())
            .unwrap_or_else(|| "main".to_string());
        (ws.worktree_path.clone(), base, repo.gh_profile.clone(), repo.path.clone())
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let gh_token = provider.resolve_token(&gh_profile);

        // Fetch latest
        let mut fetch_cmd = provider.git_cmd_with_auth(&worktree_path, &gh_token);
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

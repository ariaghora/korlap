use crate::git_provider::SharedProviderRegistry;
use crate::state::{AppState, WorkspaceInfo, WorkspaceStatus};
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

use super::helpers::{detect_default_branch, inject_shell_env, now_unix};

#[derive(Clone, serde::Serialize)]
pub struct StagingResult {
    pub workspace: WorkspaceInfo,
    pub merged_branches: Vec<String>,
    pub conflicting_branches: Vec<String>,
}

#[tauri::command]
pub async fn create_staging_workspace(
    repo_id: String,
    branch_names: Vec<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<StagingResult, String> {
    let (repo_path, gh_profile, worktree_base) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), repo.gh_profile.clone(), st.worktree_dir())
    };

    let provider = providers.for_repo(&repo_path);
    let gh_token = provider.resolve_token(&gh_profile);

    let base_branch = detect_default_branch(&repo_path)?;

    // Fetch origin/<default_branch>
    let mut fetch_cmd = provider.git_cmd_with_auth(&repo_path, &gh_token);
    fetch_cmd.args(["fetch", "origin", &base_branch]);
    let fetch_output = fetch_cmd
        .output()
        .map_err(|e| format!("Failed to run git fetch: {}", e))?;
    if !fetch_output.status.success() {
        let stderr = String::from_utf8_lossy(&fetch_output.stderr);
        return Err(format!("Failed to fetch origin/{}: {}", base_branch, stderr.trim()));
    }

    // If staging worktree already exists, remove it first
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        let existing_id = st
            .workspaces
            .values()
            .find(|w| w.branch == "korlap/staging" && w.repo_id == repo_id)
            .map(|w| w.id.clone());

        if let Some(existing_id) = existing_id {
            // Kill agent if running
            if let Some(mut handle) = st.agents.remove(&existing_id) {
                let _ = handle.child.kill();
                let _ = handle.child.wait();
            }
            // Kill all terminals for this workspace
            super::terminal::kill_workspace_terminals(&mut st.terminals, &existing_id);

            let existing_path = st
                .workspaces
                .get(&existing_id)
                .map(|w| w.worktree_path.clone());

            // Remove worktree
            if let Some(ref path) = existing_path {
                if path.exists() {
                    let mut rm_cmd = std::process::Command::new("git");
                    rm_cmd.args(["worktree", "remove", "--force"])
                        .arg(path)
                        .current_dir(&repo_path);
                    inject_shell_env(&mut rm_cmd);
                    let _ = rm_cmd.output();
                }
            }

            // Delete branch
            let mut branch_del = std::process::Command::new("git");
            branch_del
                .args(["branch", "-D", "korlap/staging"])
                .current_dir(&repo_path);
            inject_shell_env(&mut branch_del);
            let _ = branch_del.output();

            // Clean up state
            st.delete_workspace_data(&existing_id);
            st.workspaces.remove(&existing_id);
        }
    }

    // Create the staging worktree (per-repo path to avoid conflicts when multiple repos have autopilot)
    let staging_dir = format!("staging-{}", &repo_id[..repo_id.len().min(8)]);
    let worktree_path = worktree_base.join(&staging_dir);
    let start_point = format!("origin/{}", base_branch);

    // Always clean up stale worktree/branch even if not tracked in state
    if worktree_path.exists() {
        let mut rm_wt = std::process::Command::new("git");
        rm_wt.args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&repo_path);
        inject_shell_env(&mut rm_wt);
        let _ = rm_wt.output();
    }
    // Migration: clean up old shared "staging" path from before per-repo paths
    let old_staging_path = worktree_base.join("staging");
    if old_staging_path.exists() {
        let mut rm_old = std::process::Command::new("git");
        rm_old.args(["worktree", "remove", "--force"])
            .arg(&old_staging_path)
            .current_dir(&repo_path);
        inject_shell_env(&mut rm_old);
        let _ = rm_old.output();
    }
    let mut prune_cmd = std::process::Command::new("git");
    prune_cmd.args(["worktree", "prune"])
        .current_dir(&repo_path);
    inject_shell_env(&mut prune_cmd);
    let _ = prune_cmd.output();
    let mut stale_del = std::process::Command::new("git");
    stale_del
        .args(["branch", "-D", "korlap/staging"])
        .current_dir(&repo_path);
    inject_shell_env(&mut stale_del);
    let _ = stale_del.output();

    std::fs::create_dir_all(worktree_path.parent().unwrap_or(&worktree_path))
        .map_err(|e| e.to_string())?;

    let mut wt_add = std::process::Command::new("git");
    wt_add.args(["worktree", "add", "-b", "korlap/staging"])
        .arg(&worktree_path)
        .arg(&start_point)
        .current_dir(&repo_path);
    inject_shell_env(&mut wt_add);
    let output = wt_add.output()
        .map_err(|e| format!("Failed to run git worktree add: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git worktree add failed: {}", stderr.trim()));
    }

    // Merge each branch
    let mut merged_branches = Vec::new();
    let mut conflicting_branches = Vec::new();

    for branch in &branch_names {
        // Fetch the branch
        let mut fetch_br = provider.git_cmd_with_auth(&repo_path, &gh_token);
        fetch_br.args(["fetch", "origin", branch]);
        let fetch_out = fetch_br
            .output()
            .map_err(|e| format!("Failed to fetch branch {}: {}", branch, e))?;
        if !fetch_out.status.success() {
            let stderr = String::from_utf8_lossy(&fetch_out.stderr);
            tracing::warn!("Failed to fetch origin/{}: {}", branch, stderr.trim());
            conflicting_branches.push(branch.clone());
            continue;
        }

        // Merge in the staging worktree
        let merge_ref = format!("origin/{}", branch);
        let mut merge_cmd = std::process::Command::new("git");
        merge_cmd
            .args(["merge", &merge_ref, "--no-edit"])
            .current_dir(&worktree_path);
        inject_shell_env(&mut merge_cmd);
        let merge_out = merge_cmd
            .output()
            .map_err(|e| format!("Failed to merge {}: {}", branch, e))?;

        if merge_out.status.success() {
            merged_branches.push(branch.clone());
        } else {
            // Abort the failed merge
            let mut abort_cmd = std::process::Command::new("git");
            abort_cmd
                .args(["merge", "--abort"])
                .current_dir(&worktree_path);
            inject_shell_env(&mut abort_cmd);
            let _ = abort_cmd.output();

            conflicting_branches.push(branch.clone());
            tracing::warn!("Merge conflict for branch {}, skipping", branch);
        }
    }

    // Create workspace entry
    let id = Uuid::new_v4().to_string();
    let ws = WorkspaceInfo {
        id: id.clone(),
        name: "staging".to_string(),
        branch: "korlap/staging".to_string(),
        worktree_path,
        repo_id: repo_id.clone(),
        gh_profile,
        status: WorkspaceStatus::Waiting,
        created_at: now_unix(),
        task_title: Some("Staging".to_string()),
        task_description: None,
        source_todo_id: None,
        custom_branch: false,
        provider_override: None,
        source_pr: None,
        source_prs: None,
        base_branch: None,
    };

    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.workspaces.insert(id, ws.clone());
    st.save_workspaces()?;

    tracing::info!(
        "Created staging workspace: merged={:?}, conflicting={:?}",
        merged_branches,
        conflicting_branches
    );

    Ok(StagingResult {
        workspace: ws,
        merged_branches,
        conflicting_branches,
    })
}

#[tauri::command]
pub async fn remove_staging_workspace(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (ws_id, worktree_path, repo_path) = {
        let mut st = state.lock().map_err(|e| e.to_string())?;

        let ws = st
            .workspaces
            .values()
            .find(|w| w.branch == "korlap/staging" && w.repo_id == repo_id)
            .ok_or("Staging workspace not found")?;

        let ws_id = ws.id.clone();
        let worktree_path = ws.worktree_path.clone();

        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let repo_path = repo.path.clone();

        // Kill agent if running
        if let Some(mut handle) = st.agents.remove(&ws_id) {
            let _ = handle.child.kill();
            let _ = handle.child.wait();
        }

        // Kill all terminals for this workspace
        super::terminal::kill_workspace_terminals(&mut st.terminals, &ws_id);

        (ws_id, worktree_path, repo_path)
    };

    // Remove worktree
    if worktree_path.exists() {
        let mut rm_wt = std::process::Command::new("git");
        rm_wt.args(["worktree", "remove", "--force"])
            .arg(&worktree_path)
            .current_dir(&repo_path);
        inject_shell_env(&mut rm_wt);
        let output = rm_wt.output()
            .map_err(|e| format!("Failed to remove worktree: {}", e))?;

        if !output.status.success() {
            let mut prune = std::process::Command::new("git");
            prune.args(["worktree", "prune"])
                .current_dir(&repo_path);
            inject_shell_env(&mut prune);
            let _ = prune.output();
        }
    } else {
        let mut prune = std::process::Command::new("git");
        prune.args(["worktree", "prune"])
            .current_dir(&repo_path);
        inject_shell_env(&mut prune);
        let _ = prune.output();
    }

    // Delete the staging branch
    let mut branch_del = std::process::Command::new("git");
    branch_del
        .args(["branch", "-D", "korlap/staging"])
        .current_dir(&repo_path);
    inject_shell_env(&mut branch_del);
    let _ = branch_del.output();

    // Clean up state
    let mut st = state.lock().map_err(|e| e.to_string())?;
    st.delete_workspace_data(&ws_id);
    st.workspaces.remove(&ws_id);
    st.save_workspaces()?;

    tracing::info!("Removed staging workspace for repo {}", repo_id);
    Ok(())
}

use crate::git_provider::{
    CliStatus, CreateRepoOptions, GitServiceProvider, PrDetail, PrEntry, PrStatus,
    RepoEntry, ServiceProfile, SharedProviderRegistry,
};
use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, State};

use super::helpers::inject_shell_env;

// ── GitHub profile commands ──────────────────────────────────────────

#[tauri::command]
pub fn list_gh_profiles(
    providers: State<'_, SharedProviderRegistry>,
) -> Result<Vec<ServiceProfile>, String> {
    providers.github().list_profiles()
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

#[tauri::command]
pub fn check_gh_cli(
    providers: State<'_, SharedProviderRegistry>,
) -> Result<CliStatus, String> {
    providers.github().check_cli()
}

#[tauri::command]
pub async fn gh_auth_login(
    app_handle: AppHandle,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    let providers = providers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || providers.github().auth_login(app_handle))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn cancel_gh_auth_login(
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    providers.github().cancel_auth_login()
}

#[tauri::command]
pub async fn list_gh_repos(
    profile: String,
    search: Option<String>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<Vec<RepoEntry>, String> {
    let providers = providers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.github();
        let token = provider.resolve_token(&Some(profile.clone()));
        provider.list_repos(&profile, search.as_deref(), &token)
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
    provider: &dyn GitServiceProvider,
    state: Arc<Mutex<AppState>>,
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
        return Err(format!(
            "Destination already exists: {}",
            dest.display()
        ));
    }

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let mut cmd = std::process::Command::new("git");
    provider.inject_git_auth(&mut cmd, token);
    cmd.args(["clone", clone_url, &dest.to_string_lossy()]);
    inject_shell_env(&mut cmd);

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run git clone: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git clone failed: {}", stderr));
    }

    // Empty repos (e.g. created on GitHub without a README) have no commits and
    // no remote refs, which causes detect_default_branch to fail downstream.
    // Bootstrap with an initial commit + push so the default branch exists.
    let is_empty = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dest)
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(false);

    if is_empty {
        tracing::info!(
            "Cloned repo is empty, creating initial commit for {}",
            repo_name
        );

        std::fs::write(dest.join("README.md"), format!("# {}\n", repo_name))
            .map_err(|e| format!("Failed to create README: {}", e))?;

        let add_out = std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&dest)
            .output()
            .map_err(|e| format!("git add failed: {}", e))?;
        if !add_out.status.success() {
            return Err("Failed to stage initial README".to_string());
        }

        let commit_out = std::process::Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&dest)
            .output()
            .map_err(|e| format!("git commit failed: {}", e))?;
        if !commit_out.status.success() {
            let stderr = String::from_utf8_lossy(&commit_out.stderr);
            return Err(format!("Failed to create initial commit: {}", stderr));
        }

        // Determine the local branch name (respects user's init.defaultBranch)
        let branch = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&dest)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "main".to_string());

        // Push to establish the default branch on the remote
        let mut push_cmd = provider.git_cmd_with_auth(&dest, token);
        push_cmd.args(["push", "-u", "origin", &branch]);

        let push_out = push_cmd
            .output()
            .map_err(|e| format!("git push failed: {}", e))?;
        if !push_out.status.success() {
            let stderr = String::from_utf8_lossy(&push_out.stderr);
            return Err(format!("Failed to push initial commit: {}", stderr));
        }
    }

    super::repo::register_repo(dest, Some(profile.to_string()), state)
}

#[tauri::command]
pub async fn clone_repo(
    clone_url: String,
    repo_name: String,
    dest_path: Option<String>,
    profile: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<super::repo::RepoDetail, String> {
    let state = state.inner().clone();
    let providers = providers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.github();
        let token = provider.resolve_token(&Some(profile.clone()));
        clone_and_register(
            &clone_url,
            &repo_name,
            dest_path.as_deref(),
            &profile,
            &token,
            provider,
            state,
        )
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Create repo on GitHub ────────────────────────────────────────────

#[tauri::command]
pub async fn create_gh_repo(
    options: CreateRepoOptions,
    profile: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<super::repo::RepoDetail, String> {
    let state = state.inner().clone();
    let providers = providers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.github();
        let token = provider.resolve_token(&Some(profile.clone()));
        let clone_url = provider.create_repo(&options, &profile, &token)?;
        clone_and_register(
            &clone_url,
            &options.name,
            None,
            &profile,
            &token,
            provider,
            state,
        )
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Check which connected GH profile has access to the repo at `path`.
/// Returns the profile login that can access it, or None.
#[tauri::command]
pub async fn check_repo_gh_access(
    path: String,
    profiles: Vec<String>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<Option<String>, String> {
    let providers = providers.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = std::path::PathBuf::from(&path)
            .canonicalize()
            .map_err(|e| format!("Invalid path: {}", e))?;
        providers.github().check_repo_access(&path, &profiles)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── PR commands ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_pr_status(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<PrStatus, String> {
    let (worktree_path, fallback_branch, gh_profile, repo_path) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (
            ws.worktree_path.clone(),
            ws.branch.clone(),
            repo.gh_profile.clone(),
            repo.path.clone(),
        )
    };

    let providers = providers.inner().clone();

    // Use actual git branch -- metadata may be stale after a rename failure
    let branch = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(&worktree_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or(fallback_branch);

    // Run in a blocking thread so it doesn't hold up the IPC queue
    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let token = provider.resolve_token(&gh_profile);
        provider.get_pr_status(&worktree_path, &branch, &token)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
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
    providers: State<'_, SharedProviderRegistry>,
) -> Result<(), String> {
    let (worktree_path, gh_profile, repo_path) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        (
            ws.worktree_path.clone(),
            repo.gh_profile.clone(),
            repo.path.clone(),
        )
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let token = provider.resolve_token(&gh_profile);
        provider.merge_pr(&worktree_path, pr_number, &token)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── PR listing for checkout ─────────────────────────────────────────

#[tauri::command]
pub async fn list_repo_prs(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<Vec<PrEntry>, String> {
    let (repo_path, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), repo.gh_profile.clone())
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let token = provider.resolve_token(&gh_profile);
        let nwo = provider
            .extract_repo_id(&repo_path)
            .ok_or("Could not determine owner/repo from remote URL")?;
        provider.list_prs(&repo_path, &nwo, &token)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── PR detail for workspace creation ────────────────────────────────

/// Fetch detailed PR metadata (body included) for a single PR by number.
/// Used by create_workspace_from_pr to get PR context for agent injection.
#[tauri::command]
pub async fn get_pr_detail(
    repo_id: String,
    pr_number: i64,
    state: State<'_, Arc<Mutex<AppState>>>,
    providers: State<'_, SharedProviderRegistry>,
) -> Result<PrDetail, String> {
    let (repo_path, gh_profile) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (repo.path.clone(), repo.gh_profile.clone())
    };

    let providers = providers.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let provider = providers.for_repo(&repo_path);
        let token = provider.resolve_token(&gh_profile);
        let nwo = provider
            .extract_repo_id(&repo_path)
            .ok_or("Could not determine owner/repo from remote URL")?;
        provider.get_pr_detail(&repo_path, &nwo, pr_number, &token)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

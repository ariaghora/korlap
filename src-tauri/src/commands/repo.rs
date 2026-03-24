use crate::state::{AppState, RepoInfo};
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

use super::helpers::{detect_default_branch, repo_display_name};

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

/// Shared helper to register a repo in app state (used by add_repo and clone_repo).
pub(super) fn register_repo(
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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Running,
    Waiting,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: String,
    pub path: PathBuf,
    pub gh_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: String,
    pub name: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub repo_id: String,
    pub gh_profile: Option<String>,
    pub status: WorkspaceStatus,
    pub created_at: u64,
}

pub struct AgentHandle {
    pub child: std::process::Child,
}

pub struct AppState {
    pub repos: HashMap<String, RepoInfo>,
    pub workspaces: HashMap<String, WorkspaceInfo>,
    pub agents: HashMap<String, AgentHandle>,
    pub session_ids: HashMap<String, String>,
    pub data_dir: PathBuf,
}

impl AppState {
    pub fn load(&mut self) -> Result<(), String> {
        let repos_path = self.data_dir.join("repos.json");
        if !repos_path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&repos_path).map_err(|e| e.to_string())?;
        let repos: Vec<RepoInfo> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        for repo in repos {
            if repo.path.exists() {
                self.load_workspaces_for_repo(&repo)?;
                self.repos.insert(repo.id.clone(), repo);
            } else {
                tracing::warn!("Repo path no longer exists: {}", repo.path.display());
            }
        }
        Ok(())
    }

    fn load_workspaces_for_repo(&mut self, repo: &RepoInfo) -> Result<(), String> {
        let ws_path = repo.path.join(".korlap").join("workspaces.json");
        if !ws_path.exists() {
            return Ok(());
        }
        let data = std::fs::read_to_string(&ws_path).map_err(|e| e.to_string())?;
        let workspaces: Vec<WorkspaceInfo> =
            serde_json::from_str(&data).map_err(|e| e.to_string())?;
        for mut ws in workspaces {
            if ws.status == WorkspaceStatus::Running {
                ws.status = WorkspaceStatus::Waiting;
            }
            self.workspaces.insert(ws.id.clone(), ws);
        }
        Ok(())
    }

    pub fn save_repos(&self) -> Result<(), String> {
        let repos: Vec<&RepoInfo> = self.repos.values().collect();
        let data = serde_json::to_string_pretty(&repos).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join("repos.json"), data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn save_workspaces(&self, repo_id: &str) -> Result<(), String> {
        let repo = self.repos.get(repo_id).ok_or("Repo not found")?;
        let workspaces: Vec<&WorkspaceInfo> = self
            .workspaces
            .values()
            .filter(|w| w.repo_id == repo_id)
            .collect();
        let korlap_dir = repo.path.join(".korlap");
        std::fs::create_dir_all(&korlap_dir).map_err(|e| e.to_string())?;
        let data = serde_json::to_string_pretty(&workspaces).map_err(|e| e.to_string())?;
        std::fs::write(korlap_dir.join("workspaces.json"), data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn is_git_repo(path: &Path) -> Result<(), String> {
        let output = std::process::Command::new("git")
            .arg("rev-parse")
            .arg("--git-dir")
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?;
        if !output.status.success() {
            return Err(format!("{} is not a git repository", path.display()));
        }
        Ok(())
    }
}

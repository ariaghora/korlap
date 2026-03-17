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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoSettings {
    #[serde(default)]
    pub setup_script: String,
    #[serde(default)]
    pub run_script: String,
    #[serde(default)]
    pub archive_script: String,
}

pub struct TerminalHandle {
    pub writer: Box<dyn std::io::Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
}

/// All persistent state lives under Tauri's app data dir.
/// Zero files are written to the user's managed repos.
///
/// Layout:
///   <data_dir>/
///     repos.json
///     workspaces.json
///     sessions.json
///     workspaces/<workspace-id>/   ← git worktree
///     messages/<workspace-id>.json
pub struct AppState {
    pub repos: HashMap<String, RepoInfo>,
    pub workspaces: HashMap<String, WorkspaceInfo>,
    pub agents: HashMap<String, AgentHandle>,
    pub session_ids: HashMap<String, String>,
    pub repo_settings: HashMap<String, RepoSettings>,
    pub data_dir: PathBuf,
    pub mcp_api_port: u16,
    pub terminals: HashMap<String, TerminalHandle>,
}

impl AppState {
    pub fn load(&mut self) -> Result<(), String> {
        // Load repos
        let repos_path = self.data_dir.join("repos.json");
        if repos_path.exists() {
            let data = std::fs::read_to_string(&repos_path).map_err(|e| e.to_string())?;
            let repos: Vec<RepoInfo> =
                serde_json::from_str(&data).map_err(|e| e.to_string())?;
            for repo in repos {
                if repo.path.exists() {
                    self.repos.insert(repo.id.clone(), repo);
                } else {
                    tracing::warn!("Repo path no longer exists: {}", repo.path.display());
                }
            }
        }

        // Load workspaces
        let ws_path = self.data_dir.join("workspaces.json");
        if ws_path.exists() {
            let data = std::fs::read_to_string(&ws_path).map_err(|e| e.to_string())?;
            let workspaces: Vec<WorkspaceInfo> =
                serde_json::from_str(&data).map_err(|e| e.to_string())?;
            for mut ws in workspaces {
                // Reset running → waiting on restart (agent is dead)
                if ws.status == WorkspaceStatus::Running {
                    ws.status = WorkspaceStatus::Waiting;
                }
                self.workspaces.insert(ws.id.clone(), ws);
            }
        }

        // Load session IDs
        let sessions_path = self.data_dir.join("sessions.json");
        if sessions_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&sessions_path) {
                if let Ok(sessions) =
                    serde_json::from_str::<HashMap<String, String>>(&data)
                {
                    self.session_ids.extend(sessions);
                }
            }
        }

        // Load repo settings
        let settings_path = self.data_dir.join("repo_settings.json");
        if settings_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&settings_path) {
                if let Ok(settings) =
                    serde_json::from_str::<HashMap<String, RepoSettings>>(&data)
                {
                    self.repo_settings.extend(settings);
                }
            }
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

    pub fn save_workspaces(&self) -> Result<(), String> {
        let workspaces: Vec<&WorkspaceInfo> = self.workspaces.values().collect();
        let data = serde_json::to_string_pretty(&workspaces).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join("workspaces.json"), data).map_err(|e| e.to_string())?;

        // Persist session IDs
        if !self.session_ids.is_empty() {
            let data =
                serde_json::to_string_pretty(&self.session_ids).map_err(|e| e.to_string())?;
            std::fs::write(self.data_dir.join("sessions.json"), data)
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn save_repo_settings(&self) -> Result<(), String> {
        let data =
            serde_json::to_string_pretty(&self.repo_settings).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join("repo_settings.json"), data)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Path where worktrees are created
    pub fn worktree_dir(&self) -> PathBuf {
        self.data_dir.join("workspaces")
    }

    /// Path where messages are stored
    pub fn messages_dir(&self) -> PathBuf {
        self.data_dir.join("messages")
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

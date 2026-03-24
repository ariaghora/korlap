use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceStatus {
    Running,
    Waiting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoInfo {
    pub id: String,
    pub path: PathBuf,
    pub gh_profile: Option<String>,
    #[serde(default)]
    pub default_branch: Option<String>,
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
    #[serde(default)]
    pub task_title: Option<String>,
    #[serde(default)]
    pub task_description: Option<String>,
    #[serde(default)]
    pub source_todo_id: Option<String>,
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
    #[serde(default, alias = "archive_script")]
    pub remove_script: String,
    #[serde(default)]
    pub pr_message: String,
    #[serde(default)]
    pub review_message: String,
    #[serde(default)]
    pub default_thinking: bool,
    #[serde(default)]
    pub default_plan: bool,
    #[serde(default)]
    pub system_prompt: String,
}

pub struct TerminalHandle {
    pub writer: Box<dyn std::io::Write + Send>,
    pub child: Box<dyn portable_pty::Child + Send>,
    pub master: Box<dyn portable_pty::MasterPty + Send>,
}

// ── Knowledge base (warm context) ────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ContextBuildStatus {
    #[default]
    NotBuilt,
    Building,
    Built,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextMeta {
    #[serde(default)]
    pub include_globs: Vec<String>,
    #[serde(default)]
    pub exclude_globs: Vec<String>,
    #[serde(default)]
    pub build_status: ContextBuildStatus,
    #[serde(default)]
    pub last_built_at: Option<u64>,
    #[serde(default)]
    pub invariant_count: u32,
    #[serde(default)]
    pub fact_count: u32,
    #[serde(default)]
    pub context_entry_count: u32,
    #[serde(default)]
    pub contradiction_count: u32,
    #[serde(default)]
    pub precheck_model: String,
    #[serde(default)]
    pub built_at_commit: Option<String>,
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
    pub context_meta: HashMap<String, ContextMeta>,
    pub context_agents: HashMap<String, AgentHandle>,
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

        // Load workspaces — migrate old "archived" entries by deleting their data
        let ws_path = self.data_dir.join("workspaces.json");
        if ws_path.exists() {
            let data = std::fs::read_to_string(&ws_path).map_err(|e| e.to_string())?;
            // Use Value to handle the old "archived" status that no longer deserializes
            let raw: Vec<serde_json::Value> =
                serde_json::from_str(&data).map_err(|e| e.to_string())?;
            let mut needs_persist = false;
            for entry in raw {
                let status = entry.get("status").and_then(|s| s.as_str()).unwrap_or("");
                if status == "archived" {
                    // Migration: clean up leftover data from old archived workspaces
                    if let Some(id) = entry.get("id").and_then(|s| s.as_str()) {
                        tracing::info!("Migrating archived workspace {}: removing leftover data", id);
                        let _ = std::fs::remove_file(self.messages_dir().join(format!("{}.json", id)));
                        self.session_ids.remove(id);
                    }
                    continue; // Don't load archived workspaces
                }
                let mut ws: WorkspaceInfo =
                    serde_json::from_value(entry).map_err(|e| e.to_string())?;
                // Reset running → waiting on restart (agent is dead)
                if ws.status == WorkspaceStatus::Running {
                    ws.status = WorkspaceStatus::Waiting;
                }
                // Migration: backfill task_title from the first hidden user message
                if ws.task_title.is_none() {
                    if let Some((title, desc)) = extract_task_from_messages(&self.messages_dir(), &ws.id) {
                        ws.task_title = Some(title);
                        ws.task_description = desc;
                        needs_persist = true;
                    }
                }
                self.workspaces.insert(ws.id.clone(), ws);
            }
            if needs_persist {
                self.save_workspaces()?;
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

        // Load context meta
        let context_meta_path = self.data_dir.join("context_meta.json");
        if context_meta_path.exists() {
            if let Ok(data) = std::fs::read_to_string(&context_meta_path) {
                if let Ok(mut meta) =
                    serde_json::from_str::<HashMap<String, ContextMeta>>(&data)
                {
                    // Reset any stale "building" status on restart
                    for m in meta.values_mut() {
                        if m.build_status == ContextBuildStatus::Building {
                            m.build_status = ContextBuildStatus::Failed;
                        }
                    }
                    self.context_meta.extend(meta);
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

    /// Path where todos are stored (per-repo)
    pub fn todos_dir(&self) -> PathBuf {
        self.data_dir.join("todos")
    }

    /// Path where knowledge base context files are stored (per-repo)
    pub fn context_dir(&self, repo_id: &str) -> PathBuf {
        self.data_dir.join("context").join(repo_id)
    }

    pub fn save_context_meta(&self) -> Result<(), String> {
        let data =
            serde_json::to_string_pretty(&self.context_meta).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join("context_meta.json"), data)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Delete all persisted data for a workspace (messages file, session entry, images).
    /// Call this when permanently removing a workspace.
    pub fn delete_workspace_data(&mut self, workspace_id: &str) {
        let _ = std::fs::remove_file(self.messages_dir().join(format!("{}.json", workspace_id)));
        let _ = std::fs::remove_dir_all(self.data_dir.join("images").join(workspace_id));
        self.session_ids.remove(workspace_id);
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

/// Extract task title/description from the first hidden user message in a workspace's messages file.
fn extract_task_from_messages(messages_dir: &Path, workspace_id: &str) -> Option<(String, Option<String>)> {
    let path = messages_dir.join(format!("{}.json", workspace_id));
    let data = std::fs::read_to_string(&path).ok()?;
    let messages: Vec<serde_json::Value> = serde_json::from_str(&data).ok()?;

    // Find first hidden user message
    let msg = messages.iter().find(|m| {
        m.get("role").and_then(|r| r.as_str()) == Some("user")
            && m.get("hidden").and_then(|h| h.as_bool()) == Some(true)
    })?;

    // Extract text from chunks[0].content
    let chunks = msg.get("chunks")?.as_array()?;
    let content = chunks.first()?.get("content")?.as_str()?;
    if content.is_empty() {
        return None;
    }

    // Split on first double newline: title \n\n description
    if let Some(pos) = content.find("\n\n") {
        let title = content[..pos].to_string();
        let desc = content[pos + 2..].trim().to_string();
        Some((title, if desc.is_empty() { None } else { Some(desc) }))
    } else {
        Some((content.to_string(), None))
    }
}

/// Rename a worktree's branch, detecting the actual current branch from git
/// rather than trusting stored metadata (the agent may have already renamed
/// it via a bash command). No-ops if the branch is already at the target name.
pub fn rename_git_branch(worktree_path: &Path, new_branch: &str, fallback_branch: &str) -> Result<(), String> {
    let current_branch = match std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(worktree_path)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => fallback_branch.to_string(),
    };

    if current_branch == new_branch {
        return Ok(());
    }

    let output = std::process::Command::new("git")
        .args(["branch", "-m", &current_branch, new_branch])
        .current_dir(worktree_path)
        .output()
        .map_err(|e| format!("Failed to run git branch -m: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git branch rename failed: {}", stderr.trim()));
    }

    Ok(())
}

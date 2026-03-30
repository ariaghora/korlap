pub mod github;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

// ── Provider-agnostic types ──────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct ServiceProfile {
    pub login: String,
    pub active: bool,
}

#[derive(Clone, serde::Serialize)]
pub struct CliStatus {
    pub installed: bool,
    pub authenticated: bool,
    pub profiles: Vec<ServiceProfile>,
}

#[derive(Clone, serde::Serialize)]
pub struct RepoEntry {
    pub full_name: String,
    pub description: String,
    pub is_fork: bool,
    pub clone_url: String,
    pub updated_at: String,
}

#[derive(Clone, serde::Deserialize)]
pub struct CreateRepoOptions {
    pub name: String,
    pub private: bool,
    pub description: Option<String>,
    pub add_readme: bool,
}

#[derive(Clone, serde::Serialize)]
pub struct PrStatus {
    pub state: String,      // "none", "open", "merged", "closed"
    pub url: String,
    pub number: i64,
    pub title: String,
    pub checks: String,     // "pending", "passing", "failing", "none"
    pub mergeable: String,  // "mergeable", "conflicting", "unknown"
    pub additions: i64,
    pub deletions: i64,
    pub ahead_by: i64,      // commits ahead of remote (unpushed)
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PrEntry {
    pub number: i64,
    pub title: String,
    pub branch: String,
    pub base_branch: String,
    pub author: String,
    pub url: String,
    pub updated_at: String,
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PrDetail {
    pub number: i64,
    pub title: String,
    pub branch: String,
    pub base_branch: String,
    pub url: String,
    pub body: String,
}

// ── Trait ─────────────────────────────────────────────────────────────

/// Abstraction over a git hosting service (GitHub, GitLab, etc.).
///
/// Implementations encapsulate all provider-specific logic: CLI tool
/// interaction, authentication, URL rewriting, PR management, and
/// repository operations.
pub trait GitServiceProvider: Send + Sync {
    /// Human-readable name (e.g., "GitHub", "GitLab").
    fn name(&self) -> &str;

    /// Domain used to match remote URLs (e.g., "github.com").
    fn domain(&self) -> &str;

    /// Check if this provider handles the given remote URL.
    fn matches_remote_url(&self, url: &str) -> bool;

    // ── Auth ──────────────────────────────────────────────────────

    /// Resolve an API token for the given profile. Returns `None` silently
    /// when no profile is set or the token cannot be obtained.
    fn resolve_token(&self, profile: &Option<String>) -> Option<String>;

    /// List authenticated profiles/accounts.
    fn list_profiles(&self) -> Result<Vec<ServiceProfile>, String>;

    /// Check if the provider CLI is installed and authenticated.
    fn check_cli(&self) -> Result<CliStatus, String>;

    /// Start an interactive authentication flow.
    /// Progress is emitted via `app_handle` events.
    fn auth_login(&self, app_handle: tauri::AppHandle) -> Result<(), String>;

    /// Cancel an in-progress authentication flow.
    fn cancel_auth_login(&self) -> Result<(), String>;

    // ── Git auth ──────────────────────────────────────────────────

    /// Inject authentication config args into a `git` Command.
    /// Called before adding subcommand args (clone, push, fetch, etc.).
    fn inject_git_auth(&self, cmd: &mut Command, token: &Option<String>);

    /// Build a `git` Command with auth and working directory configured.
    fn git_cmd_with_auth(&self, worktree_path: &Path, token: &Option<String>) -> Command;

    /// Build a provider CLI command (e.g., `gh`, `glab`) with token injected.
    fn cli_cmd_with_auth(&self, worktree_path: &Path, token: &Option<String>) -> Command;

    /// Build env vars that should be injected into spawned processes
    /// (agents, scripts) so they can authenticate with both the provider
    /// CLI and git itself.
    fn build_auth_env_vars(&self, token: &Option<String>) -> Vec<(String, String)>;

    // ── Repo operations ──────────────────────────────────────────

    /// Extract "owner/repo" identifier from the repo's remote URL.
    /// Returns `None` if the remote doesn't match this provider.
    fn extract_repo_id(&self, repo_path: &Path) -> Option<String>;

    /// List repositories accessible to the given profile.
    fn list_repos(
        &self,
        profile: &str,
        search: Option<&str>,
        token: &Option<String>,
    ) -> Result<Vec<RepoEntry>, String>;

    /// Create a new repository on the provider. Returns the clone URL.
    fn create_repo(
        &self,
        options: &CreateRepoOptions,
        profile: &str,
        token: &Option<String>,
    ) -> Result<String, String>;

    /// Check which of the given profiles has access to the repo at `path`.
    fn check_repo_access(
        &self,
        path: &Path,
        profiles: &[String],
    ) -> Result<Option<String>, String>;

    // ── PR operations ────────────────────────────────────────────

    /// Get PR status for a branch.
    fn get_pr_status(
        &self,
        worktree_path: &Path,
        branch: &str,
        token: &Option<String>,
    ) -> Result<PrStatus, String>;

    /// Merge a PR.
    fn merge_pr(
        &self,
        worktree_path: &Path,
        pr_number: i64,
        token: &Option<String>,
    ) -> Result<(), String>;

    /// List PRs for a repo.
    fn list_prs(
        &self,
        repo_path: &Path,
        repo_id: &str,
        token: &Option<String>,
    ) -> Result<Vec<PrEntry>, String>;

    /// Get detailed PR info including body text.
    fn get_pr_detail(
        &self,
        repo_path: &Path,
        repo_id: &str,
        pr_number: i64,
        token: &Option<String>,
    ) -> Result<PrDetail, String>;
}

// ── Registry ─────────────────────────────────────────────────────────

/// Holds all registered git service provider implementations.
/// Immutable after creation; interior mutability (e.g., AtomicU32 for
/// auth PID tracking) is handled by individual providers.
pub struct GitProviderRegistry {
    github: github::GitHubProvider,
    // Future: gitlab, bitbucket, etc.
}

impl GitProviderRegistry {
    pub fn new() -> Self {
        Self {
            github: github::GitHubProvider::new(),
        }
    }

    /// Get the provider for a repo by inspecting its remote URL.
    /// Falls back to GitHub if the remote can't be read or doesn't match
    /// any known provider.
    pub fn for_repo(&self, repo_path: &Path) -> &dyn GitServiceProvider {
        let url = read_remote_url(repo_path).unwrap_or_default();
        if self.github.matches_remote_url(&url) {
            return &self.github;
        }
        // Future: check other providers here.
        // Default fallback — most repos are on GitHub.
        &self.github
    }

    /// Direct access to the GitHub provider (for commands that are
    /// explicitly GitHub-specific, e.g., `gh auth login`).
    pub fn github(&self) -> &dyn GitServiceProvider {
        &self.github
    }
}

/// Read the origin remote URL from a repo. Returns None on failure.
fn read_remote_url(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

// Re-export the Arc wrapper type that commands should use.
pub type SharedProviderRegistry = Arc<GitProviderRegistry>;

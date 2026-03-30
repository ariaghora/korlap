use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::commands::helpers::{inject_shell_env, strip_ansi};

use super::{
    CliStatus, CreateRepoOptions, GitServiceProvider, PrDetail, PrEntry, PrStatus, RepoEntry,
    ServiceProfile,
};

/// PID of the in-flight `gh auth login` process, or 0 if none.
/// Stored inside the provider so each provider manages its own auth flow.
pub struct GitHubProvider {
    auth_pid: AtomicU32,
}

impl GitHubProvider {
    pub fn new() -> Self {
        Self {
            auth_pid: AtomicU32::new(0),
        }
    }
}

// SAFETY: AtomicU32 is Send + Sync, so the whole struct is too.
// The trait requires Send + Sync.

impl GitServiceProvider for GitHubProvider {
    fn name(&self) -> &str {
        "GitHub"
    }

    fn domain(&self) -> &str {
        "github.com"
    }

    fn matches_remote_url(&self, url: &str) -> bool {
        url.contains("github.com")
    }

    // ── Auth ──────────────────────────────────────────────────────

    fn resolve_token(&self, profile: &Option<String>) -> Option<String> {
        let profile = profile.as_ref()?;
        let mut cmd = Command::new("gh");
        cmd.args(["auth", "token", "--user", profile]);
        inject_shell_env(&mut cmd);
        cmd.output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    }

    fn list_profiles(&self) -> Result<Vec<ServiceProfile>, String> {
        let mut cmd = Command::new("gh");
        cmd.args(["auth", "status", "--json", "hosts"]);
        inject_shell_env(&mut cmd);
        let output = cmd
            .output()
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
                            let active = account
                                .get("active")
                                .and_then(|a| a.as_bool())
                                .unwrap_or(false);
                            profiles.push(ServiceProfile {
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

    fn check_cli(&self) -> Result<CliStatus, String> {
        let mut cmd = Command::new("gh");
        cmd.arg("--version");
        inject_shell_env(&mut cmd);
        let version_result = cmd.output();

        let installed = match version_result {
            Ok(ref o) => o.status.success(),
            Err(_) => false,
        };

        if !installed {
            return Ok(CliStatus {
                installed: false,
                authenticated: false,
                profiles: vec![],
            });
        }

        let profiles = self.list_profiles()?;
        let authenticated = !profiles.is_empty();

        Ok(CliStatus {
            installed,
            authenticated,
            profiles,
        })
    }

    fn auth_login(&self, app_handle: tauri::AppHandle) -> Result<(), String> {
        use std::io::BufRead;
        use tauri::Emitter;

        let mut cmd = Command::new("gh");
        cmd.args([
            "auth",
            "login",
            "--hostname",
            "github.com",
            "--git-protocol",
            "https",
            "--web",
            "--scopes",
            "workflow",
        ]);
        inject_shell_env(&mut cmd);
        cmd.stdin(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to start gh auth login: {}", e))?;
        self.auth_pid.store(child.id(), Ordering::SeqCst);

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
                if let Some(url) = line
                    .split_whitespace()
                    .find(|w| w.starts_with("https://github.com/login"))
                {
                    let _ = Command::new("open").arg(url).spawn();
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

        let status = child
            .wait()
            .map_err(|e| format!("gh auth login failed: {}", e))?;
        self.auth_pid.store(0, Ordering::SeqCst);
        if !status.success() {
            return Err("GitHub authentication was cancelled or failed.".to_string());
        }
        Ok(())
    }

    fn cancel_auth_login(&self) -> Result<(), String> {
        let pid = self.auth_pid.swap(0, Ordering::SeqCst);
        if pid != 0 {
            let _ = Command::new("kill").arg(pid.to_string()).output();
        }
        Ok(())
    }

    // ── Git auth ──────────────────────────────────────────────────

    fn inject_git_auth(&self, cmd: &mut Command, token: &Option<String>) {
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
    }

    fn git_cmd_with_auth(&self, worktree_path: &Path, token: &Option<String>) -> Command {
        let mut cmd = Command::new("git");
        self.inject_git_auth(&mut cmd, token);
        cmd.current_dir(worktree_path);
        inject_shell_env(&mut cmd);
        cmd
    }

    fn cli_cmd_with_auth(&self, worktree_path: &Path, token: &Option<String>) -> Command {
        let mut cmd = Command::new("gh");
        cmd.current_dir(worktree_path);
        inject_shell_env(&mut cmd);
        if let Some(ref t) = token {
            cmd.env("GH_TOKEN", t);
        }
        cmd
    }

    fn build_auth_env_vars(&self, token: &Option<String>) -> Vec<(String, String)> {
        let mut vars = Vec::new();
        if let Some(ref t) = token {
            vars.push(("GH_TOKEN".to_string(), t.clone()));
            vars.push((
                "GIT_CONFIG_PARAMETERS".to_string(),
                format!(
                    "'url.https://x-access-token:{}@github.com/.insteadOf=git@github.com:'",
                    t
                ),
            ));
        }
        vars
    }

    // ── Repo operations ──────────────────────────────────────────

    fn extract_repo_id(&self, repo_path: &Path) -> Option<String> {
        let mut cmd = Command::new("git");
        cmd.args(["remote", "get-url", "origin"]);
        cmd.current_dir(repo_path);
        inject_shell_env(&mut cmd);

        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }

        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Match patterns:
        //   git@github.com:owner/repo.git
        //   https://github.com/owner/repo.git
        //   ssh://git@github.com/owner/repo.git
        let path_part = if let Some(rest) = url.strip_prefix("git@github.com:") {
            Some(rest)
        } else {
            url.split("github.com/").nth(1)
        };

        path_part.map(|p| p.trim_end_matches(".git").to_string())
    }

    fn list_repos(
        &self,
        profile: &str,
        search: Option<&str>,
        token: &Option<String>,
    ) -> Result<Vec<RepoEntry>, String> {
        let mut cmd = Command::new("gh");
        cmd.args([
            "repo",
            "list",
            profile,
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

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gh: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh repo list failed: {}", stderr));
        }

        let arr: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("Failed to parse gh output: {}", e))?;

        let search_lower = search.unwrap_or("").to_lowercase();

        let mut repos: Vec<RepoEntry> = arr
            .into_iter()
            .filter_map(|v| {
                let full_name = v.get("nameWithOwner")?.as_str()?.to_string();
                let description = v
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_fork = v.get("isFork").and_then(|f| f.as_bool()).unwrap_or(false);
                let clone_url = v
                    .get("sshUrl")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
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

                Some(RepoEntry {
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
    }

    fn create_repo(
        &self,
        options: &CreateRepoOptions,
        profile: &str,
        token: &Option<String>,
    ) -> Result<String, String> {
        let full_name = format!("{}/{}", profile, options.name);

        let mut cmd = Command::new("gh");
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

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gh: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(format!("Failed to create repository: {}", stderr));
        }

        Ok(format!("git@github.com:{}.git", full_name))
    }

    fn check_repo_access(
        &self,
        path: &Path,
        profiles: &[String],
    ) -> Result<Option<String>, String> {
        let nwo = match self.extract_repo_id(path) {
            Some(nwo) => nwo,
            None => return Ok(None),
        };

        for profile in profiles {
            let token = self.resolve_token(&Some(profile.clone()));
            let mut cmd = Command::new("gh");
            cmd.args(["repo", "view", &nwo, "--json", "name"]);
            inject_shell_env(&mut cmd);
            if let Some(ref t) = token {
                cmd.env("GH_TOKEN", t);
            }
            let output = cmd
                .output()
                .map_err(|e| format!("Failed to run gh: {}", e))?;
            if output.status.success() {
                return Ok(Some(profile.clone()));
            }
        }

        Ok(None)
    }

    // ── PR operations ────────────────────────────────────────────

    fn get_pr_status(
        &self,
        worktree_path: &Path,
        branch: &str,
        token: &Option<String>,
    ) -> Result<PrStatus, String> {
        let mut gh_cmd = self.cli_cmd_with_auth(worktree_path, token);
        gh_cmd.args([
            "pr",
            "view",
            branch,
            "--json",
            "state,url,number,title,statusCheckRollup,mergeable,additions,deletions",
        ]);
        let output = gh_cmd
            .output()
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

        let pr_state = v
            .get("state")
            .and_then(|s| s.as_str())
            .unwrap_or("OPEN")
            .to_lowercase();
        let url = v
            .get("url")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let number = v.get("number").and_then(|n| n.as_i64()).unwrap_or(0);
        let title = v
            .get("title")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let additions = v.get("additions").and_then(|n| n.as_i64()).unwrap_or(0);
        let deletions = v.get("deletions").and_then(|n| n.as_i64()).unwrap_or(0);
        let mergeable = v
            .get("mergeable")
            .and_then(|s| s.as_str())
            .unwrap_or("UNKNOWN")
            .to_lowercase();

        let checks =
            if let Some(checks_arr) = v.get("statusCheckRollup").and_then(|c| c.as_array()) {
                if checks_arr.is_empty() {
                    "none".to_string()
                } else {
                    let any_failing = checks_arr.iter().any(|c| {
                        let conclusion =
                            c.get("conclusion").and_then(|s| s.as_str()).unwrap_or("");
                        conclusion == "FAILURE"
                            || conclusion == "ERROR"
                            || conclusion == "TIMED_OUT"
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
            let rev_output = Command::new("git")
                .args([
                    "rev-list",
                    "--count",
                    &format!("origin/{}..{}", branch, branch),
                ])
                .current_dir(worktree_path)
                .output();
            match rev_output {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<i64>()
                    .unwrap_or(0),
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
    }

    fn merge_pr(
        &self,
        worktree_path: &Path,
        pr_number: i64,
        token: &Option<String>,
    ) -> Result<(), String> {
        let mut cmd = self.cli_cmd_with_auth(worktree_path, token);
        cmd.args([
            "pr",
            "merge",
            &pr_number.to_string(),
            "--squash",
            "--delete-branch=false",
        ]);

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gh pr merge: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh pr merge failed: {}", stderr.trim()));
        }
        Ok(())
    }

    fn list_prs(
        &self,
        _repo_path: &Path,
        repo_id: &str,
        token: &Option<String>,
    ) -> Result<Vec<PrEntry>, String> {
        let mut cmd = Command::new("gh");
        cmd.args([
            "pr",
            "list",
            "--repo",
            repo_id,
            "--json",
            "number,title,headRefName,baseRefName,author,url,updatedAt,additions,deletions",
            "--limit",
            "50",
        ]);
        inject_shell_env(&mut cmd);
        if let Some(ref t) = token {
            cmd.env("GH_TOKEN", t);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gh pr list: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh pr list failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let raw: Vec<serde_json::Value> = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse gh pr list output: {}", e))?;

        let entries = raw
            .iter()
            .map(|pr| PrEntry {
                number: pr["number"].as_i64().unwrap_or(0),
                title: pr["title"].as_str().unwrap_or("").to_string(),
                branch: pr["headRefName"].as_str().unwrap_or("").to_string(),
                base_branch: pr["baseRefName"].as_str().unwrap_or("").to_string(),
                author: pr["author"]["login"].as_str().unwrap_or("").to_string(),
                url: pr["url"].as_str().unwrap_or("").to_string(),
                updated_at: pr["updatedAt"].as_str().unwrap_or("").to_string(),
                additions: pr["additions"].as_i64().unwrap_or(0),
                deletions: pr["deletions"].as_i64().unwrap_or(0),
            })
            .collect();

        Ok(entries)
    }

    fn get_pr_detail(
        &self,
        _repo_path: &Path,
        repo_id: &str,
        pr_number: i64,
        token: &Option<String>,
    ) -> Result<PrDetail, String> {
        let mut cmd = Command::new("gh");
        cmd.args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            repo_id,
            "--json",
            "number,title,headRefName,baseRefName,url,body",
        ]);
        inject_shell_env(&mut cmd);
        if let Some(ref t) = token {
            cmd.env("GH_TOKEN", t);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gh pr view: {}", e))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh pr view failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let pr: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| format!("Failed to parse gh pr view output: {}", e))?;

        Ok(PrDetail {
            number: pr["number"].as_i64().unwrap_or(0),
            title: pr["title"].as_str().unwrap_or("").to_string(),
            branch: pr["headRefName"].as_str().unwrap_or("").to_string(),
            base_branch: pr["baseRefName"].as_str().unwrap_or("").to_string(),
            url: pr["url"].as_str().unwrap_or("").to_string(),
            body: pr["body"].as_str().unwrap_or("").to_string(),
        })
    }
}

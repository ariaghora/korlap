use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Git helpers ──────────────────────────────────────────────────────

pub fn detect_default_branch(repo_path: &Path) -> Result<String, String> {
    // Tier 1: origin HEAD symref (most reliable)
    let output = std::process::Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(repo_path)
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        let refname = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(branch) = refname.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    // Tier 2: check which of origin/main, origin/master exists as REMOTE tracking refs.
    // Never fall back to local branches — workspaces must always branch from origin.
    for candidate in ["main", "master"] {
        let ref_name = format!("refs/remotes/origin/{}", candidate);
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--verify", &ref_name])
            .current_dir(repo_path)
            .output()
            .map_err(|e| format!("Failed to run git: {}", e))?;
        if output.status.success() {
            return Ok(candidate.to_string());
        }
    }

    // No silent fallback — error out with actionable message.
    Err(
        "Could not detect default branch from remote. \
         No origin/HEAD, origin/main, or origin/master found. \
         Run `git remote set-head origin --auto` or check your remote configuration."
            .to_string(),
    )
}

pub fn repo_display_name(repo_path: &Path) -> String {
    repo_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| repo_path.display().to_string())
}

/// Cached shell env values (resolved once on first call).
pub fn get_shell_env() -> &'static ShellEnv {
    use std::sync::OnceLock;
    static ENV: OnceLock<ShellEnv> = OnceLock::new();
    ENV.get_or_init(|| {
        let ssh_auth_sock = std::env::var("SSH_AUTH_SOCK").ok().or_else(|| {
            std::process::Command::new("launchctl")
                .args(["getenv", "SSH_AUTH_SOCK"])
                .output()
                .ok()
                .and_then(|o| {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() { None } else { Some(s) }
                })
        });

        let home = std::env::var("HOME").ok();

        // Use interactive login shell (-lic) so .zshrc is sourced — this is
        // where nvm/fnm/volta add their PATH entries.  Delimiters protect
        // against noisy .zshrc output (motd, nvm "now using", etc.).
        let delimiter = "__KORLAP_ENV__";
        let path = std::process::Command::new("zsh")
            .args([
                "-lic",
                &format!("echo {delimiter}; echo $PATH; echo {delimiter}"),
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next(); // noise before first delimiter
                let value = parts.next()?;  // the actual PATH
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            });

        // Resolve absolute path to `claude` binary once, so we don't rely
        // on PATH lookup at every spawn (which can fail in sandboxed contexts).
        let claude_path = std::process::Command::new("zsh")
            .args(["-lic", &format!("echo {delimiter}; whence -p claude; echo {delimiter}")])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next();
                let value = parts.next()?;
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() || trimmed.contains("not found") {
                    None
                } else {
                    Some(trimmed)
                }
            });

        if claude_path.is_none() {
            tracing::warn!("Could not resolve `claude` binary path — agent spawn will likely fail");
        }

        // Resolve codex binary path (optional — only needed if user selects Codex provider)
        let codex_path = std::process::Command::new("zsh")
            .args(["-lic", &format!("echo {delimiter}; whence -p codex; echo {delimiter}")])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next();
                let value = parts.next()?;
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() || trimmed.contains("not found") {
                    None
                } else {
                    Some(trimmed)
                }
            });

        if codex_path.is_some() {
            tracing::info!("Resolved codex binary: {:?}", codex_path);
        }

        // Capture full environment from interactive login shell so spawned
        // processes get all user env vars (CARGO_TARGET_DIR, GOPATH, etc.)
        // that a Tauri app launched from Finder/Dock would otherwise miss.
        let all_vars: HashMap<String, String> = std::process::Command::new("zsh")
            .args([
                "-lic",
                &format!("echo {delimiter}; /usr/bin/env; echo {delimiter}"),
            ])
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                let mut parts = stdout.split(delimiter);
                let _before = parts.next();
                let env_section = parts.next()?;
                let mut vars = HashMap::new();
                let mut current_key = String::new();
                let mut current_val = String::new();
                for line in env_section.lines() {
                    if let Some(eq_pos) = line.find('=') {
                        let key = &line[..eq_pos];
                        // Valid env var names: alphanumeric + underscore, non-empty
                        if !key.is_empty()
                            && key
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
                        {
                            // Flush previous entry
                            if !current_key.is_empty() {
                                vars.insert(
                                    std::mem::take(&mut current_key),
                                    std::mem::take(&mut current_val),
                                );
                            }
                            current_key = key.to_string();
                            current_val = line[eq_pos + 1..].to_string();
                            continue;
                        }
                    }
                    // Continuation of a multi-line value
                    if !current_key.is_empty() {
                        current_val.push('\n');
                        current_val.push_str(line);
                    }
                }
                // Flush last entry
                if !current_key.is_empty() {
                    vars.insert(current_key, current_val);
                }
                Some(vars)
            })
            .unwrap_or_default();

        tracing::info!(
            "Captured {} env vars from login shell",
            all_vars.len()
        );

        ShellEnv { ssh_auth_sock, home, path, claude_path, codex_path, all_vars }
    })
}

pub struct ShellEnv {
    pub ssh_auth_sock: Option<String>,
    pub home: Option<String>,
    pub path: Option<String>,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
    /// Full environment captured from an interactive login shell.
    /// Contains all user env vars (CARGO_TARGET_DIR, GOPATH, etc.)
    /// that a Tauri app launched from Finder/Dock would otherwise miss.
    pub all_vars: HashMap<String, String>,
}

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip until we hit a letter (the terminator of an ANSI sequence)
            for c2 in chars.by_ref() {
                if c2.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Inject the full user shell environment into a Command so that processes
/// spawned from a Finder/Dock-launched Tauri app behave like they were
/// started from a terminal (includes CARGO_TARGET_DIR, GOPATH, etc.).
pub fn inject_shell_env(cmd: &mut std::process::Command) {
    let env = get_shell_env();

    // Apply all env vars captured from the interactive login shell.
    cmd.envs(&env.all_vars);

    // Fallback: SSH_AUTH_SOCK from launchctl if not present in shell env
    // (some setups only expose it via launchd, not the shell profile).
    if !env.all_vars.contains_key("SSH_AUTH_SOCK") {
        if let Some(ref sock) = env.ssh_auth_sock {
            cmd.env("SSH_AUTH_SOCK", sock);
        }
    }
}

/// Resolve the GH token for a given profile via `gh auth token`.
/// Returns None if no profile is set or the token cannot be obtained.
pub fn resolve_gh_token(profile: &Option<String>) -> Option<String> {
    let profile = profile.as_ref()?;
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token", "--user", profile]);
    inject_shell_env(&mut cmd);
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Build a `git` Command with HTTPS URL rewriting for GH token auth.
pub fn git_cmd_with_auth(
    worktree_path: &std::path::Path,
    gh_token: &Option<String>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    if let Some(ref token) = gh_token {
        cmd.args([
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=git@github.com:",
                token
            ),
            "-c",
            &format!(
                "url.https://x-access-token:{}@github.com/.insteadOf=ssh://git@github.com/",
                token
            ),
        ]);
    }
    cmd.current_dir(worktree_path);
    inject_shell_env(&mut cmd);
    cmd
}

/// Build a `gh` Command with GH_TOKEN env injected.
pub fn gh_cmd_with_auth(
    worktree_path: &std::path::Path,
    gh_token: &Option<String>,
) -> std::process::Command {
    let mut cmd = std::process::Command::new("gh");
    cmd.current_dir(worktree_path);
    inject_shell_env(&mut cmd);
    if let Some(ref token) = gh_token {
        cmd.env("GH_TOKEN", token);
    }
    cmd
}

/// Extract "owner/repo" from a repo's remote origin URL.
/// Returns None if not a GitHub repo or if the remote can't be read.
pub fn extract_gh_nwo(path: &std::path::Path) -> Option<String> {
    let mut cmd = std::process::Command::new("git");
    cmd.args(["remote", "get-url", "origin"]);
    cmd.current_dir(path);
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

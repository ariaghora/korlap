use super::types::LspServerConfig;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Scan a repo root and return which server configs have matching detect_files.
/// Checks both the root AND one level of subdirectories (for monorepos
/// like Tauri projects where Cargo.toml lives in src-tauri/).
/// Returns (server_id, config, project_root) where project_root is the
/// directory containing the detect_file (may differ from repo_root).
pub fn detect_servers<'a>(
    repo_root: &Path,
    configs: &'a HashMap<String, LspServerConfig>,
) -> Vec<(&'a str, &'a LspServerConfig, PathBuf)> {
    configs
        .iter()
        .filter_map(|(id, cfg)| {
            // If project_roots is set, only check those directories
            if !cfg.project_roots.is_empty() {
                for root in &cfg.project_roots {
                    let dir = repo_root.join(root);
                    for f in &cfg.detect_files {
                        if dir.join(f).exists() {
                            return Some((id.as_str(), cfg, dir));
                        }
                    }
                }
                return None;
            }

            // Auto-detect: check root first
            for f in &cfg.detect_files {
                if repo_root.join(f).exists() {
                    return Some((id.as_str(), cfg, repo_root.to_path_buf()));
                }
            }
            // Then one level of subdirectories
            if let Ok(entries) = std::fs::read_dir(repo_root) {
                for entry in entries.flatten() {
                    let dir = entry.path();
                    if !dir.is_dir() {
                        continue;
                    }
                    for f in &cfg.detect_files {
                        if dir.join(f).exists() {
                            return Some((id.as_str(), cfg, dir));
                        }
                    }
                }
            }
            None
        })
        .collect()
}

/// Check whether the LSP binary for a config exists on PATH.
/// Uses the shell env from helpers.rs so we see the same PATH as spawned processes.
pub fn validate_binary(config: &LspServerConfig) -> Result<PathBuf, String> {
    let env = crate::commands::helpers::get_shell_env();
    let binary = &config.command;

    if let Some(ref path_str) = env.path {
        for dir in path_str.split(':') {
            let candidate = Path::new(dir).join(binary);
            if candidate.exists() && candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    let hint = if config.install_hint.is_empty() {
        String::new()
    } else {
        format!(". Install with: {}", config.install_hint)
    };

    Err(format!("{} not found on PATH{}", binary, hint))
}

use crate::state::AppState;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::sync::{Arc, Mutex};
use tauri::State;

// ── File browser commands ────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,      // relative to worktree root
    pub is_dir: bool,
    pub size: u64,
}

#[tauri::command]
pub async fn list_directory(
    workspace_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileEntry>, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let target = if relative_path.is_empty() {
        worktree_path.clone()
    } else {
        worktree_path.join(&relative_path)
    };

    // Security: ensure path doesn't escape worktree
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    let wt = worktree_canonical.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut entries = Vec::new();
        let dir = std::fs::read_dir(&canonical)
            .map_err(|e| format!("Cannot read directory: {}", e))?;

        for entry in dir {
            let entry = entry.map_err(|e| format!("Error reading entry: {}", e))?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files/dirs (except common ones like .github)
            if name.starts_with('.') && name != ".github" && name != ".gitignore" {
                continue;
            }

            let metadata = entry
                .metadata()
                .map_err(|e| format!("Cannot stat {}: {}", name, e))?;

            let full_path = entry.path();
            let rel = full_path
                .strip_prefix(&wt)
                .unwrap_or(&full_path)
                .to_string_lossy()
                .to_string();

            entries.push(FileEntry {
                name,
                path: rel,
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            });
        }

        // Sort: directories first, then alphabetical
        entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        Ok(entries)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn read_file(
    workspace_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let target = worktree_path.join(&relative_path);
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| format!("Cannot stat file: {}", e))?;

        // Limit to 2MB to avoid UI freezes
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(format!(
                "File too large ({} bytes). Max 2MB for preview.",
                metadata.len()
            ));
        }

        std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Cannot read file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn read_repo_file(
    repo_id: String,
    relative_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    let target = repo_path.join(&relative_path);
    let canonical = target
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let repo_canonical = repo_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve repo: {}", e))?;
    if !canonical.starts_with(&repo_canonical) {
        return Err("Path escapes repo boundary".to_string());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let metadata = std::fs::metadata(&canonical)
            .map_err(|e| format!("Cannot stat file: {}", e))?;

        // Limit to 2MB to avoid UI freezes
        if metadata.len() > 2 * 1024 * 1024 {
            return Err(format!(
                "File too large ({} bytes). Max 2MB for preview.",
                metadata.len()
            ));
        }

        std::fs::read_to_string(&canonical)
            .map_err(|e| format!("Cannot read file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn write_file(
    workspace_id: String,
    relative_path: String,
    content: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let target = worktree_path.join(&relative_path);
    let canonical_parent = target
        .parent()
        .ok_or("Invalid file path")?
        .canonicalize()
        .map_err(|e| format!("Cannot resolve parent: {}", e))?;
    let worktree_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical_parent.starts_with(&worktree_canonical) {
        return Err("Path escapes worktree boundary".to_string());
    }

    let write_target = canonical_parent.join(
        target
            .file_name()
            .ok_or("Invalid file name")?,
    );

    tauri::async_runtime::spawn_blocking(move || {
        std::fs::write(&write_target, content)
            .map_err(|e| format!("Cannot write file: {}", e))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── File search commands ─────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct FileSearchResult {
    /// Relative path from worktree root (e.g. "src/lib/ipc.ts")
    pub path: String,
    /// Just the filename (e.g. "ipc.ts")
    pub name: String,
    /// "file" or "folder"
    pub kind: String,
    /// Fuzzy match score (higher = better)
    pub score: i64,
}

#[tauri::command]
pub fn search_workspace_files(
    workspace_id: String,
    query: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileSearchResult>, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    search_files_in_dir(&worktree_path, &query)
}

#[tauri::command]
pub fn search_repo_files(
    repo_id: String,
    query: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<Vec<FileSearchResult>, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    search_files_in_dir(&repo_path, &query)
}

fn search_files_in_dir(
    root: &std::path::Path,
    query: &str,
) -> Result<Vec<FileSearchResult>, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(vec![]);
    }

    let matcher = SkimMatcherV2::default();
    let mut results: Vec<FileSearchResult> = Vec::new();
    let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .max_depth(Some(12))
        .build();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let rel_path = match path.strip_prefix(root) {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip the root itself and .git
        let rel_str = rel_path.to_string_lossy();
        if rel_str.is_empty() || rel_str.starts_with(".git") {
            continue;
        }

        let is_dir = entry.file_type().map_or(false, |ft| ft.is_dir());
        let name = rel_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Match against filename and full path — take the better score
        let score_name = matcher.fuzzy_match(&name, &query).unwrap_or(0);
        let score_path = matcher.fuzzy_match(&rel_str, &query).unwrap_or(0);
        let score = score_name.max(score_path);

        if score > 0 {
            if is_dir {
                let dir_key = rel_str.to_string();
                if !seen_dirs.insert(dir_key.clone()) {
                    continue;
                }
                results.push(FileSearchResult {
                    path: format!("{}/", rel_str),
                    name: format!("{}/", name),
                    kind: "folder".to_string(),
                    score,
                });
            } else {
                results.push(FileSearchResult {
                    path: rel_str.to_string(),
                    name,
                    kind: "file".to_string(),
                    score,
                });
            }
        }
    }

    // Sort by score descending, limit to top 20
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(20);

    Ok(results)
}

// ── Grep (content search) ────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct GrepMatch {
    pub path: String,
    pub line_number: u32,
    pub column: u32,
    pub line_content: String,
}

#[derive(Clone, serde::Serialize)]
pub struct GrepResult {
    pub matches: Vec<GrepMatch>,
    pub truncated: bool,
}

#[tauri::command]
pub async fn grep_workspace(
    workspace_id: String,
    pattern: String,
    is_regex: bool,
    case_sensitive: bool,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<GrepResult, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let pattern_trimmed = pattern.trim().to_string();
    if pattern_trimmed.is_empty() {
        return Ok(GrepResult {
            matches: vec![],
            truncated: false,
        });
    }

    let wt = worktree_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        grep_in_dir(&wt, &pattern_trimmed, is_regex, case_sensitive)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn grep_repo(
    repo_id: String,
    pattern: String,
    is_regex: bool,
    case_sensitive: bool,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<GrepResult, String> {
    let repo_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st
            .repos
            .get(&repo_id)
            .ok_or("Repository not found")?;
        repo.path.clone()
    };

    let pattern_trimmed = pattern.trim().to_string();
    if pattern_trimmed.is_empty() {
        return Ok(GrepResult {
            matches: vec![],
            truncated: false,
        });
    }

    let wt = repo_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        grep_in_dir(&wt, &pattern_trimmed, is_regex, case_sensitive)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn grep_in_dir(
    root: &std::path::Path,
    pattern: &str,
    is_regex: bool,
    case_sensitive: bool,
) -> Result<GrepResult, String> {
    use grep_matcher::Matcher;
    use grep_regex::RegexMatcherBuilder;
    use grep_searcher::sinks::UTF8;
    use grep_searcher::SearcherBuilder;
    use ignore::WalkBuilder;

    let escaped_pattern = if is_regex {
        pattern.to_string()
    } else {
        regex::escape(pattern)
    };

    let mut builder = RegexMatcherBuilder::new();
    if case_sensitive {
        // Strict case: defaults are fine
    } else {
        // Smart case: case-insensitive unless pattern has uppercase
        builder.case_smart(true);
    }
    let matcher = builder
        .build(&escaped_pattern)
        .map_err(|e| format!("Invalid pattern: {}", e))?;

    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .build();

    let max_results: usize = 100;
    let max_matches_per_file: usize = 5;
    let max_line_len: usize = 500;
    let max_filesize: u64 = 1_048_576; // 1MB

    let mut results: Vec<GrepMatch> = Vec::new();
    let mut truncated = false;
    let root_prefix = root.to_string_lossy().to_string();

    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .max_filesize(Some(max_filesize))
        .build();

    'outer: for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // Skip directories and symlinks
        let ft = match entry.file_type() {
            Some(ft) => ft,
            None => continue,
        };
        if !ft.is_file() {
            continue;
        }

        let path = entry.path().to_path_buf();

        let mut file_match_count = 0usize;

        let search_result = searcher.search_path(
            &matcher,
            &path,
            UTF8(|line_number, line_content| {
                if results.len() >= max_results {
                    truncated = true;
                    return Ok(false);
                }
                if file_match_count >= max_matches_per_file {
                    return Ok(false);
                }
                file_match_count += 1;

                let raw_path = path.to_string_lossy();
                let rel_path = raw_path
                    .strip_prefix(&root_prefix)
                    .unwrap_or(&raw_path)
                    .trim_start_matches('/');

                let column = matcher
                    .find(line_content.as_bytes())
                    .ok()
                    .flatten()
                    .map(|m| m.start() as u32)
                    .unwrap_or(0);

                let mut content = line_content.trim_end().to_string();
                if content.len() > max_line_len {
                    content.truncate(max_line_len);
                    content.push_str("…");
                }

                results.push(GrepMatch {
                    path: rel_path.to_string(),
                    line_number: line_number as u32,
                    column,
                    line_content: content,
                });

                Ok(true)
            }),
        );

        // Silently skip files that can't be searched
        if let Err(_) = search_result {
            continue;
        }

        if truncated {
            break 'outer;
        }
    }

    Ok(GrepResult {
        matches: results,
        truncated,
    })
}

#[tauri::command]
pub fn read_workspace_file(
    workspace_id: String,
    file_path: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let worktree_path = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        ws.worktree_path.clone()
    };

    let full_path = worktree_path.join(&file_path);

    // Security: ensure the path stays within the worktree
    let canonical = full_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let wt_canonical = worktree_path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve worktree: {}", e))?;
    if !canonical.starts_with(&wt_canonical) {
        return Err("Path escapes worktree boundary".into());
    }

    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    const MAX_LINES: usize = 100;
    let total_lines = content.lines().count();
    let total_bytes = content.len();

    if total_lines > MAX_LINES {
        let truncated: String = content.lines().take(MAX_LINES).collect::<Vec<_>>().join("\n");
        Ok(format!(
            "{}\n\n... truncated ({} of {} lines shown, {} bytes total)",
            truncated, MAX_LINES, total_lines, total_bytes
        ))
    } else {
        Ok(content)
    }
}

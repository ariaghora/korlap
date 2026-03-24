use crate::state::{AgentHandle, AppState, ContextBuildStatus, ContextMeta};
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use super::agent::{AgentEvent, ToolUseInfo};
use super::helpers::{get_shell_env, inject_shell_env, now_unix, resolve_gh_token};

/// Get the current HEAD commit SHA of a git repo.
fn git_head_commit(repo_path: &str) -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Parse file affinity from index.md to find entry IDs matching changed files.
/// Each `changed_file` is checked via prefix match against glob bases.
pub fn find_entries_by_file_affinity(
    index_content: &str,
    changed_files: &[&str],
) -> Vec<String> {
    let mut ids = Vec::new();
    let mut in_affinity = false;
    for line in index_content.lines() {
        if line.contains("## File affinity") {
            in_affinity = true;
            continue;
        }
        if in_affinity {
            if line.starts_with("## ") {
                break;
            }
            // Format: "src/auth/*       → auth-a3f8c2"
            if let Some((glob_part, entry_id)) = line.split_once('→') {
                let glob_base = glob_part.trim().trim_end_matches('*').trim_end_matches('/');
                let entry_id = entry_id.trim().to_string();
                if glob_base.is_empty() {
                    continue;
                }
                for file in changed_files {
                    if file.starts_with(glob_base) && !ids.contains(&entry_id) {
                        ids.push(entry_id.clone());
                        break;
                    }
                }
            }
        }
    }
    ids
}

/// Extract context.md entries whose `## ` header contains one of the given IDs.
pub fn extract_entries_by_id(context_md: &str, ids: &[String]) -> String {
    let mut result = String::new();
    let mut capturing = false;
    for line in context_md.lines() {
        if line.starts_with("## ") {
            capturing = ids.iter().any(|id| line.contains(id.as_str()));
        }
        if capturing {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

/// Generate an ISO 8601 timestamp string from the system clock via `date -u`.
fn iso_timestamp() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| format!("{}", now_unix()))
}

// ── Hot context (no LLM) ─────────────────────────────────────────────

#[tauri::command]
pub async fn regenerate_hot(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let (repo_path, gh_profile, context_dir) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        (
            repo.path.clone(),
            repo.gh_profile.clone(),
            st.context_dir(&repo_id),
        )
    };

    tauri::async_runtime::spawn_blocking(move || {
        let _ = std::fs::create_dir_all(&context_dir);

        let timestamp = iso_timestamp();
        let mut sections = Vec::new();
        sections.push(format!(
            "# Current state\n_Generated: {}_",
            timestamp
        ));

        // git log
        match std::process::Command::new("git")
            .args(["log", "--oneline", "-20"])
            .current_dir(&repo_path)
            .output()
        {
            Ok(o) if o.status.success() => {
                let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !out.is_empty() {
                    sections.push(format!("## Recent commits\n```\n{}\n```", out));
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                tracing::warn!("git log failed for hot.md: {}", err);
            }
            Err(e) => tracing::warn!("git log failed for hot.md: {}", e),
        }

        // git branch -r
        match std::process::Command::new("git")
            .args([
                "branch",
                "-r",
                "--sort=-committerdate",
                "--format=%(refname:short)",
            ])
            .current_dir(&repo_path)
            .output()
        {
            Ok(o) if o.status.success() => {
                let out = String::from_utf8_lossy(&o.stdout);
                let lines: Vec<&str> = out.lines().take(20).collect();
                if !lines.is_empty() {
                    sections.push(format!(
                        "## Remote branches\n```\n{}\n```",
                        lines.join("\n")
                    ));
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                tracing::warn!("git branch -r failed for hot.md: {}", err);
            }
            Err(e) => tracing::warn!("git branch -r failed for hot.md: {}", e),
        }

        // gh pr list
        let gh_token = resolve_gh_token(&gh_profile);
        let mut gh_cmd = std::process::Command::new("gh");
        gh_cmd.args([
            "pr",
            "list",
            "--json",
            "number,title,headRefName,state",
            "--limit",
            "10",
        ]);
        gh_cmd.current_dir(&repo_path);
        inject_shell_env(&mut gh_cmd);
        if let Some(ref token) = gh_token {
            gh_cmd.env("GH_TOKEN", token);
        }
        match gh_cmd.output() {
            Ok(o) if o.status.success() => {
                let out = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !out.is_empty() && out != "[]" {
                    sections.push(format!("## Open PRs\n```json\n{}\n```", out));
                }
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                tracing::warn!("gh pr list failed for hot.md: {}", err);
            }
            Err(e) => tracing::warn!("gh pr list failed for hot.md: {}", e),
        }

        let content = sections.join("\n\n");
        if let Err(e) = std::fs::write(context_dir.join("hot.md"), &content) {
            tracing::warn!("Failed to write hot.md: {}", e);
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?;

    Ok(())
}

// ── Context meta (scope config) ──────────────────────────────────────

#[tauri::command]
pub fn get_context_meta(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<ContextMeta, String> {
    let st = state.lock().map_err(|e| e.to_string())?;
    Ok(st
        .context_meta
        .get(&repo_id)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub fn save_context_scope(
    repo_id: String,
    include_globs: Vec<String>,
    exclude_globs: Vec<String>,
    precheck_model: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;
    let meta = st.context_meta.entry(repo_id).or_default();
    meta.include_globs = include_globs;
    meta.exclude_globs = exclude_globs;
    meta.precheck_model = precheck_model;
    st.save_context_meta()
}

// ── Knowledge base build ─────────────────────────────────────────────

/// Count files in a repo (excluding noise dirs) via git ls-files.
fn count_repo_files(repo_path: &std::path::Path) -> usize {
    match std::process::Command::new("git")
        .args(["ls-files"])
        .current_dir(repo_path)
        .output()
    {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).lines().count()
        }
        _ => 0,
    }
}

fn build_size_constraint(file_count: usize) -> String {
    if file_count < 500 {
        "Size constraint: SMALL repo (<500 files). Full directory walk, full git history, sample up to 20 files for convention analysis.".to_string()
    } else if file_count <= 5000 {
        "Size constraint: MEDIUM repo (500-5000 files). Max 3 directory levels deep, last 6 months of git history only, sample up to 10 files for convention analysis.".to_string()
    } else {
        "Size constraint: LARGE repo (>5000 files). Max 2 directory levels deep, last 3 months of git history only, sample up to 5 files, skip test/vendor/generated directories.".to_string()
    }
}

fn build_scope_text(meta: &ContextMeta) -> String {
    let mut parts = Vec::new();
    if !meta.include_globs.is_empty() {
        parts.push(format!(
            "Include only files matching: {}",
            meta.include_globs.join(", ")
        ));
    }
    if !meta.exclude_globs.is_empty() {
        parts.push(format!(
            "Exclude files matching: {}",
            meta.exclude_globs.join(", ")
        ));
    }
    if parts.is_empty() {
        "No scope restrictions — analyze the entire repo.".to_string()
    } else {
        parts.join("\n")
    }
}

/// Count entries in a markdown file by counting lines starting with "- " (invariants/facts)
/// or "## " headers (context entries).
fn count_md_entries(path: &std::path::Path, pattern: &str) -> u32 {
    std::fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .filter(|line| line.starts_with(pattern))
                .count() as u32
        })
        .unwrap_or(0)
}

/// Shared NDJSON stream parser for context agents (build/update).
/// Similar to parse_stream_line in agent.rs but simpler — no session tracking.
/// Returns true if the line was a "result" event (agent finished).
/// Does NOT emit AgentEvent::Done — caller should emit Done after meta is updated.
fn parse_context_stream_line(
    line: &str,
    on_event: &Channel<AgentEvent>,
    repo_path_str: &str,
) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    let Some(msg_type) = v.get("type").and_then(|t| t.as_str()) else {
        return false;
    };

    match msg_type {
        "assistant" => {
            let Some(message) = v.get("message") else {
                return false;
            };
            let Some(content) = message.get("content").and_then(|c| c.as_array()) else {
                return false;
            };

            let mut text_parts = Vec::new();
            let mut tool_uses = Vec::new();

            for block in content {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let name = block
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        let file_path = block
                            .get("input")
                            .and_then(|input| input.get("file_path"))
                            .and_then(|f| f.as_str())
                            .map(|s| {
                                let with_slash = format!("{}/", repo_path_str);
                                s.replace(&with_slash, "./").replace(repo_path_str, ".")
                            });

                        let input_preview = block.get("input").and_then(|input| {
                            if let Some(fp) = input.get("file_path").and_then(|f| f.as_str()) {
                                let with_slash = format!("{}/", repo_path_str);
                                Some(fp.replace(&with_slash, "./").replace(repo_path_str, "."))
                            } else if let Some(cmd) = input.get("command").and_then(|c| c.as_str())
                            {
                                Some(cmd.chars().take(120).collect())
                            } else if let Some(p) = input.get("pattern").and_then(|p| p.as_str()) {
                                Some(p.to_string())
                            } else if let Some(d) =
                                input.get("description").and_then(|d| d.as_str())
                            {
                                Some(d.chars().take(80).collect())
                            } else {
                                None
                            }
                        });

                        tool_uses.push(ToolUseInfo {
                            name,
                            input_preview,
                            file_path,
                            old_string: None,
                            new_string: None,
                        });
                    }
                    _ => {}
                }
            }

            let text = text_parts.join("\n");
            if !text.is_empty() || !tool_uses.is_empty() {
                let _ = on_event.send(AgentEvent::AssistantMessage {
                    text,
                    tool_uses,
                    thinking: None,
                });
            }
            false
        }
        "result" => true,
        _ => false,
    }
}

#[tauri::command]
pub fn build_knowledge_base(
    repo_id: String,
    on_event: Channel<AgentEvent>,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let (repo_path, gh_profile, context_dir, meta) = {
        let mut st = state.lock().map_err(|e| e.to_string())?;

        // Prevent concurrent builds
        if st.context_agents.contains_key(&repo_id) {
            return Err("Knowledge base build already in progress".into());
        }

        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let repo_path = repo.path.clone();
        let gh_profile = repo.gh_profile.clone();
        let context_dir = st.context_dir(&repo_id);

        let meta = st.context_meta.entry(repo_id.clone()).or_default().clone();

        // Set building status
        st.context_meta
            .get_mut(&repo_id)
            .map(|m| m.build_status = ContextBuildStatus::Building);
        let _ = st.save_context_meta();

        (repo_path, gh_profile, context_dir, meta)
    };

    let _ = std::fs::create_dir_all(&context_dir);

    // Count files for size constraint
    let file_count = count_repo_files(&repo_path);
    let size_constraint = build_size_constraint(file_count);
    let scope_text = build_scope_text(&meta);

    let output_dir = context_dir.to_string_lossy().to_string();
    let build_prompt = format!(
        r#"You are building a knowledge base for this repository.
Write all output files to: {output_dir}

{size_constraint}
{scope_text}

## Phase 1 — parallel reconnaissance

Spawn the following as parallel subagent tasks. Each writes raw findings to {output_dir}/tmp/. Do not synthesize yet.

Subagent 1 — stack
Read all manifest and lockfiles (package.json, Cargo.toml, go.mod, pyproject.toml, requirements.txt, *.lock). Extract: runtime, framework, key dependencies, build tooling, test runner. Write to {output_dir}/tmp/stack-raw.md.

Subagent 2 — structure
Walk the directory tree respecting the size constraint above. For each significant directory infer its purpose from filenames and a sample of contents. Write to {output_dir}/tmp/structure-raw.md.

Subagent 3 — git
Run:
  git log --oneline -50
  git log --oneline --merges -20
  git log --oneline --diff-filter=A -- "*.toml" "*.json" "*.lock" -20
Identify commits that look like architectural moments: dependency additions, large cross-module changes, commits with "replace", "migrate", "switch", "instead", "refactor" in message. Write candidate commits (hash, message, date) to {output_dir}/tmp/git-raw.md.

Subagent 4 — conventions
Sample files across different modules respecting the size constraint. Look for: naming patterns, error handling style, import organization, test structure. Write to {output_dir}/tmp/conventions-raw.md.

Subagent 5 — existing docs
Read if present: README.md, CLAUDE.md, AGENTS.md, docs/, any ARCHITECTURE* files. Extract explicit decisions, constraints, or conventions already documented. Write to {output_dir}/tmp/existing-raw.md.

## Phase 2 — synthesis

After all subagents complete, write the following files to {output_dir}/. Only include information you can verify — omit rather than guess.

### invariants.md

Properties that must always hold after every agent change. Hard rules, not guidelines. Only include things you can observe as invariants from the codebase or existing documentation — never invent them.

Format:
```
# Invariants

- INV-001: <one invariant, imperative, concrete>
- INV-002: ...
```

### facts.md

Observed reality: stack, runtime, key dependencies, module map, entry points, naming conventions. Include one concrete example for each convention. If you cannot find a concrete example, omit it.

### context.md

Reasoning behind non-obvious choices. Only create an entry if you found evidence (a commit, PR description, existing doc, or unmistakable pattern). Do not invent entries.

Each entry:
```
## <title-slug>-<6char hash of title>
_Touches: src/auth/*, src/middleware/*_

<2-4 sentences: what, why, what was rejected if known>
```

### index.md

```
# Index
_Last updated: <date>_

## Invariants
<one-line summary of each invariant>

## Facts
<one-line summary of key facts>

## Context entries

| ID | Summary | Touches |
|----|---------|---------|
| auth-a3f8c2 | JWT auth, no sessions | src/auth/* |

## File affinity

src/auth/*       → auth-a3f8c2
src/middleware/*  → auth-a3f8c2
src/models/*     → db-schema-7b2e
```

### contradictions.md (only if contradictions found)

If you find conflicting patterns that cannot be reconciled into a single invariant, do not add either to invariants.md. Instead write them here:

```
## CONTRA-001: <title>
Pattern A: <description>. Found in: <files>
Pattern B: <description>. Found in: <files>
Cannot determine which is canonical. Requires human resolution.
```

## Phase 3 — cleanup

Delete {output_dir}/tmp/ after synthesis. Write a brief summary of what was found and what was uncertain. Flag areas where the codebase had insufficient signal.

## Quality rules

- Every entry in invariants.md must be observable from the codebase, not aspirational
- Every convention in facts.md must have a concrete file example
- Every entry in context.md must have a source (commit, file, existing doc)
- File globs in the affinity index must match real paths in the repo
- If the repo is too new or small to have meaningful context, say so rather than padding"#
    );

    // Build claude command
    let claude_bin = get_shell_env()
        .claude_path
        .as_deref()
        .unwrap_or("claude");
    let mut cmd = std::process::Command::new(claude_bin);
    cmd.arg("-p").arg(&build_prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    cmd.arg("--dangerously-skip-permissions");
    cmd.arg("--add-dir").arg(&context_dir);
    cmd.current_dir(&repo_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    inject_shell_env(&mut cmd);

    // Inject GH token if available
    let gh_token = resolve_gh_token(&gh_profile);
    if let Some(ref token) = gh_token {
        cmd.env("GH_TOKEN", token);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn build agent: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture build agent stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture build agent stderr")?;

    // Store handle for stop_context_build
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        st.context_agents
            .insert(repo_id.clone(), AgentHandle { child });
    }

    // Read stdout in background thread
    let rid = repo_id.clone();
    let repo_path_str = repo_path.to_string_lossy().to_string();
    let ctx_dir = context_dir.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    parse_context_stream_line(&line, &on_event, &repo_path_str);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Build agent stdout read error for {}: {}", rid, e);
                    break;
                }
            }
        }

        // Drain stderr
        let mut stderr_buf = String::new();
        let mut stderr_reader = std::io::BufReader::new(stderr);
        let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut stderr_buf);
        if !stderr_buf.trim().is_empty() {
            tracing::debug!("Build agent stderr for {}: {}", rid, stderr_buf.trim());
        }

        // Clean up and update meta
        let state: State<'_, Arc<Mutex<AppState>>> = app.state();
        if let Ok(mut st) = state.lock() {
            // If stop_context_build already removed the handle and set status,
            // don't overwrite — just bail out of cleanup.
            let Some(mut handle) = st.context_agents.remove(&rid) else {
                tracing::debug!("Build agent handle already removed for {} (likely stopped)", rid);
                return;
            };

            let success = handle
                .child
                .wait()
                .map(|s| s.success())
                .unwrap_or(false);

            let meta = st.context_meta.entry(rid.clone()).or_default();
            if success && ctx_dir.join("invariants.md").exists() {
                meta.build_status = ContextBuildStatus::Built;
                meta.last_built_at = Some(now_unix());
                meta.built_at_commit = git_head_commit(&repo_path_str);
                meta.invariant_count = count_md_entries(&ctx_dir.join("invariants.md"), "- INV-");
                meta.fact_count = count_md_entries(&ctx_dir.join("facts.md"), "- ");
                meta.context_entry_count = count_md_entries(&ctx_dir.join("context.md"), "## ");
                meta.contradiction_count =
                    count_md_entries(&ctx_dir.join("contradictions.md"), "## CONTRA-");
            } else {
                meta.build_status = ContextBuildStatus::Failed;
            }
            let _ = st.save_context_meta();
        }

        // Emit Done AFTER meta is updated so frontend fetches the final state
        let _ = on_event.send(AgentEvent::Done);
        tracing::info!("Build agent finished for repo {}", rid);
    });

    tracing::info!("Spawned build agent for repo {}", repo_id);
    Ok(())
}

#[tauri::command]
pub fn stop_context_build(
    repo_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let mut st = state.lock().map_err(|e| e.to_string())?;

    if let Some(mut handle) = st.context_agents.remove(&repo_id) {
        let _ = handle.child.kill();
        let _ = handle.child.wait();
    }

    if let Some(meta) = st.context_meta.get_mut(&repo_id) {
        // Revert to previous state if not yet built
        if meta.build_status == ContextBuildStatus::Building {
            meta.build_status = if meta.last_built_at.is_some() {
                ContextBuildStatus::Built
            } else {
                ContextBuildStatus::NotBuilt
            };
        }
    }
    st.save_context_meta()?;

    tracing::info!("Stopped build agent for repo {}", repo_id);
    Ok(())
}

// ── Invariant pre-check ──────────────────────────────────────────────

#[derive(Clone, serde::Serialize)]
pub struct InvariantViolation {
    pub invariant_id: String,
    pub file: String,
    pub line: u32,
    pub description: String,
}

#[derive(Clone, serde::Serialize)]
pub struct InvariantCheckResult {
    pub passed: bool,
    pub violations: Vec<InvariantViolation>,
}

#[tauri::command]
pub async fn check_invariants(
    workspace_id: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<InvariantCheckResult, String> {
    let (worktree_path, repo_id, base_branch, precheck_model) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let repo = st.repos.get(&ws.repo_id).ok_or("Repo not found")?;
        let base = repo
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let model = st
            .context_meta
            .get(&ws.repo_id)
            .and_then(|m| {
                if m.precheck_model.is_empty() {
                    None
                } else {
                    Some(m.precheck_model.clone())
                }
            })
            .unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string());
        (ws.worktree_path.clone(), ws.repo_id.clone(), base, model)
    };

    let context_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.context_dir(&repo_id)
    };

    let invariants_path = context_dir.join("invariants.md");
    if !invariants_path.exists() {
        return Ok(InvariantCheckResult {
            passed: true,
            violations: vec![],
        });
    }

    tauri::async_runtime::spawn_blocking(move || {
        let invariants = std::fs::read_to_string(&invariants_path)
            .map_err(|e| format!("Failed to read invariants.md: {}", e))?;

        if invariants.trim().is_empty() {
            return Ok(InvariantCheckResult {
                passed: true,
                violations: vec![],
            });
        }

        // Get diff against base
        let diff_output = std::process::Command::new("git")
            .args(["diff", &format!("origin/{}", base_branch)])
            .current_dir(&worktree_path)
            .output()
            .map_err(|e| format!("Failed to get diff: {}", e))?;

        let diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
        if diff.trim().is_empty() {
            return Ok(InvariantCheckResult {
                passed: true,
                violations: vec![],
            });
        }

        // Truncate diff if too large
        let diff = if diff.len() > 30_000 {
            format!("{}...[truncated]", &diff[..30_000])
        } else {
            diff
        };

        let prompt = format!(
            "Review this diff against the invariants below.\n\
             List any violations as a JSON array. If no violations, respond with: {{\"passed\": true, \"violations\": []}}\n\n\
             Response format (JSON only, no markdown fences):\n\
             {{\"passed\": false, \"violations\": [{{\"invariant_id\": \"INV-001\", \"file\": \"path/to/file.rs\", \"line\": 42, \"description\": \"brief description\"}}]}}\n\n\
             Invariants:\n{}\n\nDiff:\n{}",
            invariants, diff
        );

        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--model", &precheck_model]);
        cmd.args(["--max-turns", "1"]);
        cmd.current_dir(&worktree_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn invariant checker: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // Strip markdown fences
                let json_str = raw
                    .strip_prefix("```json")
                    .or_else(|| raw.strip_prefix("```"))
                    .and_then(|s| s.strip_suffix("```"))
                    .map(|s| s.trim())
                    .unwrap_or(&raw);

                match serde_json::from_str::<serde_json::Value>(json_str) {
                    Ok(v) => {
                        let passed = v
                            .get("passed")
                            .and_then(|p| p.as_bool())
                            .unwrap_or(true);
                        let violations = v
                            .get("violations")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|item| {
                                        Some(InvariantViolation {
                                            invariant_id: item
                                                .get("invariant_id")
                                                .and_then(|s| s.as_str())
                                                .unwrap_or("unknown")
                                                .to_string(),
                                            file: item
                                                .get("file")
                                                .and_then(|s| s.as_str())
                                                .unwrap_or("unknown")
                                                .to_string(),
                                            line: item
                                                .get("line")
                                                .and_then(|n| n.as_u64())
                                                .unwrap_or(0)
                                                as u32,
                                            description: item
                                                .get("description")
                                                .and_then(|s| s.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        Ok(InvariantCheckResult { passed, violations })
                    }
                    Err(_) => {
                        // If response is "clean" or unparseable, assume passed
                        Ok(InvariantCheckResult {
                            passed: true,
                            violations: vec![],
                        })
                    }
                }
            }
            Ok(_) => Ok(InvariantCheckResult {
                passed: true,
                violations: vec![],
            }),
            Err(_) => {
                // Timeout — kill and fail-open
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                tracing::warn!("Invariant check timed out for workspace {}", workspace_id);
                Ok(InvariantCheckResult {
                    passed: true,
                    violations: vec![],
                })
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ── Post-merge context update ────────────────────────────────────────

#[tauri::command]
pub fn update_context_after_merge(
    repo_id: String,
    workspace_id: String,
    on_event: Channel<AgentEvent>,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let (repo_path, worktree_path, gh_profile, context_dir, ws_branch, base_branch) = {
        let st = state.lock().map_err(|e| e.to_string())?;

        // Prevent concurrent builds/updates
        if st.context_agents.contains_key(&repo_id) {
            return Err("A build or update is already in progress.".into());
        }

        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;
        let ws = st
            .workspaces
            .get(&workspace_id)
            .ok_or("Workspace not found")?;
        let base = repo
            .default_branch
            .clone()
            .unwrap_or_else(|| "main".to_string());
        (
            repo.path.clone(),
            ws.worktree_path.clone(),
            repo.gh_profile.clone(),
            st.context_dir(&repo_id),
            ws.branch.clone(),
            base,
        )
    };

    // Don't proceed if knowledge base hasn't been built
    if !context_dir.join("index.md").exists() {
        return Ok(());
    }

    // Get changed files
    let diff_output = std::process::Command::new("git")
        .args([
            "diff",
            "--name-only",
            &format!("origin/{}...{}", base_branch, ws_branch),
        ])
        .current_dir(&worktree_path)
        .output()
        .map_err(|e| format!("Failed to get changed files: {}", e))?;

    let changed_files = String::from_utf8_lossy(&diff_output.stdout).trim().to_string();
    if changed_files.is_empty() {
        return Ok(());
    }

    // Read current context files
    let index_content = std::fs::read_to_string(context_dir.join("index.md")).unwrap_or_default();
    let invariants_content =
        std::fs::read_to_string(context_dir.join("invariants.md")).unwrap_or_default();

    // Find affected entries via file affinity
    let file_list: Vec<&str> = changed_files.lines().collect();
    let affected_entries = find_entries_by_file_affinity(&index_content, &file_list);

    // Read affected context entries
    let context_content =
        std::fs::read_to_string(context_dir.join("context.md")).unwrap_or_default();
    let affected_context = extract_entries_by_id(&context_content, &affected_entries);

    let output_dir = context_dir.to_string_lossy().to_string();
    let update_prompt = format!(
        r#"The branch {ws_branch} was just merged into {base_branch}.

Changed files:
{changed_files}

{affected_section}

Current invariants.md:
{invariants_content}

Task: Review whether the merge requires updates to the context.

For each affected entry: is it still accurate? If not, update it.
Did this merge introduce a new invariant or context entry? If so, add it.
Did this merge make an existing entry obsolete? If so, remove it.

If a merged change contradicts an existing invariant, do NOT update the invariant silently.
Instead, flag it and leave invariants.md unchanged.

Write updated files to: {output_dir}
Only touch entries related to the changed files.
Do not rewrite entries unrelated to this merge.
Explain each change you made and why in a brief summary at the end."#,
        affected_section = if affected_context.is_empty() {
            "No matching context entries found in file affinity index. Check if this merge introduces something new worth adding to invariants or context.".to_string()
        } else {
            format!(
                "Affected context entries (from file affinity index):\n{}",
                affected_context
            )
        },
    );

    // Build claude command
    let claude_bin = get_shell_env()
        .claude_path
        .as_deref()
        .unwrap_or("claude");
    let mut cmd = std::process::Command::new(claude_bin);
    cmd.arg("-p").arg(&update_prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    cmd.arg("--dangerously-skip-permissions");
    cmd.arg("--add-dir").arg(&context_dir);
    cmd.current_dir(&repo_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    inject_shell_env(&mut cmd);

    let gh_token = resolve_gh_token(&gh_profile);
    if let Some(ref token) = gh_token {
        cmd.env("GH_TOKEN", token);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn update agent: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture update agent stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture update agent stderr")?;

    // Store handle for stop_context_build
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        st.context_agents
            .insert(repo_id.clone(), AgentHandle { child });
    }

    let rid = repo_id.clone();
    let repo_path_str = repo_path.to_string_lossy().to_string();
    let ctx_dir = context_dir.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    parse_context_stream_line(&line, &on_event, &repo_path_str);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Update agent stdout error for {}: {}", rid, e);
                    break;
                }
            }
        }

        // Drain stderr
        let mut stderr_buf = String::new();
        let mut stderr_reader = std::io::BufReader::new(stderr);
        let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut stderr_buf);

        // Clean up handle and update meta counts
        let state: State<'_, Arc<Mutex<AppState>>> = app.state();
        if let Ok(mut st) = state.lock() {
            let Some(mut handle) = st.context_agents.remove(&rid) else {
                // Cancelled via stop_context_build
                return;
            };
            let _ = handle.child.wait();

            if let Some(meta) = st.context_meta.get_mut(&rid) {
                meta.invariant_count =
                    count_md_entries(&ctx_dir.join("invariants.md"), "- INV-");
                meta.fact_count = count_md_entries(&ctx_dir.join("facts.md"), "- ");
                meta.context_entry_count = count_md_entries(&ctx_dir.join("context.md"), "## ");
                meta.contradiction_count =
                    count_md_entries(&ctx_dir.join("contradictions.md"), "## CONTRA-");
                meta.last_built_at = Some(now_unix());
                meta.built_at_commit = git_head_commit(&repo_path_str);
                let _ = st.save_context_meta();
            }
        }

        let _ = on_event.send(AgentEvent::Done);
        tracing::info!("Update agent finished for repo {}", rid);
    });

    Ok(())
}

// ── Incremental knowledge base update ─────────────────────────────────

#[tauri::command]
pub fn update_knowledge_base_incremental(
    repo_id: String,
    on_event: Channel<AgentEvent>,
    state: State<'_, Arc<Mutex<AppState>>>,
    app: AppHandle,
) -> Result<(), String> {
    let (repo_path, context_dir, built_at_commit) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let repo = st.repos.get(&repo_id).ok_or("Repo not found")?;

        // Precondition: KB must be built
        let meta = st
            .context_meta
            .get(&repo_id)
            .ok_or("Knowledge base not found")?;
        if meta.build_status != ContextBuildStatus::Built {
            return Err("Knowledge base is not built. Run a full build first.".into());
        }
        let commit = meta
            .built_at_commit
            .clone()
            .ok_or("No baseline commit. Run a full build first to establish a baseline.")?;

        // No concurrent builds
        if st.context_agents.contains_key(&repo_id) {
            return Err("A build or update is already in progress.".into());
        }

        (repo.path.clone(), st.context_dir(&repo_id), commit)
    };

    // Validate base commit still exists
    let cat_file = std::process::Command::new("git")
        .args(["cat-file", "-t", &built_at_commit])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("git cat-file failed: {}", e))?;
    if !cat_file.status.success() {
        return Err(format!(
            "Baseline commit {} no longer exists in the repository. A full rebuild is required.",
            &built_at_commit[..7.min(built_at_commit.len())]
        ));
    }

    // Get current HEAD
    let head_commit = git_head_commit(&repo_path.to_string_lossy())
        .ok_or("Failed to get current HEAD commit")?;

    // No changes since last build
    if head_commit == built_at_commit {
        let _ = on_event.send(AgentEvent::Done);
        return Ok(());
    }

    // Get changed files
    let diff_names_output = std::process::Command::new("git")
        .args(["diff", "--name-only", &format!("{}..HEAD", built_at_commit)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to get changed files: {}", e))?;
    let changed_files_str = String::from_utf8_lossy(&diff_names_output.stdout)
        .trim()
        .to_string();

    if changed_files_str.is_empty() {
        // No file changes (maybe only merge commits)
        let _ = on_event.send(AgentEvent::Done);
        return Ok(());
    }

    let file_count = changed_files_str.lines().count();
    if file_count > 200 {
        return Err(format!(
            "Too many changed files ({file_count}). A full rebuild is more efficient."
        ));
    }

    // Get the actual diff (truncated to ~40KB)
    let diff_output = std::process::Command::new("git")
        .args(["diff", &format!("{}..HEAD", built_at_commit)])
        .current_dir(&repo_path)
        .output()
        .map_err(|e| format!("Failed to get diff: {}", e))?;
    let full_diff = String::from_utf8_lossy(&diff_output.stdout).to_string();
    let truncated_diff = if full_diff.len() > 40_000 {
        // Fall back to stat + only affected file diffs
        let stat_output = std::process::Command::new("git")
            .args([
                "diff",
                "--stat",
                &format!("{}..HEAD", built_at_commit),
            ])
            .current_dir(&repo_path)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        format!(
            "[Diff too large ({} bytes), showing summary]\n\n{}",
            full_diff.len(),
            stat_output
        )
    } else {
        full_diff
    };

    // Read KB files
    let index_content =
        std::fs::read_to_string(context_dir.join("index.md")).unwrap_or_default();
    let invariants_content =
        std::fs::read_to_string(context_dir.join("invariants.md")).unwrap_or_default();
    let context_md =
        std::fs::read_to_string(context_dir.join("context.md")).unwrap_or_default();
    let contradictions_content =
        std::fs::read_to_string(context_dir.join("contradictions.md")).unwrap_or_default();

    // Find affected entries via file affinity
    let file_list: Vec<&str> = changed_files_str.lines().collect();
    let affected_entries = find_entries_by_file_affinity(&index_content, &file_list);
    let affected_context = extract_entries_by_id(&context_md, &affected_entries);

    // Extract unresolved contradictions only
    let unresolved_contradictions: String = {
        let mut result = String::new();
        let mut capturing = false;
        for line in contradictions_content.lines() {
            if line.starts_with("## CONTRA-") {
                let is_resolved =
                    line.contains("<!-- RESOLVED") || line.contains("<!-- TECH_DEBT");
                capturing = !is_resolved;
            }
            if capturing {
                result.push_str(line);
                result.push('\n');
            }
        }
        result
    };

    let base_short = &built_at_commit[..7.min(built_at_commit.len())];
    let head_short = &head_commit[..7.min(head_commit.len())];
    let output_dir = context_dir.to_string_lossy().to_string();

    let update_prompt = format!(
        r#"You are updating a knowledge base after codebase changes.

## What changed (diff from {base_short} to {head_short})

Changed files:
{changed_files_str}

Diff:
{truncated_diff}

## Current affected KB entries

{affected_section}

## Current invariants

{invariants_content}

{contradictions_section}

## Instructions

1. For each affected context entry: update if inaccurate given the diff. Keep the same entry ID and format (`## <title-slug>-<hash>`).
2. If the diff introduces a new pattern/dependency/decision worth tracking, add a NEW context entry with a new title-slug and 6-char hash.
3. If the diff makes an entry completely obsolete (removed feature, deleted module), remove only that entry.
4. Check invariants against the diff:
   - If STRENGTHENED by the changes: update its wording.
   - If VIOLATED by the changes: do NOT silently update. Add a new contradiction to contradictions.md.
   - If new invariant-worthy pattern introduced: add it with the next INV-NNN number.
5. If the diff changes facts (new dependency, renamed module, changed convention), update the relevant sections in facts.md. Only touch facts directly affected.
6. Update the file affinity section of index.md if new files/directories were added or paths changed.
7. DO NOT rewrite entries unrelated to the changed files.
8. DO NOT remove entries just because you cannot verify them from the diff — they may still be valid.
9. Preserve all `<!-- RESOLVED -->` and `<!-- TECH_DEBT -->` tags in contradictions.md.

Write updated files to: {output_dir}
Only overwrite files you actually changed. If a file needs no changes, do not write it.
End with a brief summary of changes made."#,
        affected_section = if affected_context.is_empty() {
            "No matching context entries found in file affinity index. Check if this diff introduces something new worth adding.".to_string()
        } else {
            affected_context
        },
        contradictions_section = if unresolved_contradictions.is_empty() {
            String::new()
        } else {
            format!(
                "## Unresolved contradictions\n\n{}",
                unresolved_contradictions
            )
        },
    );

    // Spawn Claude
    let claude_bin = get_shell_env()
        .claude_path
        .as_deref()
        .unwrap_or("claude");
    let mut cmd = std::process::Command::new(claude_bin);
    cmd.arg("-p").arg(&update_prompt);
    cmd.args(["--output-format", "stream-json", "--verbose"]);
    cmd.arg("--dangerously-skip-permissions");
    cmd.arg("--add-dir").arg(&context_dir);
    cmd.current_dir(&repo_path);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    inject_shell_env(&mut cmd);

    // Inject GH token if available
    {
        let st = state.lock().map_err(|e| e.to_string())?;
        let gh_profile = st.repos.get(&repo_id).and_then(|r| r.gh_profile.clone());
        let gh_token = resolve_gh_token(&gh_profile);
        if let Some(ref token) = gh_token {
            cmd.env("GH_TOKEN", token);
        }
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn incremental update agent: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Failed to capture update agent stdout")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Failed to capture update agent stderr")?;

    // Store handle for cancellation
    {
        let mut st = state.lock().map_err(|e| e.to_string())?;
        st.context_meta
            .entry(repo_id.clone())
            .or_default()
            .build_status = ContextBuildStatus::Building;
        let _ = st.save_context_meta();
        st.context_agents
            .insert(repo_id.clone(), AgentHandle { child });
    }

    let rid = repo_id.clone();
    let repo_path_str = repo_path.to_string_lossy().to_string();
    let ctx_dir = context_dir.clone();
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) if !line.is_empty() => {
                    parse_context_stream_line(&line, &on_event, &repo_path_str);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::debug!("Incremental update stdout error for {}: {}", rid, e);
                    break;
                }
            }
        }

        // Drain stderr
        let mut stderr_buf = String::new();
        let mut stderr_reader = std::io::BufReader::new(stderr);
        let _ = std::io::Read::read_to_string(&mut stderr_reader, &mut stderr_buf);

        // Clean up and update meta
        let state: State<'_, Arc<Mutex<AppState>>> = app.state();
        if let Ok(mut st) = state.lock() {
            let Some(mut handle) = st.context_agents.remove(&rid) else {
                // Cancelled via stop_context_build
                return;
            };

            let success = handle
                .child
                .wait()
                .map(|s| s.success())
                .unwrap_or(false);

            if let Some(meta) = st.context_meta.get_mut(&rid) {
                if success {
                    meta.build_status = ContextBuildStatus::Built;
                    meta.last_built_at = Some(now_unix());
                    meta.built_at_commit = git_head_commit(&repo_path_str);
                    meta.invariant_count =
                        count_md_entries(&ctx_dir.join("invariants.md"), "- INV-");
                    meta.fact_count = count_md_entries(&ctx_dir.join("facts.md"), "- ");
                    meta.context_entry_count =
                        count_md_entries(&ctx_dir.join("context.md"), "## ");
                    meta.contradiction_count =
                        count_md_entries(&ctx_dir.join("contradictions.md"), "## CONTRA-");
                } else {
                    // Revert to Built — the existing KB is still valid
                    meta.build_status = ContextBuildStatus::Built;
                }
                let _ = st.save_context_meta();
            }
        }

        let _ = on_event.send(AgentEvent::Done);
        tracing::info!("Incremental update finished for repo {}", rid);
    });

    Ok(())
}

// ── Read context file (for frontend display) ─────────────────────────

#[tauri::command]
pub fn read_context_file(
    repo_id: String,
    filename: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let context_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.context_dir(&repo_id)
    };

    // Sanitize filename — only allow known context files
    let allowed = [
        "invariants.md",
        "facts.md",
        "context.md",
        "index.md",
        "hot.md",
        "contradictions.md",
    ];
    if !allowed.contains(&filename.as_str()) {
        return Err("Invalid context filename".into());
    }

    let path = context_dir.join(&filename);
    if !path.exists() {
        return Ok(String::new());
    }
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", filename, e))
}

#[tauri::command]
pub fn write_context_file(
    repo_id: String,
    filename: String,
    content: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let context_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.context_dir(&repo_id)
    };

    let allowed = [
        "invariants.md",
        "facts.md",
        "context.md",
        "index.md",
        "contradictions.md",
    ];
    if !allowed.contains(&filename.as_str()) {
        return Err("Invalid context filename".into());
    }

    let path = context_dir.join(&filename);
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", filename, e))?;

    // Recount entries and update meta
    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(meta) = st.context_meta.get_mut(&repo_id) {
        meta.invariant_count = count_md_entries(&context_dir.join("invariants.md"), "- INV-");
        meta.fact_count = count_md_entries(&context_dir.join("facts.md"), "- ");
        meta.context_entry_count = count_md_entries(&context_dir.join("context.md"), "## ");
        meta.contradiction_count = context_dir
            .join("contradictions.md")
            .exists()
            .then(|| {
                std::fs::read_to_string(context_dir.join("contradictions.md"))
                    .unwrap_or_default()
                    .lines()
                    .filter(|l| {
                        l.starts_with("## CONTRA-")
                            && !l.contains("<!-- RESOLVED")
                            && !l.contains("<!-- TECH_DEBT")
                    })
                    .count() as u32
            })
            .unwrap_or(0);
        let _ = st.save_context_meta();
    }

    Ok(())
}

// ── Resolve contradiction (LLM-assisted) ─────────────────────────────

/// Draft a resolution for a contradiction using the pre-check model.
/// Returns the drafted invariants.md content (full file, not a diff).
#[tauri::command]
pub async fn draft_contradiction_resolution(
    repo_id: String,
    contradiction_id: String,
    resolution: String,
    user_note: String,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<String, String> {
    let (context_dir, precheck_model) = {
        let st = state.lock().map_err(|e| e.to_string())?;
        let dir = st.context_dir(&repo_id);
        let model = st
            .context_meta
            .get(&repo_id)
            .and_then(|m| {
                if m.precheck_model.is_empty() {
                    None
                } else {
                    Some(m.precheck_model.clone())
                }
            })
            .unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string());
        (dir, model)
    };

    let invariants = std::fs::read_to_string(context_dir.join("invariants.md"))
        .unwrap_or_default();
    let contradictions = std::fs::read_to_string(context_dir.join("contradictions.md"))
        .unwrap_or_default();

    // Extract the specific contradiction
    let mut contra_text = String::new();
    let mut found = false;
    for line in contradictions.lines() {
        if line.starts_with(&format!("## {}", contradiction_id)) {
            found = true;
            contra_text.push_str(line);
            contra_text.push('\n');
        } else if found {
            if line.starts_with("## ") {
                break;
            }
            contra_text.push_str(line);
            contra_text.push('\n');
        }
    }

    if contra_text.is_empty() {
        return Err(format!("Contradiction {} not found", contradiction_id));
    }

    let resolution_instruction = match resolution.as_str() {
        "exception" => format!(
            "The human says BOTH patterns are intentional. Their note: \"{}\"\n\
             Add a new invariant that captures the exception rule. Do NOT remove existing invariants.",
            user_note
        ),
        "update_invariant" => format!(
            "The human says one pattern is correct and the other is wrong/stale. Their note: \"{}\"\n\
             If an existing invariant needs updating, modify it in place. \
             If a new invariant is needed, add it with the next available INV-NNN number. \
             Do NOT remove unrelated invariants.",
            user_note
        ),
        "tech_debt" => {
            // Tech debt doesn't need LLM drafting — just mark it
            return Ok(invariants);
        }
        _ => return Err(format!("Unknown resolution type: {}", resolution)),
    };

    let prompt = format!(
        "You are updating a knowledge base's invariants file.\n\n\
         ## Contradiction being resolved\n\
         {contra_text}\n\
         ## Resolution direction\n\
         {resolution_instruction}\n\n\
         ## Current invariants.md\n\
         {invariants}\n\n\
         ## Instructions\n\
         Output the COMPLETE updated invariants.md file. Keep the exact format:\n\
         ```\n\
         # Invariants\n\n\
         - INV-001: description\n\
         - INV-002: description\n\
         ```\n\
         Rules:\n\
         - Preserve ALL existing invariants that are still valid\n\
         - Only add/modify what the resolution requires\n\
         - Use the next available INV-NNN number for new entries\n\
         - Each invariant must be concrete and observable, not aspirational\n\
         - Output ONLY the file content, no explanation, no markdown fences"
    );

    tauri::async_runtime::spawn_blocking(move || {
        let claude_bin = get_shell_env()
            .claude_path
            .as_deref()
            .unwrap_or("claude");

        let mut cmd = std::process::Command::new(claude_bin);
        cmd.arg("-p").arg(&prompt);
        cmd.args(["--output-format", "text"]);
        cmd.args(["--model", &precheck_model]);
        cmd.args(["--max-turns", "1"]);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::null());
        inject_shell_env(&mut cmd);

        let child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn draft agent: {}", e))?;

        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let output = child.wait_with_output();
            let _ = tx.send(output);
        });

        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(Ok(output)) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
                // Strip markdown fences if the model wrapped it
                let clean = raw
                    .strip_prefix("```markdown")
                    .or_else(|| raw.strip_prefix("```md"))
                    .or_else(|| raw.strip_prefix("```"))
                    .and_then(|s| s.strip_suffix("```"))
                    .map(|s| s.trim().to_string())
                    .unwrap_or(raw);
                Ok(clean)
            }
            Ok(Ok(output)) => {
                let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
                Err(format!("Draft agent failed: {}", err))
            }
            Ok(Err(e)) => Err(format!("Draft agent error: {}", e)),
            Err(_) => {
                let _ = std::process::Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .output();
                Err("Draft agent timed out (60s)".to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

/// Apply a reviewed resolution: write the approved invariants content and mark contradiction resolved.
#[tauri::command]
pub fn resolve_contradiction(
    repo_id: String,
    contradiction_id: String,
    resolution: String,
    invariant_text: Option<String>,
    state: State<'_, Arc<Mutex<AppState>>>,
) -> Result<(), String> {
    let context_dir = {
        let st = state.lock().map_err(|e| e.to_string())?;
        st.context_dir(&repo_id)
    };

    let contradictions_path = context_dir.join("contradictions.md");
    if !contradictions_path.exists() {
        return Err("No contradictions file found".into());
    }

    let content = std::fs::read_to_string(&contradictions_path)
        .map_err(|e| format!("Failed to read contradictions.md: {}", e))?;

    let tag = match resolution.as_str() {
        "exception" => "RESOLVED: exception",
        "update_invariant" => "RESOLVED: updated",
        "tech_debt" => "TECH_DEBT",
        _ => return Err(format!("Unknown resolution type: {}", resolution)),
    };

    // Mark contradiction as resolved
    let updated = content.replace(
        &format!("## {}", contradiction_id),
        &format!("## {} <!-- {} -->", contradiction_id, tag),
    );
    std::fs::write(&contradictions_path, updated).map_err(|e| e.to_string())?;

    // Write the LLM-drafted (and human-approved) invariants content
    if let Some(approved_invariants) = invariant_text {
        if !approved_invariants.trim().is_empty() {
            let inv_path = context_dir.join("invariants.md");
            std::fs::write(&inv_path, &approved_invariants).map_err(|e| e.to_string())?;
        }
    }

    // Recount
    let updated_content = std::fs::read_to_string(&contradictions_path).unwrap_or_default();
    let unresolved_count = updated_content
        .lines()
        .filter(|line| {
            line.starts_with("## CONTRA-")
                && !line.contains("<!-- RESOLVED")
                && !line.contains("<!-- TECH_DEBT")
        })
        .count() as u32;

    let mut st = state.lock().map_err(|e| e.to_string())?;
    if let Some(meta) = st.context_meta.get_mut(&repo_id) {
        meta.contradiction_count = unresolved_count;
        meta.invariant_count = count_md_entries(&context_dir.join("invariants.md"), "- INV-");
        let _ = st.save_context_meta();
    }

    Ok(())
}

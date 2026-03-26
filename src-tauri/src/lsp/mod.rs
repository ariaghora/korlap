pub mod detect;
pub mod server;
pub mod types;

use server::{path_to_uri, send_notification, send_request, uri_to_path};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use types::{LspError, LspServerHandle};

const QUERY_TIMEOUT: Duration = Duration::from_secs(30);

// ── Document lifecycle ──────────────────────────────────────────────

/// Open a file in the LSP server (reads content from disk).
/// Returns true if we opened it (caller should close after query).
fn ensure_document_open(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    language_id: &str,
) -> Result<bool, LspError> {
    let uri = path_to_uri(file_path);

    let already_open = {
        let handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;
        handle.open_documents.contains(&uri)
    };

    if already_open {
        return Ok(false);
    }

    let text = std::fs::read_to_string(file_path).map_err(|e| {
        LspError::Transport(format!("Cannot read {}: {}", file_path.display(), e))
    })?;

    send_notification(
        handle_arc,
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        }),
    )?;

    {
        let mut handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;
        handle.open_documents.insert(uri);
    }

    Ok(true)
}

fn close_document(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
) -> Result<(), LspError> {
    let uri = path_to_uri(file_path);
    send_notification(
        handle_arc,
        "textDocument/didClose",
        serde_json::json!({
            "textDocument": { "uri": uri }
        }),
    )?;

    let mut handle = handle_arc
        .lock()
        .map_err(|e| LspError::Transport(e.to_string()))?;
    handle.open_documents.remove(&uri);
    Ok(())
}

/// Add a worktree as a workspace folder to an existing server.
pub fn add_worktree(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    worktree_path: &Path,
) -> Result<(), LspError> {
    // Check if already registered
    {
        let handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;
        if handle
            .workspace_folders
            .contains(&worktree_path.to_path_buf())
        {
            return Ok(());
        }
    }

    let uri = path_to_uri(worktree_path);
    let name = worktree_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    send_notification(
        handle_arc,
        "workspace/didChangeWorkspaceFolders",
        serde_json::json!({
            "event": {
                "added": [{ "uri": uri, "name": name }],
                "removed": []
            }
        }),
    )?;

    let mut handle = handle_arc
        .lock()
        .map_err(|e| LspError::Transport(e.to_string()))?;
    handle.workspace_folders.push(worktree_path.to_path_buf());
    Ok(())
}

// ── High-level LSP operations ───────────────────────────────────────
//
// All positions: line/character are 0-based (LSP protocol).
// The MCP API layer converts from 1-based (agent-facing) to 0-based.

pub fn goto_definition(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    line: u32,
    character: u32,
    language_id: &str,
) -> Result<serde_json::Value, LspError> {
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    let result = send_request(
        handle_arc,
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }),
        QUERY_TIMEOUT,
    );

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    result
}

pub fn find_references(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    line: u32,
    character: u32,
    language_id: &str,
    include_declaration: bool,
) -> Result<serde_json::Value, LspError> {
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    let result = send_request(
        handle_arc,
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": include_declaration }
        }),
        QUERY_TIMEOUT,
    );

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    result
}

pub fn hover(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    line: u32,
    character: u32,
    language_id: &str,
) -> Result<serde_json::Value, LspError> {
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    let result = send_request(
        handle_arc,
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }),
        QUERY_TIMEOUT,
    );

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    result
}

pub fn workspace_symbols(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    query: &str,
) -> Result<serde_json::Value, LspError> {
    send_request(
        handle_arc,
        "workspace/symbol",
        serde_json::json!({ "query": query }),
        QUERY_TIMEOUT,
    )
}

pub fn get_diagnostics(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    language_id: &str,
) -> Result<serde_json::Value, LspError> {
    // Open file to trigger publishDiagnostics from the server
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    // Give the server time to analyze and push diagnostics
    std::thread::sleep(Duration::from_millis(500));

    let result = {
        let handle = handle_arc
            .lock()
            .map_err(|e| LspError::Transport(e.to_string()))?;
        handle
            .diagnostics_cache
            .get(&uri)
            .cloned()
            .unwrap_or(serde_json::json!([]))
    };

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    Ok(result)
}

pub fn prepare_rename(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    line: u32,
    character: u32,
    language_id: &str,
) -> Result<serde_json::Value, LspError> {
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    let result = send_request(
        handle_arc,
        "textDocument/prepareRename",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        }),
        QUERY_TIMEOUT,
    );

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    result
}

pub fn rename(
    handle_arc: &Arc<Mutex<LspServerHandle>>,
    file_path: &Path,
    line: u32,
    character: u32,
    new_name: &str,
    language_id: &str,
) -> Result<serde_json::Value, LspError> {
    let we_opened = ensure_document_open(handle_arc, file_path, language_id)?;
    let uri = path_to_uri(file_path);

    let result = send_request(
        handle_arc,
        "textDocument/rename",
        serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "newName": new_name
        }),
        QUERY_TIMEOUT,
    );

    if we_opened {
        let _ = close_document(handle_arc, file_path);
    }

    result
}

/// Apply a WorkspaceEdit to files on disk. Returns a summary of changes.
pub fn apply_workspace_edit(
    edit: &serde_json::Value,
    worktree_path: &Path,
) -> Result<Vec<(String, usize)>, String> {
    let mut summary: Vec<(String, usize)> = Vec::new(); // (relative_path, edit_count)

    // Handle "changes": { uri: TextEdit[] }
    if let Some(changes) = edit.get("changes").and_then(|c| c.as_object()) {
        for (uri, edits) in changes {
            let abs_path = uri_to_path(uri)
                .ok_or_else(|| format!("Invalid URI: {}", uri))?;
            let rel_path = abs_path
                .strip_prefix(worktree_path)
                .unwrap_or(&abs_path)
                .display()
                .to_string();

            let edits_arr = edits.as_array().ok_or("edits not an array")?;
            let content = std::fs::read_to_string(&abs_path)
                .map_err(|e| format!("Cannot read {}: {}", rel_path, e))?;

            let new_content = apply_text_edits(&content, edits_arr)?;
            std::fs::write(&abs_path, &new_content)
                .map_err(|e| format!("Cannot write {}: {}", rel_path, e))?;

            summary.push((rel_path, edits_arr.len()));
        }
    }

    // Handle "documentChanges": TextDocumentEdit[]
    if let Some(doc_changes) = edit.get("documentChanges").and_then(|c| c.as_array()) {
        for change in doc_changes {
            // Skip create/rename/delete operations — only handle TextDocumentEdit
            let Some(text_doc) = change.get("textDocument") else { continue };
            let Some(uri) = text_doc.get("uri").and_then(|u| u.as_str()) else { continue };
            let Some(edits) = change.get("edits").and_then(|e| e.as_array()) else { continue };

            let abs_path = uri_to_path(uri)
                .ok_or_else(|| format!("Invalid URI: {}", uri))?;
            let rel_path = abs_path
                .strip_prefix(worktree_path)
                .unwrap_or(&abs_path)
                .display()
                .to_string();

            let content = std::fs::read_to_string(&abs_path)
                .map_err(|e| format!("Cannot read {}: {}", rel_path, e))?;

            let new_content = apply_text_edits(&content, edits)?;
            std::fs::write(&abs_path, &new_content)
                .map_err(|e| format!("Cannot write {}: {}", rel_path, e))?;

            summary.push((rel_path, edits.len()));
        }
    }

    Ok(summary)
}

/// Apply LSP TextEdit[] to a string. Edits are applied in reverse order
/// (bottom-up) so earlier edits don't shift positions of later ones.
fn apply_text_edits(content: &str, edits: &[serde_json::Value]) -> Result<String, String> {
    let lines: Vec<&str> = content.lines().collect();

    // Convert each edit to (byte_offset_start, byte_offset_end, new_text)
    let mut byte_edits: Vec<(usize, usize, String)> = Vec::new();

    for edit in edits {
        let range = edit.get("range").ok_or("edit missing range")?;
        let start = range.get("start").ok_or("range missing start")?;
        let end = range.get("end").ok_or("range missing end")?;

        let start_line = start.get("line").and_then(|l| l.as_u64()).ok_or("missing start line")? as usize;
        let start_char = start.get("character").and_then(|c| c.as_u64()).ok_or("missing start character")? as usize;
        let end_line = end.get("line").and_then(|l| l.as_u64()).ok_or("missing end line")? as usize;
        let end_char = end.get("character").and_then(|c| c.as_u64()).ok_or("missing end character")? as usize;

        let new_text = edit.get("newText").and_then(|t| t.as_str()).unwrap_or("");

        // Convert line:char to byte offset
        let start_offset = line_char_to_offset(content, &lines, start_line, start_char);
        let end_offset = line_char_to_offset(content, &lines, end_line, end_char);

        byte_edits.push((start_offset, end_offset, new_text.to_string()));
    }

    // Sort by start offset descending — apply from bottom to top
    byte_edits.sort_by(|a, b| b.0.cmp(&a.0));

    let mut result = content.to_string();
    for (start, end, new_text) in byte_edits {
        let start = start.min(result.len());
        let end = end.min(result.len());
        result.replace_range(start..end, &new_text);
    }

    Ok(result)
}

fn line_char_to_offset(content: &str, lines: &[&str], line: usize, character: usize) -> usize {
    let mut offset = 0;
    for (i, l) in lines.iter().enumerate() {
        if i == line {
            // LSP character offsets are UTF-16 code units, but for ASCII-heavy code
            // treating them as byte offsets is close enough. For full correctness,
            // we'd need UTF-16 offset conversion.
            return offset + character.min(l.len());
        }
        offset += l.len() + 1; // +1 for newline
    }
    // Past end of file
    content.len()
}

/// Format a WorkspaceEdit summary for agent output.
pub fn format_rename_result(summary: &[(String, usize)]) -> String {
    if summary.is_empty() {
        return "No changes made.".to_string();
    }

    let total_edits: usize = summary.iter().map(|(_, n)| n).sum();
    let mut lines = vec![format!(
        "Renamed across {} {} ({} {}):",
        summary.len(),
        if summary.len() == 1 { "file" } else { "files" },
        total_edits,
        if total_edits == 1 { "edit" } else { "edits" },
    )];

    for (path, count) in summary {
        lines.push(format!("  {} ({} {})", path, count, if *count == 1 { "edit" } else { "edits" }));
    }

    lines.join("\n")
}

// ── Response formatting ─────────────────────────────────────────────
//
// Convert raw LSP JSON responses into agent-friendly text.
// All positions are converted from 0-based (LSP) to 1-based (output).

/// Format a definition/references result (Location or Location[]) relative to a worktree.
pub fn format_locations(
    value: &serde_json::Value,
    worktree_path: &Path,
) -> String {
    let locations = match value {
        serde_json::Value::Array(arr) => arr.clone(),
        serde_json::Value::Object(_) => vec![value.clone()],
        serde_json::Value::Null => return "No results found.".to_string(),
        _ => return "No results found.".to_string(),
    };

    if locations.is_empty() {
        return "No results found.".to_string();
    }

    let mut lines = Vec::new();
    for loc in &locations {
        let uri = loc.get("uri").and_then(|u| u.as_str()).unwrap_or("");
        let range = loc.get("range").unwrap_or(&serde_json::Value::Null);
        let start = range.get("start").unwrap_or(&serde_json::Value::Null);
        let line = start.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
        let char = start
            .get("character")
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as u32;

        let rel_path = uri_to_path(uri)
            .and_then(|p| p.strip_prefix(worktree_path).ok().map(|r| r.to_path_buf()))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| uri.to_string());

        // Read context line from disk
        let context = uri_to_path(uri)
            .and_then(|p| read_line_from_file(&p, line))
            .map(|l| format!("  > {}", l.trim()))
            .unwrap_or_default();

        lines.push(format!("{}:{}:{}{}", rel_path, line + 1, char + 1, context));
    }

    lines.join("\n")
}

/// Extract hover content with its markup kind.
/// Returns (kind, text) where kind is "markdown" or "plaintext".
pub fn extract_hover(value: &serde_json::Value) -> Option<(String, String)> {
    if value.is_null() {
        return None;
    }

    let contents = value.get("contents").unwrap_or(value);

    // MarkupContent: { kind, value }
    if let Some(val) = contents.get("value").and_then(|v| v.as_str()) {
        let kind = contents
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or("markdown")
            .to_string();
        return Some((kind, val.to_string()));
    }

    // Plain string
    if let Some(s) = contents.as_str() {
        return Some(("plaintext".to_string(), s.to_string()));
    }

    // Array of MarkedString: { language, value } or plain string
    if let Some(arr) = contents.as_array() {
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|item| {
                if let Some(lang) = item.get("language").and_then(|l| l.as_str()) {
                    let val = item.get("value").and_then(|v| v.as_str()).unwrap_or("");
                    Some(format!("```{}\n{}\n```", lang, val))
                } else if let Some(val) = item.get("value").and_then(|v| v.as_str()) {
                    Some(val.to_string())
                } else {
                    item.as_str().map(|s| s.to_string())
                }
            })
            .collect();
        if parts.is_empty() {
            return None;
        }
        return Some(("markdown".to_string(), parts.join("\n\n")));
    }

    None
}

/// Format hover result as readable text (for MCP/agent output).
pub fn format_hover(value: &serde_json::Value) -> String {
    extract_hover(value)
        .map(|(_, text)| text)
        .unwrap_or_else(|| "No hover information available.".to_string())
}

/// Format workspace symbols as readable text.
pub fn format_symbols(
    value: &serde_json::Value,
    worktree_path: &Path,
) -> String {
    let symbols = match value.as_array() {
        Some(arr) => arr,
        None => return "No symbols found.".to_string(),
    };

    if symbols.is_empty() {
        return "No symbols found.".to_string();
    }

    let mut lines = Vec::new();
    for sym in symbols.iter().take(30) {
        let name = sym.get("name").and_then(|n| n.as_str()).unwrap_or("?");
        let kind = sym
            .get("kind")
            .and_then(|k| k.as_u64())
            .map(symbol_kind_name)
            .unwrap_or("unknown");

        // SymbolInformation has location.uri, DocumentSymbol doesn't
        let loc_uri = sym
            .get("location")
            .and_then(|l| l.get("uri"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let loc_range = sym
            .get("location")
            .and_then(|l| l.get("range"))
            .and_then(|r| r.get("start"));
        let line = loc_range
            .and_then(|s| s.get("line"))
            .and_then(|l| l.as_u64())
            .unwrap_or(0) as u32;
        let char = loc_range
            .and_then(|s| s.get("character"))
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as u32;

        let rel_path = uri_to_path(loc_uri)
            .and_then(|p| p.strip_prefix(worktree_path).ok().map(|r| r.to_path_buf()))
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| loc_uri.to_string());

        lines.push(format!("{} ({}) \u{2014} {}:{}:{}", name, kind, rel_path, line + 1, char + 1));
    }

    if symbols.len() > 30 {
        lines.push(format!("... and {} more", symbols.len() - 30));
    }

    lines.join("\n")
}

/// Format diagnostics as readable text.
pub fn format_diagnostics(value: &serde_json::Value) -> String {
    let diagnostics = match value.as_array() {
        Some(arr) => arr,
        None => return "No diagnostics.".to_string(),
    };

    if diagnostics.is_empty() {
        return "No diagnostics.".to_string();
    }

    let mut lines = Vec::new();
    for diag in diagnostics {
        let severity = diag
            .get("severity")
            .and_then(|s| s.as_u64())
            .map(|s| match s {
                1 => "ERROR",
                2 => "WARNING",
                3 => "INFO",
                4 => "HINT",
                _ => "UNKNOWN",
            })
            .unwrap_or("UNKNOWN");

        let range = diag.get("range").unwrap_or(&serde_json::Value::Null);
        let start = range.get("start").unwrap_or(&serde_json::Value::Null);
        let line = start.get("line").and_then(|l| l.as_u64()).unwrap_or(0) as u32;
        let char = start
            .get("character")
            .and_then(|c| c.as_u64())
            .unwrap_or(0) as u32;

        let message = diag
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("?");
        let source = diag
            .get("source")
            .and_then(|s| s.as_str())
            .unwrap_or("");

        let source_suffix = if source.is_empty() {
            String::new()
        } else {
            format!(" ({})", source)
        };

        lines.push(format!(
            "{} {}:{} \u{2014} {}{}",
            severity,
            line + 1,
            char + 1,
            message,
            source_suffix
        ));
    }

    lines.join("\n")
}

// ── Utilities ───────────────────────────────────────────────────────

fn read_line_from_file(path: &Path, line_0based: u32) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().nth(line_0based as usize).map(String::from)
}

fn symbol_kind_name(kind: u64) -> &'static str {
    match kind {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type parameter",
        _ => "unknown",
    }
}

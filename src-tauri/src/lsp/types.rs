use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::mpsc;

/// User-configurable language server definition.
/// Stored in RepoSettings.lsp_servers, keyed by an arbitrary id (e.g. "rust", "svelte").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Binary to spawn (must be on PATH).
    pub command: String,
    /// CLI args passed to the binary.
    #[serde(default)]
    pub args: Vec<String>,
    /// File extensions this server handles (without dot): ["rs"], ["ts","tsx","js","jsx"].
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Project files whose presence triggers auto-detection: ["Cargo.toml"].
    #[serde(default)]
    pub detect_files: Vec<String>,
    /// LSP languageId sent in textDocument/didOpen.
    pub language_id: String,
    /// Human-readable install instructions shown when binary is missing.
    #[serde(default)]
    pub install_hint: String,
    /// Explicit project roots relative to repo root (e.g. ["src-tauri", "packages/backend"]).
    /// When set, detect_files are checked only in these directories.
    /// When empty (default), auto-detection scans root + one level of subdirectories.
    #[serde(default)]
    pub project_roots: Vec<String>,
}

/// Built-in defaults. Merged with user overrides from RepoSettings — user wins on conflict.
pub fn builtin_configs() -> HashMap<String, LspServerConfig> {
    let mut m = HashMap::new();
    m.insert("rust".into(), LspServerConfig {
        command: "rust-analyzer".into(),
        args: vec![],
        extensions: vec!["rs".into()],
        detect_files: vec!["Cargo.toml".into()],
        language_id: "rust".into(),
        install_hint: "rustup component add rust-analyzer".into(),
        project_roots: vec![],
    });
    m.insert("typescript".into(), LspServerConfig {
        command: "typescript-language-server".into(),
        args: vec!["--stdio".into()],
        extensions: vec!["ts".into(), "tsx".into(), "js".into(), "jsx".into(), "mts".into(), "mjs".into(), "cts".into(), "cjs".into()],
        detect_files: vec!["tsconfig.json".into(), "package.json".into()],
        language_id: "typescript".into(),
        install_hint: "bun i -g typescript-language-server typescript".into(),
        project_roots: vec![],
    });
    m.insert("go".into(), LspServerConfig {
        command: "gopls".into(),
        args: vec!["serve".into()],
        extensions: vec!["go".into()],
        detect_files: vec!["go.mod".into()],
        language_id: "go".into(),
        install_hint: "go install golang.org/x/tools/gopls@latest".into(),
        project_roots: vec![],
    });
    m.insert("python".into(), LspServerConfig {
        command: "pyright-langserver".into(),
        args: vec!["--stdio".into()],
        extensions: vec!["py".into(), "pyi".into()],
        detect_files: vec!["pyproject.toml".into(), "requirements.txt".into(), "setup.py".into()],
        language_id: "python".into(),
        install_hint: "pip install pyright".into(),
        project_roots: vec![],
    });
    m
}

/// Merge built-in defaults with user overrides. User configs win on key collision.
pub fn resolve_configs(user_overrides: &HashMap<String, LspServerConfig>) -> HashMap<String, LspServerConfig> {
    let mut merged = builtin_configs();
    for (k, v) in user_overrides {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// Find the config that handles a given file extension.
pub fn config_for_extension<'a>(
    configs: &'a HashMap<String, LspServerConfig>,
    ext: &str,
) -> Option<(&'a str, &'a LspServerConfig)> {
    configs.iter().find_map(|(id, cfg)| {
        if cfg.extensions.iter().any(|e| e == ext) {
            Some((id.as_str(), cfg))
        } else {
            None
        }
    })
}

/// Uniquely identifies an LSP server instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LspServerKey {
    pub repo_id: String,
    /// The config id (e.g. "rust", "typescript", "svelte").
    pub server_id: String,
}

/// Handle to a running LSP server. Lives behind its own Arc<Mutex<>>.
pub struct LspServerHandle {
    pub child: Child,
    pub stdin: BufWriter<ChildStdin>,
    pub next_id: i64,
    /// Pending requests: JSON-RPC id -> sender for the response.
    pub pending: HashMap<i64, mpsc::Sender<LspResult>>,
    /// Workspace folders currently registered with this server.
    pub workspace_folders: Vec<PathBuf>,
    /// Whether initialize handshake is complete.
    pub initialized: bool,
    /// Set of document URIs currently open via didOpen.
    pub open_documents: HashSet<String>,
    /// Diagnostics pushed by the server (keyed by document URI).
    pub diagnostics_cache: HashMap<String, serde_json::Value>,
}

pub type LspResult = Result<serde_json::Value, LspError>;

#[derive(Debug)]
pub enum LspError {
    /// Server returned a JSON-RPC error.
    ServerError { code: i64, message: String },
    /// Transport/IO failure.
    Transport(String),
    /// Request timed out.
    Timeout,
    /// Server is not running / crashed.
    ServerDead,
    /// Binary not found on PATH.
    BinaryNotFound(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspError::ServerError { code, message } => write!(f, "LSP error {}: {}", code, message),
            LspError::Transport(msg) => write!(f, "LSP transport error: {}", msg),
            LspError::Timeout => write!(f, "LSP request timed out"),
            LspError::ServerDead => write!(f, "LSP server is not running"),
            LspError::BinaryNotFound(bin) => write!(f, "LSP binary not found: {}", bin),
        }
    }
}

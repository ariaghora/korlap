import { invoke, Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ── Types ────────────────────────────────────────────────────────────

export interface RepoInfo {
  id: string;
  path: string;
  gh_profile: string | null;
}

export interface RepoDetail {
  id: string;
  path: string;
  gh_profile: string | null;
  display_name: string;
  default_branch: string;
}

export interface WorkspaceInfo {
  id: string;
  name: string;
  branch: string;
  worktree_path: string;
  repo_id: string;
  gh_profile: string | null;
  status: "running" | "waiting";
  created_at: number;
  task_title?: string | null;
  task_description?: string | null;
  source_todo_id?: string | null;
  custom_branch?: boolean;
}

export interface ToolUseInfo {
  name: string;
  input_preview?: string;
  file_path?: string;
  old_string?: string;
  new_string?: string;
}

export type AgentEvent =
  | { type: "assistant_message"; text: string; tool_uses: ToolUseInfo[]; thinking?: string }
  | { type: "usage"; input_tokens: number; output_tokens: number; cumulative: boolean }
  | { type: "done" }
  | { type: "error"; message: string };

export interface AgentStatusEvent {
  workspace_id: string;
  status: string;
}

// ── Repository ───────────────────────────────────────────────────────

export async function addRepo(path: string): Promise<RepoDetail> {
  return invoke<RepoDetail>("add_repo", { path });
}

export async function removeRepo(repoId: string): Promise<void> {
  return invoke("remove_repo", { repoId });
}

export async function listRepos(): Promise<RepoDetail[]> {
  return invoke<RepoDetail[]>("list_repos");
}

// ── GitHub Profiles ──────────────────────────────────────────────────

export interface GhProfile {
  login: string;
  active: boolean;
}

export async function listGhProfiles(): Promise<GhProfile[]> {
  return invoke<GhProfile[]>("list_gh_profiles");
}

export async function setRepoProfile(
  repoId: string,
  profile: string | null,
): Promise<void> {
  return invoke("set_repo_profile", { repoId, profile });
}

// ── GitHub Onboarding ─────────────────────────────────────────────────

export interface GhCliStatus {
  installed: boolean;
  authenticated: boolean;
  profiles: GhProfile[];
}

export async function checkGhCli(): Promise<GhCliStatus> {
  return invoke<GhCliStatus>("check_gh_cli");
}

export async function ghAuthLogin(): Promise<void> {
  return invoke("gh_auth_login");
}

export async function cancelGhAuthLogin(): Promise<void> {
  return invoke("cancel_gh_auth_login");
}

export interface GhRepoEntry {
  full_name: string;
  description: string;
  is_fork: boolean;
  clone_url: string;
  updated_at: string;
}

export async function listGhRepos(
  profile: string,
  search?: string,
): Promise<GhRepoEntry[]> {
  return invoke<GhRepoEntry[]>("list_gh_repos", {
    profile,
    search: search ?? null,
  });
}

export async function cloneRepo(
  cloneUrl: string,
  repoName: string,
  profile: string,
  destPath?: string,
): Promise<RepoDetail> {
  return invoke<RepoDetail>("clone_repo", {
    cloneUrl,
    repoName,
    destPath: destPath ?? null,
    profile,
  });
}

export interface CreateRepoOptions {
  name: string;
  private: boolean;
  description: string | null;
  add_readme: boolean;
}

export async function createGhRepo(
  options: CreateRepoOptions,
  profile: string,
): Promise<RepoDetail> {
  return invoke<RepoDetail>("create_gh_repo", { options, profile });
}

export async function checkRepoGhAccess(
  path: string,
  profiles: string[],
): Promise<string | null> {
  return invoke<string | null>("check_repo_gh_access", { path, profiles });
}

// ── Repo Branch ─────────────────────────────────────────────────────

export async function getRepoHead(repoId: string): Promise<string> {
  return invoke<string>("get_repo_head", { repoId });
}

export async function checkoutDefaultBranch(repoId: string): Promise<void> {
  return invoke("checkout_default_branch", { repoId });
}

// ── Workspace ────────────────────────────────────────────────────────

export async function createWorkspace(
  repoId: string,
  taskTitle?: string,
  taskDescription?: string,
  sourceTodoId?: string,
  customBranch?: string,
): Promise<WorkspaceInfo> {
  return invoke<WorkspaceInfo>("create_workspace", {
    repoId,
    taskTitle: taskTitle ?? null,
    taskDescription: taskDescription ?? null,
    sourceTodoId: sourceTodoId ?? null,
    customBranch: customBranch ?? null,
  });
}

export async function removeWorkspace(workspaceId: string): Promise<void> {
  return invoke("remove_workspace", { workspaceId });
}

export async function listWorkspaces(
  repoId: string,
): Promise<WorkspaceInfo[]> {
  return invoke<WorkspaceInfo[]>("list_workspaces", { repoId });
}

// ── Images ──────────────────────────────────────────────────────────

export async function saveImage(
  workspaceId: string,
  data: string,
  extension: string,
): Promise<string> {
  return invoke<string>("save_image", { workspaceId, data, extension });
}

// ── File Search ─────────────────────────────────────────────────

export interface FileSearchResult {
  path: string;
  name: string;
  kind: "file" | "folder";
  score: number;
}

export async function searchWorkspaceFiles(
  workspaceId: string,
  query: string,
): Promise<FileSearchResult[]> {
  return invoke<FileSearchResult[]>("search_workspace_files", {
    workspaceId,
    query,
  });
}

// ── Content Search (Grep) ───────────────────────────────────────

export interface GrepMatch {
  path: string;
  line_number: number;
  column: number;
  line_content: string;
}

export interface GrepResult {
  matches: GrepMatch[];
  truncated: boolean;
}

export async function grepWorkspace(
  workspaceId: string,
  pattern: string,
  isRegex: boolean = false,
  caseSensitive: boolean = false,
): Promise<GrepResult> {
  return invoke<GrepResult>("grep_workspace", {
    workspaceId,
    pattern,
    isRegex,
    caseSensitive,
  });
}

export async function readWorkspaceFile(
  workspaceId: string,
  filePath: string,
): Promise<string> {
  return invoke<string>("read_workspace_file", {
    workspaceId,
    filePath,
  });
}

// ── Repo-level File Search / Grep / Read ─────────────────────────────

export async function searchRepoFiles(
  repoId: string,
  query: string,
): Promise<FileSearchResult[]> {
  return invoke<FileSearchResult[]>("search_repo_files", {
    repoId,
    query,
  });
}

export async function grepRepo(
  repoId: string,
  pattern: string,
  isRegex: boolean = false,
  caseSensitive: boolean = false,
): Promise<GrepResult> {
  return invoke<GrepResult>("grep_repo", {
    repoId,
    pattern,
    isRegex,
    caseSensitive,
  });
}

export async function readRepoFile(
  repoId: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>("read_repo_file", { repoId, relativePath });
}

export async function listRepoDirectory(
  repoId: string,
  relativePath: string = "",
): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("list_repo_directory", { repoId, relativePath });
}

export async function writeRepoFile(
  repoId: string,
  relativePath: string,
  content: string,
): Promise<void> {
  return invoke("write_repo_file", { repoId, relativePath, content });
}

// ── Agent ────────────────────────────────────────────────────────────

export interface ModelOption {
  value: string;
  label: string;
}

const DEFAULT_MODELS: ModelOption[] = [{ value: "", label: "Default" }];
let cachedModels: ModelOption[] | null = null;

export async function listModels(): Promise<ModelOption[]> {
  if (cachedModels) return cachedModels;
  try {
    cachedModels = await invoke<ModelOption[]>("list_models");
    return cachedModels;
  } catch {
    return DEFAULT_MODELS;
  }
}

export function getCachedModels(): ModelOption[] {
  return cachedModels ?? DEFAULT_MODELS;
}

export function getModelLabel(value: string): string {
  const models = cachedModels ?? DEFAULT_MODELS;
  return models.find((m) => m.value === value)?.label ?? (value || "Default");
}

export async function sendMessage(
  workspaceId: string,
  prompt: string,
  onEvent: (event: AgentEvent) => void,
  planMode: boolean = false,
  thinkingMode: boolean = false,
  model: string = "",
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("send_message", { workspaceId, prompt, onEvent: channel, planMode, thinkingMode, model: model || null });
}

export async function stopAgent(workspaceId: string): Promise<void> {
  return invoke("stop_agent", { workspaceId });
}

// ── Branch ──────────────────────────────────────────────────────────

export async function renameBranch(
  workspaceId: string,
  newName: string,
): Promise<WorkspaceInfo> {
  return invoke<WorkspaceInfo>("rename_branch", { workspaceId, newName });
}

// ── File Browser ────────────────────────────────────────────────────

export interface FileEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
}

export async function listDirectory(
  workspaceId: string,
  relativePath: string = "",
): Promise<FileEntry[]> {
  return invoke<FileEntry[]>("list_directory", { workspaceId, relativePath });
}

export async function readFile(
  workspaceId: string,
  relativePath: string,
): Promise<string> {
  return invoke<string>("read_file", { workspaceId, relativePath });
}

export async function writeFile(
  workspaceId: string,
  relativePath: string,
  content: string,
): Promise<void> {
  return invoke("write_file", { workspaceId, relativePath, content });
}

// ── Git ─────────────────────────────────────────────────────────────

export interface ChangedFile {
  path: string;
  status: string;
  additions: number;
  deletions: number;
}

export async function getChangedFiles(
  workspaceId: string,
): Promise<ChangedFile[]> {
  return invoke<ChangedFile[]>("get_changed_files", { workspaceId });
}

export async function getDiff(
  workspaceId: string,
  filePath?: string,
): Promise<string> {
  return invoke<string>("get_diff", { workspaceId, filePath });
}

// ── Base Branch Updates ─────────────────────────────────────────────

export interface BaseUpdateStatus {
  behind_by: number;
}

export async function checkBaseUpdates(
  workspaceId: string,
): Promise<BaseUpdateStatus> {
  return invoke<BaseUpdateStatus>("check_base_updates", { workspaceId });
}

export async function updateFromBase(workspaceId: string): Promise<void> {
  return invoke("update_from_base", { workspaceId });
}

// ── Direct Git/GH Operations ────────────────────────────────────────

export interface CommitResult {
  hash: string;
  message: string;
}

export async function gitCommit(
  workspaceId: string,
  message: string,
): Promise<CommitResult> {
  return invoke<CommitResult>("git_commit", { workspaceId, message });
}

export async function gitPush(workspaceId: string): Promise<void> {
  return invoke("git_push", { workspaceId });
}

export async function checkMainBehind(repoId: string): Promise<number> {
  return invoke<number>("check_main_behind", { repoId });
}

export async function syncMain(repoId: string): Promise<void> {
  return invoke("sync_main", { repoId });
}

export async function ghPrMerge(
  workspaceId: string,
  prNumber: number,
): Promise<void> {
  return invoke("gh_pr_merge", { workspaceId, prNumber });
}

export async function generateCommitMessage(
  workspaceId: string,
): Promise<string> {
  return invoke<string>("generate_commit_message", { workspaceId });
}

// ── Terminal ─────────────────────────────────────────────────────────

export async function openTerminal(
  workspaceId: string,
  terminalId: string,
  onData: (data: number[]) => void,
): Promise<void> {
  const channel = new Channel<number[]>();
  channel.onmessage = onData;
  return invoke("open_terminal", { workspaceId, terminalId, onData: channel });
}

export async function writeTerminal(
  workspaceId: string,
  terminalId: string,
  data: number[],
): Promise<void> {
  return invoke("write_terminal", { workspaceId, terminalId, data });
}

export async function resizeTerminal(
  workspaceId: string,
  terminalId: string,
  rows: number,
  cols: number,
): Promise<void> {
  return invoke("resize_terminal", { workspaceId, terminalId, rows, cols });
}

export async function closeTerminal(
  workspaceId: string,
  terminalId: string,
): Promise<void> {
  return invoke("close_terminal", { workspaceId, terminalId });
}

// ── Repo-level Terminal ─────────────────────────────────────────────

export async function openRepoTerminal(
  repoId: string,
  terminalId: string,
  onData: (data: number[]) => void,
): Promise<void> {
  const channel = new Channel<number[]>();
  channel.onmessage = onData;
  return invoke("open_repo_terminal", { repoId, terminalId, onData: channel });
}

export async function writeRepoTerminal(
  repoId: string,
  terminalId: string,
  data: number[],
): Promise<void> {
  return invoke("write_repo_terminal", { repoId, terminalId, data });
}

export async function resizeRepoTerminal(
  repoId: string,
  terminalId: string,
  rows: number,
  cols: number,
): Promise<void> {
  return invoke("resize_repo_terminal", { repoId, terminalId, rows, cols });
}

export async function closeRepoTerminal(
  repoId: string,
  terminalId: string,
): Promise<void> {
  return invoke("close_repo_terminal", { repoId, terminalId });
}

// ── Messages ────────────────────────────────────────────────────────

export async function saveMessages(
  workspaceId: string,
  messages: unknown[],
): Promise<void> {
  return invoke("save_messages", { workspaceId, messages });
}

export async function loadMessages(
  workspaceId: string,
): Promise<unknown[]> {
  return invoke<unknown[]>("load_messages", { workspaceId });
}

// ── Todos ────────────────────────────────────────────────────────────

export async function saveTodos(
  repoId: string,
  todos: unknown[],
): Promise<void> {
  return invoke("save_todos", { repoId, todos });
}

export async function loadTodos(
  repoId: string,
): Promise<unknown[]> {
  return invoke<unknown[]>("load_todos", { repoId });
}

// ── PR Status ────────────────────────────────────────────────────────

export interface PrStatus {
  state: "none" | "open" | "merged" | "closed";
  url: string;
  number: number;
  title: string;
  checks: "pending" | "passing" | "failing" | "none";
  mergeable: "mergeable" | "conflicting" | "unknown";
  additions: number;
  deletions: number;
  ahead_by: number;
}

export async function getPrStatus(workspaceId: string): Promise<PrStatus> {
  return invoke<PrStatus>("get_pr_status", { workspaceId });
}

export async function getPrTemplate(repoId: string): Promise<string> {
  return invoke<string>("get_pr_template", { repoId });
}

// ── Repo Settings ────────────────────────────────────────────────────

export interface NamedScript {
  name: string;
  command: string;
}

export interface LspServerConfig {
  command: string;
  args: string[];
  extensions: string[];
  detect_files: string[];
  language_id: string;
  install_hint: string;
  project_roots: string[];
}

export interface RepoSettings {
  setup_script: string;
  run_scripts: NamedScript[];
  remove_script: string;
  pr_message: string;
  review_message: string;
  default_thinking: boolean;
  default_plan: boolean;
  system_prompt: string;
  lsp_servers: Record<string, LspServerConfig>;
}

export async function getRepoSettings(repoId: string): Promise<RepoSettings> {
  return invoke<RepoSettings>("get_repo_settings", { repoId });
}

export async function saveRepoSettings(
  repoId: string,
  settings: RepoSettings,
): Promise<void> {
  return invoke("save_repo_settings", { repoId, settings });
}

// ── LSP ─────────────────────────────────────────────────────────────

/** Start LSP servers for a workspace in the background. Returns immediately. */
export async function lspStartServer(workspaceId: string): Promise<void> {
  return invoke("lsp_start_server", { workspaceId });
}

/** Stop a single LSP server by repo + server ID. */
export async function lspStopServer(repoId: string, serverId: string): Promise<void> {
  return invoke("lsp_stop_server", { repoId, serverId });
}

/** Stop and restart a single LSP server. Needs workspaceId to resolve worktree path. */
export async function lspRestartServer(repoId: string, serverId: string, workspaceId: string): Promise<void> {
  return invoke("lsp_restart_server", { repoId, serverId, workspaceId });
}

/** Query current LSP server status (for populating status bar on mount). */
export async function lspGetStatus(): Promise<{ repo_id: string; server_id: string; status: string }[]> {
  return invoke("lsp_get_status");
}

export interface LspLocation {
  file_path: string;
  line: number;
  character: number;
}

export interface LspDiagnostic {
  line: number;
  character: number;
  end_line: number;
  end_character: number;
  severity: string;
  message: string;
  source: string;
}

export interface LspHoverResult {
  kind: "markdown" | "plaintext";
  text: string;
}

export async function lspHover(
  workspaceId: string,
  filePath: string,
  line: number,
  character: number,
): Promise<LspHoverResult | null> {
  return invoke<LspHoverResult | null>("lsp_hover", { workspaceId, filePath, line, character });
}

export async function lspGotoDefinition(
  workspaceId: string,
  filePath: string,
  line: number,
  character: number,
): Promise<LspLocation | null> {
  return invoke<LspLocation | null>("lsp_goto_definition", { workspaceId, filePath, line, character });
}

export async function lspDiagnostics(
  workspaceId: string,
  filePath: string,
): Promise<LspDiagnostic[]> {
  return invoke<LspDiagnostic[]>("lsp_diagnostics", { workspaceId, filePath });
}

export interface LspRenameResult {
  files_changed: number;
  edits_applied: number;
  details: { file_path: string; edit_count: number }[];
}

export async function lspRename(
  workspaceId: string,
  filePath: string,
  line: number,
  character: number,
  newName: string,
): Promise<LspRenameResult> {
  return invoke<LspRenameResult>("lsp_rename", { workspaceId, filePath, line, character, newName });
}

// ── Script Runner ───────────────────────────────────────────────────

export type ScriptEvent =
  | { type: "output"; data: string }
  | { type: "exit"; code: number | null };

export async function runScript(
  workspaceId: string,
  command: string,
  onEvent: (event: ScriptEvent) => void,
): Promise<void> {
  const channel = new Channel<ScriptEvent>();
  channel.onmessage = onEvent;
  return invoke("run_script", { workspaceId, command, onEvent: channel });
}

export async function stopScript(workspaceId: string): Promise<void> {
  return invoke("stop_script", { workspaceId });
}

export async function runRepoScript(
  repoId: string,
  command: string,
  onEvent: (event: ScriptEvent) => void,
): Promise<void> {
  const channel = new Channel<ScriptEvent>();
  channel.onmessage = onEvent;
  return invoke("run_repo_script", { repoId, command, onEvent: channel });
}

export async function stopRepoScript(repoId: string): Promise<void> {
  return invoke("stop_repo_script", { repoId });
}

// ── Events ───────────────────────────────────────────────────────────

export function onAgentStatus(
  callback: (event: AgentStatusEvent) => void,
): Promise<UnlistenFn> {
  return listen<AgentStatusEvent>("agent-status", (e) => callback(e.payload));
}

export function onWorkspaceUpdated(
  callback: (ws: WorkspaceInfo) => void,
): Promise<UnlistenFn> {
  return listen<WorkspaceInfo>("workspace-updated", (e) => callback(e.payload));
}

// ── Suggested replies ────────────────────────────────────────────────

export async function suggestReplies(text: string): Promise<string[]> {
  return invoke<string[]>("suggest_replies", { text });
}

// ── Staging ─────────────────────────────────────────────────────────

export interface StagingResult {
  workspace: WorkspaceInfo;
  merged_branches: string[];
  conflicting_branches: string[];
}

export async function createStagingWorkspace(
  repoId: string,
  branchNames: string[],
): Promise<StagingResult> {
  return invoke<StagingResult>("create_staging_workspace", { repoId, branchNames });
}

export async function removeStagingWorkspace(repoId: string): Promise<void> {
  return invoke("remove_staging_workspace", { repoId });
}

// ── System Resources ────────────────────────────────────────────────

export interface SystemResources {
  cpu_cores: number;
  memory_gb: number;
  available_memory_gb: number;
}

export async function getSystemResources(): Promise<SystemResources> {
  return invoke<SystemResources>("get_system_resources");
}

// ── Autopilot ───────────────────────────────────────────────────────

export async function prioritizeTodos(todoJson: string): Promise<string[]> {
  return invoke<string[]>("prioritize_todos", { todoJson });
}

export interface AutopilotAction {
  response: string;
  action_type: string;
  todo_ids: string[];
  reorder: string[];
}

export async function determineDependencies(todoJson: string): Promise<Record<string, string[]>> {
  const raw = await invoke<string>("determine_dependencies", { todoJson });
  return JSON.parse(raw);
}

export async function interpretAutopilotCommand(
  command: string,
  contextJson: string,
): Promise<AutopilotAction> {
  return invoke<AutopilotAction>("interpret_autopilot_command", { command, contextJson });
}

// ── Knowledge Base (Warm Context) ────────────────────────────────────

export type ContextBuildStatus = "not_built" | "building" | "built" | "failed";

export interface ContextMeta {
  include_globs: string[];
  exclude_globs: string[];
  build_status: ContextBuildStatus;
  last_built_at: number | null;
  invariant_count: number;
  fact_count: number;
  context_entry_count: number;
  contradiction_count: number;
  precheck_model: string;
  built_at_commit: string | null;
}

export interface InvariantViolation {
  invariant_id: string;
  file: string;
  line: number;
  description: string;
}

export interface InvariantCheckResult {
  passed: boolean;
  violations: InvariantViolation[];
}

export async function regenerateHot(repoId: string): Promise<void> {
  return invoke("regenerate_hot", { repoId });
}

export async function getContextMeta(repoId: string): Promise<ContextMeta> {
  return invoke<ContextMeta>("get_context_meta", { repoId });
}

export async function saveContextScope(
  repoId: string,
  includeGlobs: string[],
  excludeGlobs: string[],
  precheckModel: string,
): Promise<void> {
  return invoke("save_context_scope", { repoId, includeGlobs, excludeGlobs, precheckModel });
}

export async function buildKnowledgeBase(
  repoId: string,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("build_knowledge_base", { repoId, onEvent: channel });
}

export async function stopContextBuild(repoId: string): Promise<void> {
  return invoke("stop_context_build", { repoId });
}

export async function checkInvariants(workspaceId: string): Promise<InvariantCheckResult> {
  return invoke<InvariantCheckResult>("check_invariants", { workspaceId });
}

export async function updateContextAfterMerge(
  repoId: string,
  workspaceId: string,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("update_context_after_merge", { repoId, workspaceId, onEvent: channel });
}

export async function updateKnowledgeBaseIncremental(
  repoId: string,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("update_knowledge_base_incremental", { repoId, onEvent: channel });
}

export async function readContextFile(
  repoId: string,
  filename: string,
): Promise<string> {
  return invoke<string>("read_context_file", { repoId, filename });
}

export async function writeContextFile(
  repoId: string,
  filename: string,
  content: string,
): Promise<void> {
  return invoke("write_context_file", { repoId, filename, content });
}

export async function draftContradictionResolution(
  repoId: string,
  contradictionId: string,
  resolution: "exception" | "update_invariant" | "tech_debt",
  userNote: string,
): Promise<string> {
  return invoke<string>("draft_contradiction_resolution", {
    repoId,
    contradictionId,
    resolution,
    userNote,
  });
}

export async function resolveContradiction(
  repoId: string,
  contradictionId: string,
  resolution: "exception" | "update_invariant" | "tech_debt",
  invariantText?: string,
): Promise<void> {
  return invoke("resolve_contradiction", {
    repoId,
    contradictionId,
    resolution,
    invariantText: invariantText ?? null,
  });
}

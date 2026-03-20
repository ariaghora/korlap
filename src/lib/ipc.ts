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
): Promise<WorkspaceInfo> {
  return invoke<WorkspaceInfo>("create_workspace", {
    repoId,
    taskTitle: taskTitle ?? null,
    taskDescription: taskDescription ?? null,
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

// ── Agent ────────────────────────────────────────────────────────────

export async function sendMessage(
  workspaceId: string,
  prompt: string,
  onEvent: (event: AgentEvent) => void,
  planMode: boolean = false,
  thinkingMode: boolean = false,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("send_message", { workspaceId, prompt, onEvent: channel, planMode, thinkingMode });
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
  onData: (data: number[]) => void,
): Promise<void> {
  const channel = new Channel<number[]>();
  channel.onmessage = onData;
  return invoke("open_terminal", { workspaceId, onData: channel });
}

export async function writeTerminal(
  workspaceId: string,
  data: number[],
): Promise<void> {
  return invoke("write_terminal", { workspaceId, data });
}

export async function resizeTerminal(
  workspaceId: string,
  rows: number,
  cols: number,
): Promise<void> {
  return invoke("resize_terminal", { workspaceId, rows, cols });
}

export async function closeTerminal(workspaceId: string): Promise<void> {
  return invoke("close_terminal", { workspaceId });
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

export interface RepoSettings {
  setup_script: string;
  run_script: string;
  remove_script: string;
  pr_message: string;
  review_message: string;
  default_thinking: boolean;
  default_plan: boolean;
  system_prompt: string;
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

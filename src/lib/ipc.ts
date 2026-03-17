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
  status: "running" | "waiting" | "archived";
  created_at: number;
}

export interface ToolUseInfo {
  name: string;
  input_preview?: string;
  file_path?: string;
}

export type AgentEvent =
  | { type: "assistant_message"; text: string; tool_uses: ToolUseInfo[] }
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

// ── Workspace ────────────────────────────────────────────────────────

export async function createWorkspace(
  repoId: string,
): Promise<WorkspaceInfo> {
  return invoke<WorkspaceInfo>("create_workspace", { repoId });
}

export async function archiveWorkspace(workspaceId: string): Promise<void> {
  return invoke("archive_workspace", { workspaceId });
}

export async function listWorkspaces(
  repoId: string,
): Promise<WorkspaceInfo[]> {
  return invoke<WorkspaceInfo[]>("list_workspaces", { repoId });
}

// ── Agent ────────────────────────────────────────────────────────────

export async function sendMessage(
  workspaceId: string,
  prompt: string,
  onEvent: (event: AgentEvent) => void,
): Promise<void> {
  const channel = new Channel<AgentEvent>();
  channel.onmessage = onEvent;
  return invoke("send_message", { workspaceId, prompt, onEvent: channel });
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

// ── Scripts ─────────────────────────────────────────────────────────

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

// ── Events ───────────────────────────────────────────────────────────

export function onAgentStatus(
  callback: (event: AgentStatusEvent) => void,
): Promise<UnlistenFn> {
  return listen<AgentStatusEvent>("agent-status", (e) => callback(e.payload));
}

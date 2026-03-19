import { saveMessages, loadMessages } from "$lib/ipc";
import { SvelteMap } from "svelte/reactivity";

// ── Types ──────────────────────────────────────────────────────────

export type MessageChunk =
  | { type: "text"; content: string }
  | { type: "thinking"; content: string }
  | { type: "tool"; name: string; input: string; filePath?: string; oldString?: string; newString?: string };

export interface MessageMention {
  type: "file" | "folder";
  path: string;
  displayName: string;
}

export interface Message {
  id: string;
  role: "user" | "assistant" | "action";
  chunks: MessageChunk[];
  done: boolean;
  actionLabel?: string;
  imageDataUrls?: string[];
  mentions?: MessageMention[];
  planMode?: boolean;
  hidden?: boolean;
}

// ── State ──────────────────────────────────────────────────────────
// Keyed by workspaceId → Map<messageId, Message>
// Updating one message = one reactive cell, not the whole list.

export const messagesByWorkspace = new SvelteMap<string, SvelteMap<string, Message>>();
export const sendingByWorkspace = new SvelteMap<string, boolean>();

export function setSending(wsId: string, value: boolean) {
  sendingByWorkspace.set(wsId, value);
}

export function isSending(wsId: string): boolean {
  return sendingByWorkspace.get(wsId) ?? false;
}

// ── Helpers ────────────────────────────────────────────────────────

function ensureMap(workspaceId: string): SvelteMap<string, Message> {
  let map = messagesByWorkspace.get(workspaceId);
  if (!map) {
    map = new SvelteMap();
    messagesByWorkspace.set(workspaceId, map);
  }
  return map;
}

/** Add a message to the workspace's map. */
function pushMessage(workspaceId: string, msg: Message) {
  const map = ensureMap(workspaceId);
  map.set(msg.id, msg);
}

/** Add a complete user message */
export function addUserMessage(
  workspaceId: string,
  id: string,
  text: string,
  imageDataUrls?: string[],
  mentions?: MessageMention[],
  planMode?: boolean,
  hidden?: boolean,
) {
  pushMessage(workspaceId, {
    id,
    role: "user",
    chunks: text ? [{ type: "text", content: text }] : [],
    done: true,
    imageDataUrls: imageDataUrls && imageDataUrls.length > 0 ? imageDataUrls : undefined,
    mentions: mentions && mentions.length > 0 ? mentions : undefined,
    planMode: planMode || undefined,
    hidden: hidden || undefined,
  });
  persistMessages(workspaceId);
}

/** Add a system action message (displayed as compact indicator, not full bubble) */
export function addActionMessage(
  workspaceId: string,
  id: string,
  label: string,
) {
  pushMessage(workspaceId, {
    id,
    role: "action",
    chunks: [],
    done: true,
    actionLabel: label,
  });
  persistMessages(workspaceId);
}

/** Add a complete assistant message (from stream-json "assistant" event) */
export function addAssistantMessage(
  workspaceId: string,
  id: string,
  text: string,
  toolUses: { name: string; input: string; filePath?: string; oldString?: string; newString?: string }[],
  thinking?: string,
) {
  const chunks: MessageChunk[] = [];
  if (thinking) {
    chunks.push({ type: "thinking", content: thinking });
  }
  if (text) {
    chunks.push({ type: "text", content: text });
  }
  for (const tool of toolUses) {
    chunks.push({ type: "tool", name: tool.name, input: tool.input, filePath: tool.filePath, oldString: tool.oldString, newString: tool.newString });
  }
  pushMessage(workspaceId, {
    id,
    role: "assistant",
    chunks,
    done: true,
  });
  persistMessages(workspaceId);
}

/** Get messages for a workspace as an ordered array. */
export function getMessages(workspaceId: string): Message[] {
  const map = messagesByWorkspace.get(workspaceId);
  return map ? [...map.values()] : [];
}

/** Load persisted messages from disk */
export async function loadPersistedMessages(
  workspaceId: string,
): Promise<void> {
  try {
    const raw = (await loadMessages(workspaceId)) as Message[];
    if (raw.length === 0) return;
    const map = ensureMap(workspaceId);
    for (const msg of raw) {
      map.set(msg.id, msg);
    }
  } catch {
    // No saved messages
  }
}

// Debounced persistence — don't write on every single message
let pendingSaves = new Map<string, ReturnType<typeof setTimeout>>();

function persistMessages(workspaceId: string) {
  const existing = pendingSaves.get(workspaceId);
  if (existing) clearTimeout(existing);

  pendingSaves.set(
    workspaceId,
    setTimeout(() => {
      pendingSaves.delete(workspaceId);
      const map = messagesByWorkspace.get(workspaceId);
      if (!map) return;
      saveMessages(workspaceId, [...map.values()]).catch(() => {});
    }, 500),
  );
}

/** Force immediate persist (e.g., before app close) */
export function flushPersist(workspaceId: string) {
  const existing = pendingSaves.get(workspaceId);
  if (existing) {
    clearTimeout(existing);
    pendingSaves.delete(workspaceId);
  }
  const map = messagesByWorkspace.get(workspaceId);
  if (!map) return;
  saveMessages(workspaceId, [...map.values()]).catch(() => {});
}

/** Remove all in-memory state for a workspace (call on remove) */
export function clearWorkspaceData(workspaceId: string) {
  const pending = pendingSaves.get(workspaceId);
  if (pending) clearTimeout(pending);
  pendingSaves.delete(workspaceId);
  messagesByWorkspace.delete(workspaceId);
}

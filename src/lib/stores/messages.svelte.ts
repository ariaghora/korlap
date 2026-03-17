import { saveMessages, loadMessages } from "$lib/ipc";

// ── Types ──────────────────────────────────────────────────────────

export type MessageChunk =
  | { type: "text"; content: string }
  | { type: "tool"; name: string; input: string; filePath?: string };

export interface Message {
  id: string;
  role: "user" | "assistant" | "action";
  chunks: MessageChunk[];
  done: boolean;
  actionLabel?: string; // compact label for action messages (e.g. "Creating PR", "Merging")
}

// ── State ──────────────────────────────────────────────────────────
// Keyed by workspaceId → Map<messageId, Message>
// Updating one message = one reactive cell, not the whole list.

export const messagesByWorkspace = $state(
  new Map<string, Map<string, Message>>(),
);

// ── Helpers ────────────────────────────────────────────────────────

function ensureWorkspace(workspaceId: string): Map<string, Message> {
  let msgs = messagesByWorkspace.get(workspaceId);
  if (!msgs) {
    msgs = new Map<string, Message>();
    messagesByWorkspace.set(workspaceId, msgs);
  }
  return msgs;
}

/** Notify Svelte that messages changed — only bumps the affected workspace's counter */
function notifyChange(workspaceId: string, msgs: Map<string, Message>) {
  messagesByWorkspace.set(workspaceId, msgs);
  _versions.set(workspaceId, (_versions.get(workspaceId) ?? 0) + 1);
}

/** Add a complete user message */
export function addUserMessage(
  workspaceId: string,
  id: string,
  text: string,
) {
  const msgs = ensureWorkspace(workspaceId);
  msgs.set(id, {
    id,
    role: "user",
    chunks: [{ type: "text", content: text }],
    done: true,
  });
  notifyChange(workspaceId, msgs);
  persistMessages(workspaceId);
}

/** Add a system action message (displayed as compact indicator, not full bubble) */
export function addActionMessage(
  workspaceId: string,
  id: string,
  label: string,
) {
  const msgs = ensureWorkspace(workspaceId);
  msgs.set(id, {
    id,
    role: "action",
    chunks: [],
    done: true,
    actionLabel: label,
  });
  notifyChange(workspaceId, msgs);
  persistMessages(workspaceId);
}

/** Add a complete assistant message (from stream-json "assistant" event) */
export function addAssistantMessage(
  workspaceId: string,
  id: string,
  text: string,
  toolUses: { name: string; input: string; filePath?: string }[],
) {
  const msgs = ensureWorkspace(workspaceId);
  const chunks: MessageChunk[] = [];
  if (text) {
    chunks.push({ type: "text", content: text });
  }
  for (const tool of toolUses) {
    chunks.push({ type: "tool", name: tool.name, input: tool.input, filePath: tool.filePath });
  }
  msgs.set(id, {
    id,
    role: "assistant",
    chunks,
    done: true,
  });
  notifyChange(workspaceId, msgs);
  persistMessages(workspaceId);
}

/** Per-workspace reactive counters — only the affected workspace re-evaluates */
const _versions = $state(new Map<string, number>());

// Memoization cache (plain JS, not reactive).
// Returns the same array reference when content hasn't changed,
// so downstream components see no prop change and skip re-rendering.
const _msgCache = new Map<string, { version: number; array: Message[] }>();
const EMPTY_MESSAGES: Message[] = [];

/** Get ordered messages for a workspace */
export function getMessages(workspaceId: string): Message[] {
  // Read this workspace's version counter to create a scoped reactive dependency.
  const version = _versions.get(workspaceId) ?? 0;
  const cached = _msgCache.get(workspaceId);
  if (cached && cached.version === version) return cached.array;
  const msgs = messagesByWorkspace.get(workspaceId);
  if (!msgs || msgs.size === 0) return EMPTY_MESSAGES;
  const array = [...msgs.values()];
  _msgCache.set(workspaceId, { version, array });
  return array;
}

/** Load persisted messages from disk */
export async function loadPersistedMessages(
  workspaceId: string,
): Promise<void> {
  try {
    const raw = (await loadMessages(workspaceId)) as Message[];
    if (raw.length === 0) return;
    const msgs = ensureWorkspace(workspaceId);
    for (const msg of raw) {
      msgs.set(msg.id, msg);
    }
    notifyChange(workspaceId, msgs);
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
      const msgs = messagesByWorkspace.get(workspaceId);
      if (!msgs) return;
      const arr = [...msgs.values()];
      saveMessages(workspaceId, arr).catch(() => {});
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
  const msgs = messagesByWorkspace.get(workspaceId);
  if (!msgs) return;
  const arr = [...msgs.values()];
  saveMessages(workspaceId, arr).catch(() => {});
}

/** Remove all in-memory state for a workspace (call on archive) */
export function clearWorkspaceData(workspaceId: string) {
  const pending = pendingSaves.get(workspaceId);
  if (pending) clearTimeout(pending);
  pendingSaves.delete(workspaceId);
  messagesByWorkspace.delete(workspaceId);
  _versions.delete(workspaceId);
  _msgCache.delete(workspaceId);
}

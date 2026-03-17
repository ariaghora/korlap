import { saveMessages, loadMessages } from "$lib/ipc";

// ── Types ──────────────────────────────────────────────────────────

export type MessageChunk =
  | { type: "text"; content: string }
  | { type: "tool"; name: string; input: string };

export interface Message {
  id: string;
  role: "user" | "assistant";
  chunks: MessageChunk[];
  done: boolean;
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

/** Notify Svelte that messages changed */
function notifyChange(workspaceId: string, msgs: Map<string, Message>) {
  messagesByWorkspace.set(workspaceId, msgs);
  _version.count++;
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

/** Add a complete assistant message (from stream-json "assistant" event) */
export function addAssistantMessage(
  workspaceId: string,
  id: string,
  text: string,
  toolUses: { name: string; input: string }[],
) {
  const msgs = ensureWorkspace(workspaceId);
  const chunks: MessageChunk[] = [];
  if (text) {
    chunks.push({ type: "text", content: text });
  }
  for (const tool of toolUses) {
    chunks.push({ type: "tool", name: tool.name, input: tool.input });
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

/** Reactive counter — used to force re-evaluation in components */
const _version = $state({ count: 0 });

/** Get ordered messages for a workspace */
export function getMessages(workspaceId: string): Message[] {
  // Read version to create a reactive dependency
  void _version.count;
  const msgs = messagesByWorkspace.get(workspaceId);
  if (!msgs) return [];
  return [...msgs.values()];
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

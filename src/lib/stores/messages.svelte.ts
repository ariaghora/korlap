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
  actionLabel?: string; // compact label for action messages (e.g. "Creating PR", "Merging")
  imageDataUrls?: string[]; // data URLs for attached image thumbnails (user messages only)
  mentions?: MessageMention[]; // @-mentioned files/folders (user messages only)
  planMode?: boolean; // true if this message was sent in plan mode
}

// ── State ──────────────────────────────────────────────────────────
// Keyed by workspaceId → Message[]
// Each mutation replaces the array reference so Svelte's Map proxy sees a real change.

export const messagesByWorkspace = new SvelteMap<string, Message[]>();
export const sendingByWorkspace = new SvelteMap<string, boolean>();

export function setSending(wsId: string, value: boolean) {
  sendingByWorkspace.set(wsId, value);
}

export function isSending(wsId: string): boolean {
  return sendingByWorkspace.get(wsId) ?? false;
}

// Internal lookup maps for O(1) dedup/update (plain JS, not reactive).
const _lookups = new Map<string, Map<string, number>>();

// ── Helpers ────────────────────────────────────────────────────────

function ensureLookup(workspaceId: string): Map<string, number> {
  let lookup = _lookups.get(workspaceId);
  if (!lookup) {
    lookup = new Map();
    _lookups.set(workspaceId, lookup);
  }
  return lookup;
}

/** Push a message and replace the array reference so the $state proxy signals a change. */
function pushMessage(workspaceId: string, msg: Message) {
  const arr = messagesByWorkspace.get(workspaceId) ?? [];
  const lookup = ensureLookup(workspaceId);
  const newArr = [...arr, msg];
  lookup.set(msg.id, newArr.length - 1);
  messagesByWorkspace.set(workspaceId, newArr);
}

/** Add a complete user message */
export function addUserMessage(
  workspaceId: string,
  id: string,
  text: string,
  imageDataUrls?: string[],
  mentions?: MessageMention[],
  planMode?: boolean,
) {
  pushMessage(workspaceId, {
    id,
    role: "user",
    chunks: text ? [{ type: "text", content: text }] : [],
    done: true,
    imageDataUrls: imageDataUrls && imageDataUrls.length > 0 ? imageDataUrls : undefined,
    mentions: mentions && mentions.length > 0 ? mentions : undefined,
    planMode: planMode || undefined,
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

const EMPTY_MESSAGES: Message[] = [];

/** Get messages for a workspace — reads directly from the $state Map. */
export function getMessages(workspaceId: string): Message[] {
  return messagesByWorkspace.get(workspaceId) ?? EMPTY_MESSAGES;
}

/** Load persisted messages from disk */
export async function loadPersistedMessages(
  workspaceId: string,
): Promise<void> {
  try {
    const raw = (await loadMessages(workspaceId)) as Message[];
    if (raw.length === 0) return;
    const lookup = ensureLookup(workspaceId);
    for (const msg of raw) {
      lookup.set(msg.id, lookup.size);
    }
    messagesByWorkspace.set(workspaceId, raw);
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
      saveMessages(workspaceId, msgs).catch(() => {});
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
  saveMessages(workspaceId, msgs).catch(() => {});
}

/** Remove all in-memory state for a workspace (call on archive) */
export function clearWorkspaceData(workspaceId: string) {
  const pending = pendingSaves.get(workspaceId);
  if (pending) clearTimeout(pending);
  pendingSaves.delete(workspaceId);
  messagesByWorkspace.delete(workspaceId);
  _lookups.delete(workspaceId);
}

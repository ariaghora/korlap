import type { Message, MessageChunk, MessageMention } from "$lib/stores/messages.svelte.js";
import {
  FileText, Pencil, FilePlus, Terminal, FolderSearch, TextSearch,
  Bot, Globe, Zap, Settings, MessageCircleQuestion, ListChecks,
} from "lucide-svelte";

// ── Tool icon mapping ────────────────────────────────────────────────

export const toolIcons: Record<string, typeof Settings> = {
  // Claude tools
  Read: FileText,
  Edit: Pencil,
  Write: FilePlus,
  Bash: Terminal,
  Glob: FolderSearch,
  Grep: TextSearch,
  Agent: Bot,
  WebFetch: Globe,
  WebSearch: Globe,
  Skill: Zap,
  ToolSearch: Settings,
  AskUserQuestion: MessageCircleQuestion,
  TodoWrite: ListChecks,
  // Codex tools
  shell: Terminal,
  file_read: FileText,
  file_write: FilePlus,
  file_edit: Pencil,
};

// ── Shared types ─────────────────────────────────────────────────────

export interface PastedImage {
  id: string;
  dataUrl: string;    // for thumbnail preview
  base64: string;     // raw base64 data (no prefix)
  extension: string;  // png, jpg, etc.
}

export interface ChatPanelApi {
  addMention: (mention: { type: "file" | "folder"; path: string; displayName: string; lineNumber?: number }) => void;
  /** Insert plain text into the chat input (e.g. a quoted diff block). */
  insertText: (text: string) => void;
  /** Call when the agent finishes to trigger AI-powered suggestion generation. */
  refreshSuggestions: () => void;
}

export interface QueueDisplayItem {
  id: string;
  prompt: string;
  imageCount: number;
  mentionCount: number;
  planMode: boolean;
}

// ── Text with mentions ───────────────────────────────────────────────

export type TextSegment =
  | { kind: "text"; value: string }
  | { kind: "mention"; mention: MessageMention };

export function splitTextWithMentions(text: string, mentions: MessageMention[]): TextSegment[] {
  if (mentions.length === 0) return [{ kind: "text", value: text }];

  // Build regex matching any @displayName, longest first to avoid partial matches
  const sorted = [...mentions].sort((a, b) => b.displayName.length - a.displayName.length);
  const escaped = sorted.map((m) => `@${m.displayName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`);
  const regex = new RegExp(`(${escaped.join("|")})`, "g");

  const segments: TextSegment[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ kind: "text", value: text.slice(lastIndex, match.index) });
    }
    const displayName = match[0].slice(1); // strip @
    const mention = mentions.find((m) => m.displayName === displayName);
    if (mention) {
      segments.push({ kind: "mention", mention });
    } else {
      segments.push({ kind: "text", value: match[0] });
    }
    lastIndex = regex.lastIndex;
  }
  if (lastIndex < text.length) {
    segments.push({ kind: "text", value: text.slice(lastIndex) });
  }
  return segments;
}

// ── Visual blocks (message grouping for rendering) ───────────────────

export type ToolEntry = { chunk: MessageChunk & { type: "tool" }; msgId: string; ci: number };
export type VisualBlock =
  | { kind: "user"; msg: Message; key: string }
  | { kind: "action"; msg: Message; key: string }
  | { kind: "assistant-label"; key: string }
  | { kind: "thinking"; chunk: MessageChunk & { type: "thinking" }; key: string }
  | { kind: "text"; chunk: MessageChunk & { type: "text" }; msgId: string; key: string }
  | { kind: "special-tool"; chunk: MessageChunk & { type: "tool" }; msgId: string; ci: number; key: string }
  | { kind: "todo-list"; chunk: MessageChunk & { type: "tool" }; msgId: string; ci: number; isLatest: boolean; key: string }
  | { kind: "tool-group"; tools: ToolEntry[]; key: string };

export function buildVisualBlocks(msgs: Message[]): VisualBlock[] {
  const blocks: VisualBlock[] = [];
  let pendingTools: ToolEntry[] = [];
  let lastRole: string | null = null;

  function flushTools() {
    if (pendingTools.length > 0) {
      blocks.push({ kind: "tool-group", tools: pendingTools, key: `tg:${pendingTools[0].msgId}:${pendingTools[0].ci}` });
      pendingTools = [];
    }
  }

  for (const msg of msgs) {
    if (msg.hidden) continue;

    if (msg.role === "user" || msg.role === "action") {
      flushTools();
      blocks.push({
        kind: msg.role as "user" | "action",
        msg,
        key: msg.id,
      });
      lastRole = msg.role;
      continue;
    }

    // assistant message
    if (lastRole !== "assistant") {
      flushTools();
      blocks.push({ kind: "assistant-label", key: `label:${msg.id}` });
    }
    lastRole = "assistant";

    for (let ci = 0; ci < msg.chunks.length; ci++) {
      const chunk = msg.chunks[ci];
      if (chunk.type === "thinking") {
        flushTools();
        blocks.push({ kind: "thinking", chunk, key: `think:${msg.id}` });
      } else if (chunk.type === "text") {
        flushTools();
        blocks.push({ kind: "text", chunk, msgId: msg.id, key: `text:${msg.id}` });
      } else if (chunk.type === "tool") {
        if (chunk.name === "TodoWrite") {
          flushTools();
          blocks.push({ kind: "todo-list", chunk, msgId: msg.id, ci, isLatest: false, key: `todo:${msg.id}:${ci}` });
        } else {
          const isSpecial = chunk.name === "AskUserQuestion" ||
                            (chunk.oldString != null && chunk.newString != null);
          if (isSpecial) {
            flushTools();
            blocks.push({ kind: "special-tool", chunk, msgId: msg.id, ci, key: `st:${msg.id}:${ci}` });
          } else {
            pendingTools.push({ chunk, msgId: msg.id, ci });
          }
        }
      }
    }
  }
  flushTools();

  // Mark the last TodoWrite block as the latest (it reflects current state)
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].kind === "todo-list") {
      (blocks[i] as Extract<VisualBlock, { kind: "todo-list" }>).isLatest = true;
      break;
    }
  }

  return blocks;
}

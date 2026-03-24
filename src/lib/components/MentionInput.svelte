<script lang="ts">
  export interface Mention {
    type: "file" | "folder";
    path: string;
    displayName: string;
    lineNumber?: number;
  }

  export interface MentionInputValue {
    text: string;
    mentions: Mention[];
  }

  export interface MentionInputApi {
    insertMention: (mention: Mention) => void;
    appendMention: (mention: Mention) => void;
    insertText: (text: string) => void;
    restoreContent: (text: string, mentions: Mention[]) => void;
    focus: () => void;
    submit: () => void;
    getValue: () => MentionInputValue;
  }

  interface Props {
    placeholder?: string;
    disabled?: boolean;
    multiline?: boolean;
    onSubmit: (value: MentionInputValue) => void;
    onQueryChange: (query: string | null) => void;
    onPaste?: (e: ClipboardEvent) => void;
    ref?: MentionInputApi | undefined;
  }

  let {
    placeholder = "Ask to make changes, @mention files, run /commands",
    disabled = false,
    multiline = false,
    onSubmit,
    onQueryChange,
    onPaste,
    ref = $bindable(undefined),
  }: Props = $props();

  let editorEl: HTMLDivElement | undefined = $state();

  // Track the Range where the current @ trigger starts (the @ character itself)
  let atTriggerRange: Range | null = $state(null);

  function focus() {
    editorEl?.focus();
  }

  function createMentionChip(mention: Mention): HTMLSpanElement {
    const chip = document.createElement("span");
    chip.className = "mention-chip";
    chip.contentEditable = "false";
    chip.dataset.mentionType = mention.type;
    chip.dataset.mentionPath = mention.path;

    const icon = document.createElement("span");
    icon.className = "mention-chip-icon";
    icon.textContent = mention.type === "folder" ? "\uD83D\uDCC1" : "\uD83D\uDCC4";
    chip.appendChild(icon);

    chip.appendChild(document.createTextNode(mention.displayName));
    return chip;
  }

  function placeCursorAfter(node: Node) {
    const sel = window.getSelection();
    if (!sel) return;
    const newRange = document.createRange();
    newRange.setStartAfter(node);
    newRange.collapse(true);
    sel.removeAllRanges();
    sel.addRange(newRange);
  }

  function insertMention(mention: Mention) {
    if (!editorEl || !atTriggerRange) return;

    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0) return;

    // Build the range from @ to current cursor
    const currentRange = sel.getRangeAt(0);
    const replaceRange = document.createRange();
    replaceRange.setStart(atTriggerRange.startContainer, atTriggerRange.startOffset);
    replaceRange.setEnd(currentRange.startContainer, currentRange.startOffset);

    const chip = createMentionChip(mention);

    // Replace the @query text with the chip
    replaceRange.deleteContents();
    replaceRange.insertNode(chip);

    // Insert a space after the chip so cursor can continue typing
    const spacer = document.createTextNode("\u00A0");
    chip.after(spacer);
    placeCursorAfter(spacer);

    // Clear trigger state
    atTriggerRange = null;
    onQueryChange(null);
  }

  function appendMention(mention: Mention) {
    if (!editorEl) return;

    const chip = createMentionChip(mention);
    const spacer = document.createTextNode("\u00A0");
    editorEl.appendChild(chip);
    chip.after(spacer);
    placeCursorAfter(spacer);
  }

  function insertText(text: string) {
    if (!editorEl) return;
    // Split on newlines and interleave text nodes with <br> elements
    // so line breaks render correctly in contenteditable
    const parts = text.split("\n");
    let lastNode: Node | undefined;
    for (let i = 0; i < parts.length; i++) {
      if (i > 0) {
        const br = document.createElement("br");
        editorEl.appendChild(br);
        lastNode = br;
      }
      if (parts[i]) {
        const textNode = document.createTextNode(parts[i]);
        editorEl.appendChild(textNode);
        lastNode = textNode;
      }
    }
    if (lastNode) placeCursorAfter(lastNode);
  }

  function restoreContent(text: string, mentions: Mention[]) {
    if (!editorEl) return;
    editorEl.innerHTML = "";

    // Build segments by finding @displayName tokens in the text and replacing with chips
    interface Segment {
      kind: "text" | "mention";
      value: string;
      mention?: Mention;
    }

    const segments: Segment[] = [];
    let remaining = text;

    // Create a lookup: displayName -> mention (for matching @displayName tokens)
    // A mention may appear multiple times, so use an array and consume matches
    const mentionPool = [...mentions];

    while (remaining.length > 0) {
      let earliestIndex = -1;
      let earliestMention: Mention | null = null;
      let matchLength = 0;

      for (const m of mentionPool) {
        const token = `@${m.displayName}`;
        const idx = remaining.indexOf(token);
        if (idx >= 0 && (earliestIndex === -1 || idx < earliestIndex)) {
          earliestIndex = idx;
          earliestMention = m;
          matchLength = token.length;
        }
      }

      if (earliestIndex === -1 || !earliestMention) {
        segments.push({ kind: "text", value: remaining });
        break;
      }

      if (earliestIndex > 0) {
        segments.push({ kind: "text", value: remaining.substring(0, earliestIndex) });
      }

      segments.push({ kind: "mention", value: "", mention: earliestMention });

      // Remove this match from the pool so each saved mention maps to one chip
      const poolIdx = mentionPool.indexOf(earliestMention);
      if (poolIdx >= 0) mentionPool.splice(poolIdx, 1);

      remaining = remaining.substring(earliestIndex + matchLength);
    }

    // Build the DOM from segments
    for (const seg of segments) {
      if (seg.kind === "text") {
        const parts = seg.value.split("\n");
        for (let i = 0; i < parts.length; i++) {
          if (i > 0) {
            editorEl.appendChild(document.createElement("br"));
          }
          if (parts[i]) {
            editorEl.appendChild(document.createTextNode(parts[i]));
          }
        }
      } else if (seg.mention) {
        const chip = createMentionChip(seg.mention);
        editorEl.appendChild(chip);
      }
    }
  }

  // Expose API via bindable ref
  $effect(() => {
    ref = { insertMention, appendMention, insertText, restoreContent, focus, submit: handleSubmit, getValue: serialize };
  });

  function serialize(): MentionInputValue {
    if (!editorEl) return { text: "", mentions: [] };

    const mentions: Mention[] = [];
    let text = "";

    function walk(node: Node) {
      if (node.nodeType === Node.TEXT_NODE) {
        text += node.textContent ?? "";
      } else if (node.nodeType === Node.ELEMENT_NODE) {
        const el = node as HTMLElement;

        if (el.classList.contains("mention-chip")) {
          const mType = el.dataset.mentionType as "file" | "folder";
          const mPath = el.dataset.mentionPath ?? "";
          // Extract display name: skip the icon span, get remaining text
          let displayName = "";
          for (const child of el.childNodes) {
            if (child.nodeType === Node.TEXT_NODE) {
              displayName += child.textContent ?? "";
            }
          }
          displayName = displayName.trim();
          mentions.push({ type: mType, path: mPath, displayName });
          text += `@${displayName}`;
        } else if (el.tagName === "BR") {
          text += "\n";
        } else {
          // Recurse into children (handles divs that contenteditable creates for newlines)
          const isBlock = el.tagName === "DIV" || el.tagName === "P";
          // Add newline before block elements (except the first child)
          if (isBlock && el.previousSibling) {
            text += "\n";
          }
          for (const child of el.childNodes) {
            walk(child);
          }
        }
      }
    }

    for (const child of editorEl.childNodes) {
      walk(child);
    }

    return { text, mentions };
  }

  function isEmpty(): boolean {
    if (!editorEl) return true;
    // Check if there's any actual content (not just whitespace or empty spans)
    const textContent = editorEl.textContent?.trim() ?? "";
    const hasChips = editorEl.querySelector(".mention-chip") !== null;
    return textContent.length === 0 && !hasChips;
  }

  function handleInput() {
    detectAtQuery();
  }

  function detectAtQuery() {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || !sel.isCollapsed) {
      if (atTriggerRange) {
        atTriggerRange = null;
        onQueryChange(null);
      }
      return;
    }

    const range = sel.getRangeAt(0);
    const container = range.startContainer;

    // Only look at text nodes
    if (container.nodeType !== Node.TEXT_NODE) {
      if (atTriggerRange) {
        atTriggerRange = null;
        onQueryChange(null);
      }
      return;
    }

    const text = container.textContent ?? "";
    const offset = range.startOffset;

    // Scan backwards from cursor to find @ that starts the query
    // The query is valid if there's no space between @ and cursor
    let atPos = -1;
    for (let i = offset - 1; i >= 0; i--) {
      const ch = text[i];
      if (ch === "@") {
        // Found @. Check that the character before it (if any) is a space, newline, or start of text
        if (i === 0 || /\s/.test(text[i - 1])) {
          atPos = i;
        }
        break;
      }
      // If we hit a space or newline, no valid @ trigger
      if (/\s/.test(ch)) break;
    }

    if (atPos >= 0) {
      const query = text.slice(atPos + 1, offset);
      // Store the range of the @ character
      const triggerRange = document.createRange();
      triggerRange.setStart(container, atPos);
      triggerRange.setEnd(container, atPos + 1);
      atTriggerRange = triggerRange;
      onQueryChange(query);
    } else {
      if (atTriggerRange) {
        atTriggerRange = null;
        onQueryChange(null);
      }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      // If autocomplete is open, let the parent handle Enter for selection
      // The parent will call e.preventDefault() via the autocomplete's selectCurrent
      // We only submit if there's no active @ query
      if (atTriggerRange) return; // parent handles this

      // In multiline mode, bare Enter inserts a newline (default contenteditable behavior)
      // Only Cmd+Enter (handled by parent) should submit
      if (multiline) return;

      e.preventDefault();
      handleSubmit();
    }

    if (e.key === "Backspace") {
      handleBackspaceOnChip(e);
    }
  }

  function handleBackspaceOnChip(e: KeyboardEvent) {
    const sel = window.getSelection();
    if (!sel || sel.rangeCount === 0 || !sel.isCollapsed) return;

    const range = sel.getRangeAt(0);
    const container = range.startContainer;
    const offset = range.startOffset;

    // Case 1: Cursor is in a text node at position 0, check previous sibling
    if (container.nodeType === Node.TEXT_NODE && offset === 0) {
      const prev = container.previousSibling;
      if (prev && prev instanceof HTMLElement && prev.classList.contains("mention-chip")) {
        e.preventDefault();
        prev.remove();
        detectAtQuery();
      }
      return;
    }

    // Case 2: Cursor is in the editor div itself (between child nodes)
    if (container === editorEl && offset > 0) {
      const prev = container.childNodes[offset - 1];
      if (prev && prev instanceof HTMLElement && prev.classList.contains("mention-chip")) {
        e.preventDefault();
        prev.remove();
        detectAtQuery();
      }
    }
  }

  function handleSubmit() {
    if (disabled || isEmpty()) return;
    const value = serialize();
    if (!value.text.trim() && value.mentions.length === 0) return;

    // Clear the editor
    if (editorEl) editorEl.innerHTML = "";
    atTriggerRange = null;
    onQueryChange(null);
    onSubmit(value);
  }

  function handlePasteEvent(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;

    // Check for images first - delegate to parent
    for (const item of items) {
      if (item.type.startsWith("image/")) {
        if (onPaste) onPaste(e);
        return;
      }
    }

    // For text, insert as plain text only
    e.preventDefault();
    const text = e.clipboardData?.getData("text/plain") ?? "";
    if (text) {
      document.execCommand("insertText", false, text);
    }
  }
</script>

<div
  bind:this={editorEl}
  class="mention-input"
  contenteditable={!disabled}
  role="textbox"
  tabindex="0"
  data-placeholder={placeholder}
  data-disabled={disabled}
  oninput={handleInput}
  onkeydown={handleKeydown}
  onpaste={handlePasteEvent}
></div>

<style>
  .mention-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    padding: 0.55rem 0.75rem;
    font-family: inherit;
    font-size: 0.85rem;
    line-height: 1.4;
    max-height: 160px;
    overflow-y: auto;
    outline: none;
    min-height: 1.4em;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .mention-input:empty::before {
    content: attr(data-placeholder);
    color: var(--text-dim);
    pointer-events: none;
  }

  .mention-input[data-disabled="true"] {
    opacity: 0.4;
    cursor: default;
  }

  :global(.mention-chip) {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.1rem 0.45rem;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-radius: 4px;
    font-size: 0.8rem;
    color: var(--accent);
    font-family: var(--font-mono);
    vertical-align: baseline;
    line-height: 1.4;
    user-select: all;
    white-space: nowrap;
  }

  :global(.mention-chip-icon) {
    font-size: 0.7rem;
    opacity: 0.7;
  }
</style>

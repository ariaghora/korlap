<script lang="ts">
  import { messagesByWorkspace, sendingByWorkspace, type Message, type MessageChunk } from "$lib/stores/messages.svelte";

  export interface PastedImage {
    id: string;
    dataUrl: string;    // for thumbnail preview
    base64: string;     // raw base64 data (no prefix)
    extension: string;  // png, jpg, etc.
  }

  interface Props {
    workspaceId: string;
    creating?: boolean;
    disabled: boolean;
    onSend: (prompt: string, images: PastedImage[]) => void;
    onStop: () => void;
  }

  let { workspaceId, creating = false, disabled, onSend, onStop }: Props = $props();

  let messages = $derived(messagesByWorkspace.get(workspaceId) ?? []);
  let sending = $derived(sendingByWorkspace.get(workspaceId) ?? false);

  let userInput = $state("");
  let pastedImages = $state<PastedImage[]>([]);
  let chatArea: HTMLDivElement | undefined = $state();
  let userScrolledUp = $state(false);

  // Track which edit diffs are collapsed (by "msgId:chunkIdx" key)
  let collapsedDiffs = $state(new Set<string>());

  function handleScroll(e: Event) {
    const el = e.target as HTMLElement;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    userScrolledUp = !atBottom;
  }

  $effect(() => {
    messages.length;
    sending;
    if (!userScrolledUp && chatArea) {
      requestAnimationFrame(() => {
        chatArea!.scrollTop = chatArea!.scrollHeight;
      });
    }
  });

  // Files touched in the latest agent turn (after last user message)
  let recentFiles = $derived.by(() => {
    const seen = new Set<string>();
    const files: string[] = [];
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i];
      if (msg.role === "user") break;
      for (const chunk of msg.chunks) {
        if (chunk.type === "tool" && chunk.filePath) {
          const name = chunk.filePath.split("/").pop() ?? chunk.filePath;
          if (!seen.has(name)) {
            seen.add(name);
            files.push(name);
          }
        }
      }
    }
    return files;
  });

  function handleSubmit() {
    if ((!userInput.trim() && pastedImages.length === 0) || sending || disabled || creating) return;
    const prompt = userInput.trim();
    const images = [...pastedImages];
    userInput = "";
    pastedImages = [];
    // Reset textarea height after clearing
    const ta = document.querySelector(".input-row textarea") as HTMLTextAreaElement | null;
    if (ta) ta.style.height = "auto";
    onSend(prompt, images);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  }

  function handlePaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;

    for (const item of items) {
      if (!item.type.startsWith("image/")) continue;

      e.preventDefault();
      const file = item.getAsFile();
      if (!file) continue;

      const ext = item.type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = reader.result as string;
        // Extract base64 after the data:image/...;base64, prefix
        const base64 = dataUrl.split(",")[1] ?? "";
        pastedImages = [
          ...pastedImages,
          { id: crypto.randomUUID(), dataUrl, base64, extension: ext },
        ];
      };
      reader.readAsDataURL(file);
    }
  }

  function removeImage(id: string) {
    pastedImages = pastedImages.filter((img) => img.id !== id);
  }

  function autoResize(e: Event) {
    const el = e.target as HTMLTextAreaElement;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 160) + "px";
  }
</script>

<div class="chat-panel">
  <div class="chat-area" bind:this={chatArea} onscroll={handleScroll}>
    {#if creating}
      <div class="chat-empty">
        <div class="creating-spinner"></div>
        <p class="creating-text">Setting up workspace...</p>
      </div>
    {:else if messages.length === 0 && !sending}
      <div class="chat-empty">
        <p>Send a message to start the agent.</p>
      </div>
    {:else}
      {#each messages as msg, i (msg.id)}
        {@const prevRole = i > 0 ? messages[i - 1].role : null}
        {@const showLabel = msg.role === "assistant" && prevRole !== "assistant"}

        {#if msg.role === "action"}
          <div class="action-msg">
            <span class="action-indicator">{msg.actionLabel ?? "Action"}</span>
          </div>
        {:else if msg.role === "user"}
          <div class="user-msg">
            {#if msg.imageDataUrls && msg.imageDataUrls.length > 0}
              <div class="user-images">
                {#each msg.imageDataUrls as dataUrl}
                  <div class="user-image-thumb">
                    <img src={dataUrl} alt="Attached" />
                  </div>
                {/each}
              </div>
            {/if}
            <div class="user-bubble">
              {#each msg.chunks as chunk}
                {#if chunk.type === "text"}{chunk.content}{/if}
              {/each}
            </div>
          </div>
        {:else}
          {#if showLabel}
            <div class="assistant-label">Claude</div>
          {/if}
          <div class="assistant-msg">
            {#each msg.chunks as chunk, ci}
              {#if chunk.type === "thinking"}
                <details class="thinking-block">
                  <summary class="thinking-summary">
                    <span class="thinking-icon">
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                        <path d="M12 2a7 7 0 0 1 7 7c0 2.38-1.19 4.47-3 5.74V17a1 1 0 0 1-1 1H9a1 1 0 0 1-1-1v-2.26C6.19 13.47 5 11.38 5 9a7 7 0 0 1 7-7z"/>
                        <line x1="9" y1="21" x2="15" y2="21"/>
                      </svg>
                    </span>
                    <span class="thinking-label">Thinking</span>
                    <span class="thinking-chevron"></span>
                  </summary>
                  <div class="thinking-content">
                    <p class="thinking-text">{chunk.content}</p>
                  </div>
                </details>
              {:else if chunk.type === "text"}
                <div class="assistant-card">
                  <p class="assistant-text">{chunk.content}</p>
                </div>
              {:else if chunk.type === "tool" && chunk.oldString != null && chunk.newString != null}
                {@const diffKey = `${msg.id}:${ci}`}
                {@const isCollapsed = collapsedDiffs.has(diffKey)}
                <div class="edit-diff-block">
                  <button class="edit-diff-header" onclick={() => {
                    if (collapsedDiffs.has(diffKey)) {
                      collapsedDiffs.delete(diffKey);
                    } else {
                      collapsedDiffs.add(diffKey);
                    }
                    collapsedDiffs = new Set(collapsedDiffs);
                  }}>
                    <span class="edit-diff-chevron" class:collapsed={isCollapsed}>▾</span>
                    <span class="edit-diff-icon">&lt;/&gt;</span>
                    <span class="edit-diff-label">Edit {chunk.input}</span>
                  </button>
                  {#if !isCollapsed}
                    <div class="edit-diff-body">
                      {#each chunk.oldString.split("\n") as line, li}
                        <div class="diff-line remove"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">-</span><span class="diff-code">{line}</span></div>
                      {/each}
                      {#each chunk.newString.split("\n") as line, li}
                        <div class="diff-line add"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">+</span><span class="diff-code">{line}</span></div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {:else}
                <div class="tool-pills">
                  <span class="tool-pill">
                    <span class="tool-icon">⚙</span>
                    {chunk.name}{#if chunk.input}: {chunk.input}{/if}
                  </span>
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      {/each}

      <!-- File pills after last assistant turn -->
      {#if recentFiles.length > 0 && !sending}
        <div class="file-pills-row">
          {#each recentFiles as file}
            <span class="file-pill">
              <span class="file-pill-icon">∞</span>
              {file}
            </span>
          {/each}
        </div>
      {/if}

      {#if sending}
        {@const lastRole = messages.length > 0 ? messages[messages.length - 1].role : null}
        {#if lastRole !== "assistant"}
          <div class="assistant-label">Claude</div>
        {/if}
        <div class="assistant-msg">
          <div class="thinking">Thinking...</div>
        </div>
      {/if}
    {/if}
  </div>

  <form
    class="input-form"
    onsubmit={(e) => {
      e.preventDefault();
      handleSubmit();
    }}
  >
    {#if pastedImages.length > 0}
      <div class="image-preview-strip">
        {#each pastedImages as img (img.id)}
          <div class="image-preview">
            <img src={img.dataUrl} alt="Pasted" />
            <button
              type="button"
              class="image-remove-btn"
              onclick={() => removeImage(img.id)}
            >
              &times;
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="input-row">
      <textarea
        bind:value={userInput}
        onkeydown={handleKeydown}
        oninput={autoResize}
        onpaste={handlePaste}
        placeholder="Ask to make changes, @mention files, run /commands"
        disabled={disabled || creating}
        rows="1"
      ></textarea>
    {#if sending}
      <button type="button" class="stop-btn" onclick={onStop}>Stop</button>
    {:else}
      <button type="submit" class="send-btn" disabled={!userInput.trim() && pastedImages.length === 0 || disabled}
        >Send</button
      >
    {/if}
    </div>
  </form>
</div>

<style>
  .chat-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .chat-area {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .chat-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  .creating-spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-light);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .creating-text {
    color: var(--text-secondary);
    font-size: 0.82rem;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ── User messages ─────────────────────────── */

  .action-msg {
    align-self: center;
    margin: 0.5rem 0;
  }

  .action-indicator {
    font-size: 0.72rem;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    padding: 0.25rem 0.75rem;
    border-radius: 12px;
    font-weight: 500;
  }

  .user-msg {
    align-self: flex-end;
    max-width: 75%;
  }

  .user-bubble {
    background: var(--border);
    border: 1px solid var(--border-light);
    border-radius: 10px;
    padding: 0.5rem 0.85rem;
    color: var(--text-bright);
    font-size: 0.85rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── Assistant messages ────────────────────── */

  .assistant-label {
    font-size: 0.68rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-top: 0.5rem;
    margin-bottom: 0.15rem;
  }

  .assistant-msg {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    max-width: 90%;
  }

  .assistant-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.6rem 0.85rem;
  }

  .assistant-text {
    margin: 0;
    font-size: 0.85rem;
    line-height: 1.55;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── Tool use pills ────────────────────────── */

  .tool-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .tool-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.25rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    font-size: 0.75rem;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    letter-spacing: -0.01em;
  }

  .tool-icon {
    font-size: 0.65rem;
    opacity: 0.6;
  }

  /* ── Inline edit diff ─────────────────────── */

  .edit-diff-block {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-card);
  }

  .edit-diff-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.4rem 0.7rem;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 0.73rem;
    text-align: left;
  }

  .edit-diff-header:hover {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .edit-diff-chevron {
    font-size: 0.65rem;
    opacity: 0.5;
    transition: transform 0.15s ease;
  }

  .edit-diff-chevron.collapsed {
    transform: rotate(-90deg);
  }

  .edit-diff-icon {
    font-size: 0.7rem;
    opacity: 0.6;
    color: var(--accent);
  }

  .edit-diff-label {
    color: var(--text-secondary);
  }

  .edit-diff-body {
    overflow: auto;
    max-height: 300px;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.55;
  }

  .edit-diff-body .diff-line {
    display: flex;
    padding: 0 0.7rem;
    white-space: pre;
  }

  .edit-diff-body .diff-line.add {
    background: var(--diff-add-bg);
    color: var(--diff-add);
  }

  .edit-diff-body .diff-line.remove {
    background: var(--diff-del-bg);
    color: var(--diff-del);
  }

  .edit-diff-body .diff-ln {
    display: inline-block;
    width: 3ch;
    flex-shrink: 0;
    text-align: right;
    padding-right: 0.5ch;
    user-select: none;
    opacity: 0.35;
    color: var(--text-dim);
  }

  .edit-diff-body .diff-prefix {
    display: inline-block;
    width: 1.5ch;
    flex-shrink: 0;
    user-select: none;
    opacity: 0.7;
  }

  .edit-diff-body .diff-code {
    flex: 1;
    min-width: 0;
  }

  /* ── Thinking indicator (while streaming) ──── */

  .thinking {
    font-size: 0.85rem;
    color: var(--accent);
    padding: 0.3rem 0;
    animation: pulse 2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  /* ── Thinking block (collapsible) ──────────── */

  .thinking-block {
    border: 1px solid color-mix(in srgb, var(--accent) 15%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-card));
    overflow: hidden;
  }

  .thinking-summary {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.75rem;
    cursor: pointer;
    user-select: none;
    list-style: none;
    font-size: 0.78rem;
    color: var(--text-dim);
    transition: color 0.15s;
  }

  .thinking-summary::-webkit-details-marker {
    display: none;
  }

  .thinking-summary:hover {
    color: var(--text-secondary);
  }

  .thinking-icon {
    display: flex;
    align-items: center;
    color: var(--accent);
    opacity: 0.7;
  }

  .thinking-label {
    font-weight: 500;
    letter-spacing: 0.01em;
  }

  .thinking-chevron {
    margin-left: auto;
    transition: transform 0.2s ease;
  }

  .thinking-chevron::after {
    content: "▸";
    font-size: 0.7rem;
  }

  .thinking-block[open] .thinking-chevron {
    transform: rotate(90deg);
  }

  .thinking-content {
    padding: 0 0.75rem 0.5rem;
    border-top: 1px solid color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .thinking-text {
    margin: 0.4rem 0 0;
    font-size: 0.8rem;
    line-height: 1.55;
    color: var(--text-dim);
    white-space: pre-wrap;
    word-break: break-word;
    font-style: italic;
  }

  /* ── File pills (inline in chat) ─────────────── */

  .file-pills-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0.2rem 0;
  }

  .file-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    font-size: 0.73rem;
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .file-pill-icon {
    font-size: 0.8rem;
    opacity: 0.5;
  }

  /* ── User attached images (in chat history) ── */

  .user-images {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
    justify-content: flex-end;
    margin-bottom: 0.3rem;
  }

  .user-image-thumb {
    width: 64px;
    height: 64px;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid var(--border-light);
  }

  .user-image-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* ── Input ─────────────────────────────────── */

  .input-form {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
  }

  .image-preview-strip {
    display: flex;
    gap: 0.4rem;
    padding: 0.5rem 1rem 0;
    flex-wrap: wrap;
  }

  .image-preview {
    position: relative;
    width: 56px;
    height: 56px;
    border-radius: 6px;
    overflow: visible;
    flex-shrink: 0;
  }

  .image-preview img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--border-light);
  }

  .image-remove-btn {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .image-remove-btn:hover {
    background: var(--border);
    color: var(--text-bright);
  }

  .input-row {
    display: flex;
    align-items: flex-end;
    gap: 0.5rem;
    padding: 0.6rem 1rem;
  }

  .input-row textarea {
    flex: 1;
    background: var(--bg-card);
    border: 1px solid var(--border);
    color: var(--text-primary);
    padding: 0.55rem 0.85rem;
    border-radius: 8px;
    font-family: inherit;
    font-size: 0.85rem;
    resize: none;
    overflow-y: auto;
    line-height: 1.4;
    max-height: 160px;
  }

  .input-row textarea:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--accent) 33%, transparent);
  }

  .input-row textarea:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .send-btn {
    padding: 0.55rem 1rem;
    background: var(--border);
    border: 1px solid var(--border-light);
    color: var(--text-primary);
    border-radius: 8px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .send-btn:hover:not(:disabled) {
    background: var(--border-light);
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .stop-btn {
    padding: 0.55rem 1rem;
    background: var(--diff-del-bg);
    border: 1px solid var(--diff-del);
    color: var(--diff-del);
    border-radius: 8px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
    font-weight: 500;
  }

  .stop-btn:hover {
    filter: brightness(1.2);
  }
</style>

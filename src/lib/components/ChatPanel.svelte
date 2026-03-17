<script lang="ts">
  import type { Message, MessageChunk } from "$lib/stores/messages.svelte";

  interface Props {
    messages: Message[];
    sending: boolean;
    disabled: boolean;
    onSend: (prompt: string) => void;
  }

  let { messages, sending, disabled, onSend }: Props = $props();

  let userInput = $state("");
  let chatArea: HTMLDivElement | undefined = $state();
  let userScrolledUp = $state(false);

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
    if (!userInput.trim() || sending || disabled) return;
    const prompt = userInput.trim();
    userInput = "";
    onSend(prompt);
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  }
</script>

<div class="chat-panel">
  <div class="chat-area" bind:this={chatArea} onscroll={handleScroll}>
    {#if messages.length === 0 && !sending}
      <div class="chat-empty">
        <p>Send a message to start the agent.</p>
      </div>
    {:else}
      {#each messages as msg, i (msg.id)}
        {@const prevRole = i > 0 ? messages[i - 1].role : null}
        {@const showLabel = msg.role === "assistant" && prevRole !== "assistant"}

        {#if msg.role === "user"}
          <div class="user-msg">
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
            {#each msg.chunks as chunk}
              {#if chunk.type === "text"}
                <div class="assistant-card">
                  <p class="assistant-text">{chunk.content}</p>
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
    class="input-row"
    onsubmit={(e) => {
      e.preventDefault();
      handleSubmit();
    }}
  >
    <input
      bind:value={userInput}
      onkeydown={handleKeydown}
      placeholder="Ask to make changes, @mention files, run /commands"
      disabled={disabled}
    />
    <button type="submit" class="send-btn" disabled={sending || !userInput.trim() || disabled}
      >Send</button
    >
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
    align-items: center;
    justify-content: center;
    color: #6a6050;
    font-size: 0.85rem;
  }

  /* ── User messages ─────────────────────────── */

  .user-msg {
    align-self: flex-end;
    max-width: 75%;
  }

  .user-bubble {
    background: #2a2520;
    border: 1px solid #3a3530;
    border-radius: 10px;
    padding: 0.5rem 0.85rem;
    color: #e8dcc8;
    font-size: 0.85rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── Assistant messages ────────────────────── */

  .assistant-label {
    font-size: 0.68rem;
    color: #6a6050;
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
    background: #1a1814;
    border: 1px solid #2a2520;
    border-radius: 8px;
    padding: 0.6rem 0.85rem;
  }

  .assistant-text {
    margin: 0;
    font-size: 0.85rem;
    line-height: 1.55;
    color: #d4c5a9;
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
    background: #1a1814;
    border: 1px solid #2e2a24;
    border-radius: 14px;
    font-size: 0.75rem;
    color: #8a7e6a;
    font-family: "SF Mono", "Fira Code", monospace;
    letter-spacing: -0.01em;
  }

  .tool-icon {
    font-size: 0.65rem;
    opacity: 0.6;
  }

  /* ── Thinking ──────────────────────────────── */

  .thinking {
    font-size: 0.85rem;
    color: #c8a97e;
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
    background: #1a1814;
    border: 1px solid #2e2a24;
    border-radius: 14px;
    font-size: 0.73rem;
    color: #8a7e6a;
    font-family: "SF Mono", "Fira Code", monospace;
  }

  .file-pill-icon {
    font-size: 0.8rem;
    opacity: 0.5;
  }

  /* ── Input ─────────────────────────────────── */

  .input-row {
    display: flex;
    gap: 0.5rem;
    padding: 0.6rem 1rem;
    border-top: 1px solid #2a2520;
  }

  .input-row input {
    flex: 1;
    background: #1a1814;
    border: 1px solid #2e2a24;
    color: #d4c5a9;
    padding: 0.55rem 0.85rem;
    border-radius: 8px;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .input-row input:focus {
    outline: none;
    border-color: #c8a97e55;
  }

  .input-row input:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .send-btn {
    padding: 0.55rem 1rem;
    background: #2a2520;
    border: 1px solid #3a3530;
    color: #d4c5a9;
    border-radius: 8px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .send-btn:hover:not(:disabled) {
    background: #3a3530;
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

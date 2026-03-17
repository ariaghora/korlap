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

  // Auto-scroll to bottom on new messages (unless user scrolled up)
  $effect(() => {
    messages.length;
    sending;
    if (!userScrolledUp && chatArea) {
      requestAnimationFrame(() => {
        chatArea!.scrollTop = chatArea!.scrollHeight;
      });
    }
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

  function formatChunk(chunk: MessageChunk): string {
    if (chunk.type === "text") return chunk.content;
    if (chunk.input) return `${chunk.name}: ${chunk.input}`;
    return chunk.name;
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
        <div class="chat-msg" class:user={msg.role === "user"} class:consecutive={msg.role === "assistant" && !showLabel}>
          {#if showLabel}
            <div class="msg-label">Claude</div>
          {/if}
          {#each msg.chunks as chunk}
            {#if chunk.type === "text"}
              <div class="msg-text">{chunk.content}</div>
            {:else}
              <span class="tool-tag">{formatChunk(chunk)}</span>
            {/if}
          {/each}
        </div>
      {/each}
      {#if sending}
        {@const lastRole = messages.length > 0 ? messages[messages.length - 1].role : null}
        <div class="chat-msg" class:consecutive={lastRole === "assistant"}>
          {#if lastRole !== "assistant"}
            <div class="msg-label">Claude</div>
          {/if}
          <div class="msg-thinking">Thinking...</div>
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
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .chat-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6a6050;
    font-size: 0.85rem;
  }

  .chat-msg {
    max-width: 85%;
  }

  .chat-msg.user {
    align-self: flex-end;
  }

  .chat-msg.consecutive {
    margin-top: -0.4rem;
  }

  .chat-msg.user .msg-text {
    background: #2a2520;
    border: 1px solid #3a3530;
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    color: #e8dcc8;
  }

  .msg-label {
    font-size: 0.7rem;
    color: #6a6050;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 0.25rem;
  }

  .msg-text {
    font-size: 0.85rem;
    line-height: 1.5;
    color: #d4c5a9;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .msg-thinking {
    font-size: 0.85rem;
    color: #c8a97e;
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

  .tool-tag {
    display: inline-block;
    padding: 0.2rem 0.5rem;
    background: #1e1b17;
    border: 1px solid #2e2a24;
    border-radius: 4px;
    font-size: 0.72rem;
    color: #8a7e6a;
    font-family: "SF Mono", "Fira Code", monospace;
    margin-top: 0.3rem;
  }

  .input-row {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-top: 1px solid #2a2520;
  }

  .input-row input {
    flex: 1;
    background: #1e1b17;
    border: 1px solid #2e2a24;
    color: #d4c5a9;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .input-row input:focus {
    outline: none;
    border-color: #c8a97e;
  }

  .input-row input:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .send-btn {
    padding: 0.5rem 1rem;
    background: #2a2520;
    border: 1px solid #3a3530;
    color: #d4c5a9;
    border-radius: 6px;
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

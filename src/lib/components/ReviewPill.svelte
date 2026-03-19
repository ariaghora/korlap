<script lang="ts">
  import { renderMarkdown } from "$lib/markdown";
  import { CircleCheck, CircleX, Loader2, X } from "lucide-svelte";

  export interface ReviewState {
    status: "running" | "complete";
    currentTask: string;
    resultMarkdown: string;
  }

  interface Props {
    state: ReviewState;
    onCancel: () => void;
    onSendToChat: (markdown: string) => void;
  }

  let { state, onCancel, onSendToChat }: Props = $props();

  // Classify the result: "clean" (no issues), "failed", or "issues"
  let resultKind = $derived.by(() => {
    if (state.status !== "complete") return null;
    const text = state.resultMarkdown.trim().toLowerCase();
    if (text.startsWith("**review failed:**")) return "failed" as const;
    if (text === "no issues found." || text === "no issues found") return "clean" as const;
    return "issues" as const;
  });

  let renderedHtml = $derived(
    state.status === "complete" && resultKind === "issues"
      ? renderMarkdown(state.resultMarkdown)
      : ""
  );

  let failedMessage = $derived(
    resultKind === "failed"
      ? state.resultMarkdown.replace(/^\*\*Review failed:\*\*\s*/i, "").trim()
      : ""
  );
</script>

<div
  class="review-pill"
  class:complete={state.status === "complete" && resultKind === "issues"}
  class:clean={resultKind === "clean"}
  class:failed={resultKind === "failed"}
>
  {#if state.status === "running"}
    <!-- Running: compact pill with spinner -->
    <div class="pill-running">
      <Loader2 size={13} class="spinner" />
      <span class="task-text">{state.currentTask}</span>
      <button class="pill-btn cancel" onclick={onCancel} title="Cancel review">
        <X size={12} />
      </button>
    </div>
  {:else if resultKind === "clean"}
    <!-- No issues: compact success pill -->
    <div class="pill-clean">
      <CircleCheck size={14} class="check-icon" />
      <span class="clean-text">No issues found</span>
      <button class="pill-btn dismiss" onclick={onCancel} title="Dismiss">
        <X size={12} />
      </button>
    </div>
  {:else if resultKind === "failed"}
    <!-- Failed: compact error pill -->
    <div class="pill-failed">
      <CircleX size={14} class="error-icon" />
      <span class="failed-text">{failedMessage}</span>
      <button class="pill-btn dismiss" onclick={onCancel} title="Dismiss">
        <X size={12} />
      </button>
    </div>
  {:else}
    <!-- Issues found: full card -->
    <div class="pill-header">
      <span class="pill-title">Review</span>
      <button class="pill-btn dismiss" onclick={onCancel} title="Dismiss">
        <X size={12} />
      </button>
    </div>
    <div class="pill-body">
      {@html renderedHtml}
    </div>
    <div class="pill-footer">
      <button class="send-btn" onclick={() => onSendToChat(state.resultMarkdown)}>
        Send to Chat
      </button>
    </div>
  {/if}
</div>

<style>
  .review-pill {
    position: absolute;
    top: 12px;
    right: 12px;
    z-index: 10;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 20px;
    padding: 0.4rem 0.6rem;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    max-width: 400px;
    min-width: 200px;
  }

  /* ── Running state ─────────────────────────── */

  .pill-running {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .pill-running :global(.spinner) {
    color: var(--accent);
    animation: spin 1s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .task-text {
    font-size: 0.75rem;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  /* ── Clean (no issues) ─────────────────────── */

  .review-pill.clean {
    border-color: color-mix(in srgb, #7e9e6b 40%, transparent);
  }

  .pill-clean {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .pill-clean :global(.check-icon) {
    color: #7e9e6b;
    flex-shrink: 0;
  }

  .clean-text {
    font-size: 0.75rem;
    font-weight: 500;
    color: #7e9e6b;
    flex: 1;
  }

  /* ── Failed ────────────────────────────────── */

  .review-pill.failed {
    border-color: color-mix(in srgb, #b5564e 40%, transparent);
  }

  .pill-failed {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .pill-failed :global(.error-icon) {
    color: #b5564e;
    flex-shrink: 0;
  }

  .failed-text {
    font-size: 0.75rem;
    color: #b5564e;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  /* ── Issues found (full card) ──────────────── */

  .review-pill.complete {
    border-radius: 12px;
    padding: 0;
    max-height: 60vh;
    display: flex;
    flex-direction: column;
  }

  .pill-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .pill-title {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--accent);
  }

  .pill-body {
    padding: 0.75rem;
    overflow-y: auto;
    font-size: 0.8rem;
    line-height: 1.5;
    color: var(--text-primary);
    min-height: 0;
    flex: 1;
  }

  .pill-body :global(h1),
  .pill-body :global(h2),
  .pill-body :global(h3) {
    font-size: 0.85rem;
    font-weight: 600;
    margin: 0.75rem 0 0.35rem;
    color: var(--text-primary);
  }

  .pill-body :global(h1:first-child),
  .pill-body :global(h2:first-child),
  .pill-body :global(h3:first-child) {
    margin-top: 0;
  }

  .pill-body :global(p) {
    margin: 0.35rem 0;
  }

  .pill-body :global(ul),
  .pill-body :global(ol) {
    padding-left: 1.2rem;
    margin: 0.35rem 0;
  }

  .pill-body :global(li) {
    margin: 0.2rem 0;
  }

  .pill-body :global(code) {
    font-size: 0.75rem;
    background: rgba(255, 255, 255, 0.05);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .pill-body :global(pre) {
    background: rgba(0, 0, 0, 0.3);
    padding: 0.5rem;
    border-radius: 6px;
    overflow-x: auto;
    margin: 0.35rem 0;
  }

  .pill-body :global(pre code) {
    background: none;
    padding: 0;
  }

  .pill-footer {
    padding: 0.5rem 0.75rem;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .send-btn {
    width: 100%;
    padding: 0.4rem 0.75rem;
    font-size: 0.75rem;
    font-weight: 600;
    font-family: inherit;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 6px;
    cursor: pointer;
  }

  .send-btn:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  /* ── Shared ────────────────────────────────── */

  .pill-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .pill-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
  }
</style>

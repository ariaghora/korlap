<script lang="ts">
  import { getDiff } from "$lib/ipc";

  interface Props {
    workspaceId: string;
  }

  let { workspaceId }: Props = $props();

  let diff = $state("");
  let loading = $state(false);
  let error = $state("");

  async function loadDiff() {
    loading = true;
    error = "";
    try {
      diff = await getDiff(workspaceId);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // Load on mount and when workspaceId changes
  $effect(() => {
    workspaceId;
    loadDiff();
  });

  interface DiffLine {
    type: "add" | "remove" | "context" | "header" | "hunk";
    text: string;
  }

  function parseDiff(raw: string): DiffLine[] {
    if (!raw.trim()) return [];
    return raw.split("\n").map((line) => {
      if (line.startsWith("+++") || line.startsWith("---") || line.startsWith("diff ") || line.startsWith("index ")) {
        return { type: "header" as const, text: line };
      }
      if (line.startsWith("@@")) {
        return { type: "hunk" as const, text: line };
      }
      if (line.startsWith("+")) {
        return { type: "add" as const, text: line };
      }
      if (line.startsWith("-")) {
        return { type: "remove" as const, text: line };
      }
      return { type: "context" as const, text: line };
    });
  }

  let lines = $derived(parseDiff(diff));
</script>

<div class="diff-viewer">
  {#if loading}
    <div class="diff-status">Loading diff...</div>
  {:else if error}
    <div class="diff-error">{error}</div>
  {:else if lines.length === 0}
    <div class="diff-status">
      <p>No changes yet.</p>
      <button class="refresh-btn" onclick={loadDiff}>Refresh</button>
    </div>
  {:else}
    <div class="diff-toolbar">
      <button class="refresh-btn" onclick={loadDiff}>Refresh</button>
    </div>
    <div class="diff-content">
      {#each lines as line, i}
        <div class="diff-line {line.type}" data-line={i + 1}>
          <span class="diff-gutter">{line.type === "add" ? "+" : line.type === "remove" ? "-" : " "}</span>
          <span class="diff-text">{line.text.slice(1) || " "}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .diff-viewer {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .diff-status {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: #6a6050;
    font-size: 0.85rem;
  }

  .diff-error {
    padding: 1rem;
    color: #e88;
    font-size: 0.85rem;
  }

  .diff-toolbar {
    display: flex;
    justify-content: flex-end;
    padding: 0.4rem 0.75rem;
    border-bottom: 1px solid #2a2520;
  }

  .refresh-btn {
    padding: 0.25rem 0.6rem;
    background: #2a2520;
    border: 1px solid #3a3530;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
  }

  .refresh-btn:hover {
    color: #d4c5a9;
    background: #3a3530;
  }

  .diff-content {
    flex: 1;
    overflow: auto;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.78rem;
    line-height: 1.6;
  }

  .diff-line {
    display: flex;
    padding: 0 0.75rem;
    white-space: pre;
  }

  .diff-line.add {
    background: #1a2a1a;
    color: #7e9e6b;
  }

  .diff-line.remove {
    background: #2a1a1a;
    color: #c87e7e;
  }

  .diff-line.context {
    color: #6a6050;
  }

  .diff-line.header {
    color: #8a7e6a;
    font-weight: 600;
    padding-top: 0.5rem;
  }

  .diff-line.hunk {
    color: #c8a97e;
    background: #1a1814;
    padding-top: 0.3rem;
    padding-bottom: 0.3rem;
    margin-top: 0.25rem;
  }

  .diff-gutter {
    width: 1.5ch;
    flex-shrink: 0;
    text-align: center;
    opacity: 0.6;
    user-select: none;
  }

  .diff-text {
    flex: 1;
  }
</style>

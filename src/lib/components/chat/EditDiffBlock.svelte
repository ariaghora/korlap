<script lang="ts">
  import { Pencil } from "lucide-svelte";
  import type { MessageChunk } from "$lib/stores/messages.svelte";

  interface Props {
    chunk: MessageChunk & { type: "tool" };
  }

  let { chunk }: Props = $props();

  let collapsed = $state(false);
</script>

<div class="edit-diff-block">
  <button class="edit-diff-header" onclick={() => { collapsed = !collapsed; }}>
    <span class="edit-diff-chevron" class:collapsed>▾</span>
    <span class="edit-diff-icon"><Pencil size={13} strokeWidth={2} /></span>
    <span class="edit-diff-label">{chunk.input}</span>
  </button>
  {#if !collapsed}
    <div class="edit-diff-body">
      {#each (chunk.oldString ?? "").split("\n") as line, li (li)}
        <div class="diff-line remove"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">-</span><span class="diff-code">{line}</span></div>
      {/each}
      {#each (chunk.newString ?? "").split("\n") as line, li (li)}
        <div class="diff-line add"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">+</span><span class="diff-code">{line}</span></div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .edit-diff-block {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-card);
  }

  .edit-diff-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
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
    display: flex;
    align-items: center;
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
</style>

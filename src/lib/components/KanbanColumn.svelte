<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    title: string;
    count: number;
    accent?: boolean;
    dimmed?: boolean;
    children: Snippet;
    footer?: Snippet;
    headerAction?: Snippet;
  }

  let { title, count, accent = false, dimmed = false, children, footer, headerAction }: Props = $props();
</script>

<div class="column" class:dimmed>
  <div class="column-header">
    <span class="column-title">{title}</span>
    <span class="column-count" class:accent>{count}</span>
    {#if headerAction}
      <span class="header-action-spacer"></span>
      {@render headerAction()}
    {/if}
  </div>
  <div class="column-body">
    {@render children()}
  </div>
  {#if footer}
    <div class="column-footer">
      {@render footer()}
    </div>
  {/if}
</div>

<style>
  .column {
    flex: 1;
    min-width: 0;
    max-width: 380px;
    display: flex;
    flex-direction: column;
    background: var(--bg-sidebar);
    border-radius: 8px;
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .column.dimmed {
    opacity: 0.38;
  }

  .column.dimmed:hover {
    opacity: 0.55;
  }

  .column-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .column-title {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .column-count {
    font-size: 0.6rem;
    font-weight: 600;
    min-width: 1.2rem;
    height: 1.2rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    background: var(--border);
    color: var(--text-dim);
  }

  .column-count.accent {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    color: var(--accent);
  }

  .header-action-spacer {
    flex: 1;
  }

  .column-body {
    flex: 1;
    overflow-y: auto;
    padding: 0.5rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .column-footer {
    border-top: 1px solid var(--border);
    padding: 0.5rem;
    flex-shrink: 0;
  }
</style>

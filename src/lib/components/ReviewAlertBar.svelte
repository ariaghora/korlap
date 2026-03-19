<script lang="ts">
  import type { WorkspaceInfo } from "$lib/ipc";

  interface Props {
    workspace: WorkspaceInfo;
    moreCount: number;
    onReviewNow: () => void;
  }

  let { workspace, moreCount, onReviewNow }: Props = $props();
</script>

<div class="review-alert">
  <div class="alert-content">
    <span class="alert-dot"></span>
    <span class="alert-branch">{workspace.branch}</span>
    <span class="alert-text">— Opus finished review.</span>
    {#if moreCount > 0}
      <span class="alert-more">+{moreCount} more</span>
    {/if}
  </div>
  <button class="alert-btn" onclick={onReviewNow}>Review now</button>
</div>

<style>
  .review-alert {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.35rem 0.75rem;
    background: color-mix(in srgb, var(--accent) 8%, var(--bg-base));
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    flex-shrink: 0;
  }

  .alert-content {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.75rem;
    min-width: 0;
  }

  .alert-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
    animation: blink 3s ease-in-out infinite;
  }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.2; }
  }

  .alert-branch {
    color: var(--accent);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .alert-text {
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .alert-more {
    color: var(--text-dim);
    font-size: 0.68rem;
    flex-shrink: 0;
  }

  .alert-btn {
    padding: 0.25rem 0.6rem;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 4px;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
  }

  .alert-btn:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
    border-color: color-mix(in srgb, var(--accent) 45%, transparent);
  }
</style>

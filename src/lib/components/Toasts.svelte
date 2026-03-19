<script lang="ts">
  import { getToasts, removeToast, type Toast } from "$lib/stores/toasts.svelte";

  const toasts = getToasts();
  let list = $derived([...toasts.values()].reverse());
</script>

{#if list.length > 0}
  <div class="toast-container" role="status" aria-live="polite">
    {#each list as toast (toast.id)}
      <div class="toast toast-{toast.type}">
        <span class="toast-msg">{toast.message}</span>
        <button class="toast-dismiss" onclick={() => removeToast(toast.id)} aria-label="Dismiss">×</button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    bottom: 8px;
    right: 8px;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 400px;
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    font-family: var(--font-sans);
    line-height: 1.4;
    word-break: break-word;
    border: 1px solid var(--border);
    background: var(--bg-card);
  }

  .toast-error {
    color: var(--error);
  }

  .toast-info {
    color: var(--text-secondary);
  }

  .toast-success {
    color: var(--status-ok);
  }

  .toast-msg {
    flex: 1;
    min-width: 0;
  }

  .toast-dismiss {
    flex-shrink: 0;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 1rem;
    padding: 0;
    line-height: 1;
  }

  .toast-dismiss:hover {
    color: var(--text-secondary);
  }
</style>

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
    bottom: 12px;
    right: 12px;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 420px;
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 10px 14px;
    border-radius: 6px;
    font-size: 0.8rem;
    font-family: var(--font-sans);
    line-height: 1.4;
    word-break: break-word;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.5);
    animation: toast-in 200ms ease-out;
  }

  .toast-error {
    background: var(--error-bg);
    color: var(--error);
    border-left: 3px solid var(--error);
  }

  .toast-info {
    background: #1a1e2a;
    color: #8ab4f8;
    border-left: 3px solid #8ab4f8;
  }

  .toast-success {
    background: #1a2a1a;
    color: var(--status-ok);
    border-left: 3px solid var(--status-ok);
  }

  .toast-msg {
    flex: 1;
    min-width: 0;
  }

  .toast-dismiss {
    flex-shrink: 0;
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    font-size: 1.1rem;
    padding: 0;
    line-height: 1;
    opacity: 0.6;
  }

  .toast-dismiss:hover {
    opacity: 1;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }
</style>

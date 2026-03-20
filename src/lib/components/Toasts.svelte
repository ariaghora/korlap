<script lang="ts">
  import { getToasts, removeToast, type Toast } from "$lib/stores/toasts.svelte";
  import { Copy, Check, X } from "lucide-svelte";

  const toasts = getToasts();
  let list = $derived([...toasts.values()].reverse());
  let copied = $state<string | null>(null);

  function copyMessage(toast: Toast) {
    navigator.clipboard.writeText(toast.message);
    copied = toast.id;
    setTimeout(() => { if (copied === toast.id) copied = null; }, 1200);
  }
</script>

{#if list.length > 0}
  <div class="toast-container" role="status" aria-live="polite">
    {#each list as toast (toast.id)}
      <div class="toast toast-{toast.type}">
        <span class="toast-msg">{toast.message}</span>
        <button class="toast-btn" onclick={() => copyMessage(toast)} aria-label="Copy">
          {#if copied === toast.id}<Check size={13} />{:else}<Copy size={13} />{/if}
        </button>
        <button class="toast-btn" onclick={() => removeToast(toast.id)} aria-label="Dismiss">
          <X size={13} />
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    bottom: 32px;
    right: 12px;
    z-index: 9999;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 380px;
    pointer-events: none;
  }

  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    font-size: 0.8rem;
    font-family: var(--font-sans);
    line-height: 1.35;
    word-break: break-word;
    border-radius: 10px;
    background: var(--toast-bg);
    -webkit-backdrop-filter: saturate(180%) blur(20px);
    backdrop-filter: saturate(180%) blur(20px);
    border: 0.5px solid var(--toast-border);
    box-shadow: 0 2px 12px rgba(0, 0, 0, 0.35);
  }

  .toast-error {
    color: var(--error);
  }

  .toast-info {
    color: var(--text-primary);
  }

  .toast-success {
    color: var(--status-ok);
  }

  .toast-msg {
    flex: 1;
    min-width: 0;
  }

  .toast-btn {
    flex-shrink: 0;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 0.75rem;
    padding: 0;
    line-height: 1;
  }

  .toast-btn:hover {
    color: var(--text-secondary);
  }
</style>

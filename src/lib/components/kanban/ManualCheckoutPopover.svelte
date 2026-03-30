<script lang="ts">
  export interface ManualCheckoutData {
    branchName: string;
    description: string;
  }

  interface Props {
    onSubmit: (data: ManualCheckoutData) => void;
    onCancel: () => void;
  }

  let { onSubmit, onCancel }: Props = $props();

  let branchName = $state("");
  let description = $state("");
  let branchRef: HTMLInputElement | undefined = $state();

  $effect(() => {
    requestAnimationFrame(() => branchRef?.focus());
  });

  let canSubmit = $derived(branchName.trim().length > 0);

  function submit() {
    if (!canSubmit) return;
    onSubmit({ branchName: branchName.trim(), description: description.trim() });
  }

  function handleOverlayKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
    if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      submit();
    }
  }

  function handleBranchKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      descRef?.focus();
    }
  }

  let descRef: HTMLTextAreaElement | undefined = $state();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel} onkeydown={handleOverlayKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <div class="dialog-header">Manual checkout</div>
    <input
      class="branch-input"
      bind:this={branchRef}
      bind:value={branchName}
      onkeydown={handleBranchKeydown}
      placeholder="feat/my-feature"
      spellcheck={false}
    />
    <textarea
      class="desc-input"
      bind:this={descRef}
      bind:value={description}
      placeholder="Description (optional)"
      rows={3}
    ></textarea>
    <div class="dialog-footer">
      <span class="footer-hint">⌘Enter to create</span>
      <div class="footer-actions">
        <button class="cancel-btn" onclick={onCancel}>Cancel</button>
        <button class="submit-btn" onclick={submit} disabled={!canSubmit}>Create</button>
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dialog {
    width: 420px;
    max-width: 90vw;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem;
    background: color-mix(in srgb, var(--bg-sidebar) 97%, white);
    border: 0.5px solid color-mix(in srgb, var(--border-light) 60%, transparent);
    border-radius: 12px;
    box-shadow:
      0 0 0 0.5px rgba(0, 0, 0, 0.3),
      0 8px 32px rgba(0, 0, 0, 0.45),
      0 2px 8px rgba(0, 0, 0, 0.2);
  }

  .dialog-header {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 0 0.1rem 0.15rem;
  }

  .branch-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.55rem 0.65rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 8px;
    color: var(--text-bright);
    font-family: var(--font-mono);
    font-size: 0.92rem;
    font-weight: 600;
    outline: none;
  }

  .branch-input::placeholder {
    color: var(--text-muted);
    font-weight: 400;
  }

  .branch-input:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .desc-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.55rem 0.65rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 8px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.85rem;
    outline: none;
    resize: vertical;
    min-height: 60px;
  }

  .desc-input::placeholder {
    color: var(--text-muted);
  }

  .desc-input:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 0.15rem;
  }

  .footer-hint {
    font-size: 0.65rem;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .footer-actions {
    display: flex;
    gap: 0.35rem;
    align-items: center;
  }

  .cancel-btn {
    padding: 0.35rem 0.7rem;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .cancel-btn:hover {
    background: var(--btn-subtle-hover);
    color: var(--text-primary);
  }

  .submit-btn {
    padding: 0.35rem 0.9rem;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: var(--bg-base);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
  }

  .submit-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>

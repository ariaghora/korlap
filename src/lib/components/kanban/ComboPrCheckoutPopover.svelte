<script lang="ts">
  import { listRepoPrs, type RepoPrEntry } from "$lib/ipc";
  import { Check } from "lucide-svelte";

  interface Props {
    repoId: string;
    onSubmit: (prNumbers: number[]) => void;
    onCancel: () => void;
  }

  let { repoId, onSubmit, onCancel }: Props = $props();

  let prs = $state<RepoPrEntry[]>([]);
  let loading = $state(true);
  let error = $state("");
  let search = $state("");
  let searchRef: HTMLInputElement | undefined = $state();
  let selected = $state(new Set<number>());

  $effect(() => {
    loadPrs();
    requestAnimationFrame(() => searchRef?.focus());
  });

  async function loadPrs() {
    loading = true;
    error = "";
    try {
      prs = await listRepoPrs(repoId);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  let filtered = $derived(() => {
    const q = search.trim().toLowerCase();
    if (!q) return prs;
    return prs.filter(
      (pr) =>
        String(pr.number).includes(q) ||
        pr.title.toLowerCase().includes(q) ||
        pr.branch.toLowerCase().includes(q) ||
        pr.author.toLowerCase().includes(q),
    );
  });

  function togglePr(prNumber: number) {
    const next = new Set(selected);
    if (next.has(prNumber)) {
      next.delete(prNumber);
    } else {
      next.add(prNumber);
    }
    selected = next;
  }

  function handleSubmit() {
    if (selected.size < 2) return;
    onSubmit([...selected]);
  }

  function handleOverlayKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel} onkeydown={handleOverlayKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <div class="dialog-header">Combo PRs</div>
    <input
      class="search-input"
      bind:this={searchRef}
      bind:value={search}
      placeholder="Search PRs by number, title, branch, or author…"
      spellcheck={false}
    />
    <div class="pr-list">
      {#if loading}
        <div class="empty-state">Loading PRs…</div>
      {:else if error}
        <div class="empty-state error">{error}</div>
      {:else if filtered().length === 0}
        <div class="empty-state">
          {search.trim() ? "No PRs match your search" : "No open PRs found"}
        </div>
      {:else}
        {#each filtered() as pr (pr.number)}
          <button
            class="pr-row"
            class:pr-selected={selected.has(pr.number)}
            onclick={() => togglePr(pr.number)}
          >
            <div class="pr-check">
              {#if selected.has(pr.number)}
                <span class="check-icon"><Check size={12} /></span>
              {:else}
                <span class="check-empty"></span>
              {/if}
            </div>
            <div class="pr-content">
              <div class="pr-top">
                <span class="pr-number">#{pr.number}</span>
                <span class="pr-title">{pr.title}</span>
              </div>
              <div class="pr-bottom">
                <span class="pr-branch">{pr.branch}</span>
                <span class="pr-meta">{pr.author}</span>
                <span class="pr-stat add">+{pr.additions}</span>
                <span class="pr-stat del">-{pr.deletions}</span>
              </div>
            </div>
          </button>
        {/each}
      {/if}
    </div>
    <div class="dialog-footer">
      <span class="footer-hint">
        {#if selected.size === 0}
          Select at least 2 PRs to combine
        {:else if selected.size === 1}
          Select 1 more PR
        {:else}
          {selected.size} PRs selected
        {/if}
      </span>
      <div class="footer-actions">
        <button class="cancel-btn" onclick={onCancel}>Cancel</button>
        <button
          class="submit-btn"
          disabled={selected.size < 2}
          onclick={handleSubmit}
        >
          Create Combo
        </button>
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
    width: 520px;
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

  .search-input {
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
  }

  .search-input::placeholder {
    color: var(--text-muted);
  }

  .search-input:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .pr-list {
    max-height: 340px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-radius: 8px;
  }

  .empty-state {
    padding: 1.5rem;
    text-align: center;
    color: var(--text-muted);
    font-size: 0.82rem;
  }

  .empty-state.error {
    color: var(--text-danger, #f87171);
  }

  .pr-row {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.5rem 0.65rem;
    background: transparent;
    border: none;
    border-radius: 6px;
    text-align: left;
    cursor: pointer;
    width: 100%;
    font-family: inherit;
  }

  .pr-row:hover {
    background: var(--btn-subtle-hover);
  }

  .pr-row.pr-selected {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .pr-check {
    flex-shrink: 0;
    margin-top: 0.15rem;
  }

  .check-empty {
    display: block;
    width: 14px;
    height: 14px;
    border-radius: 3px;
    border: 1.5px solid var(--text-muted);
    opacity: 0.5;
  }

  .check-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    border-radius: 3px;
    background: var(--accent);
    color: white;
  }

  .pr-content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .pr-top {
    display: flex;
    align-items: baseline;
    gap: 0.4rem;
  }

  .pr-number {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--accent);
    flex-shrink: 0;
  }

  .pr-title {
    font-size: 0.82rem;
    color: var(--text-bright);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pr-bottom {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.72rem;
    color: var(--text-muted);
  }

  .pr-branch {
    font-family: var(--font-mono);
    font-size: 0.7rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pr-meta {
    flex-shrink: 0;
  }

  .pr-stat {
    flex-shrink: 0;
    font-family: var(--font-mono);
    font-size: 0.68rem;
  }

  .pr-stat.add {
    color: var(--diff-add, #4ade80);
  }

  .pr-stat.del {
    color: var(--diff-del, #f87171);
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
    gap: 0.4rem;
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
    padding: 0.35rem 0.7rem;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: white;
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
  }

  .submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .submit-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }
</style>

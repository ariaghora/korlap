<script lang="ts">
  import { listRepoPrs, type RepoPrEntry } from "$lib/ipc";

  interface Props {
    repoId: string;
    onSelect: (prNumber: number) => void;
    onCancel: () => void;
  }

  let { repoId, onSelect, onCancel }: Props = $props();

  let prs = $state<RepoPrEntry[]>([]);
  let loading = $state(true);
  let error = $state("");
  let search = $state("");
  let searchRef: HTMLInputElement | undefined = $state();

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
    <div class="dialog-header">Review PR</div>
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
          <button class="pr-row" onclick={() => onSelect(pr.number)}>
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
          </button>
        {/each}
      {/if}
    </div>
    <div class="dialog-footer">
      <span class="footer-hint">Click a PR to create review workspace</span>
      <button class="cancel-btn" onclick={onCancel}>Cancel</button>
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
    flex-direction: column;
    gap: 0.2rem;
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
</style>

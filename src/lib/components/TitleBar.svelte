<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { Settings, ExternalLink, Check, X, Loader, Plus, GitPullRequestCreate, GitMerge, ArrowUp, AlertTriangle, Wrench, Eye } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Dropdown from "./Dropdown.svelte";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    prStatus: PrStatus | undefined;
    wsChanges: { additions: number; deletions: number } | undefined;
    onSelectRepo: (repo: RepoDetail) => void;
    onAddRepo: () => void;
    onSettings: () => void;
    onPrAction: () => void;
    onReview: () => void;
    reviewRunning: boolean;
    operationInProgress: boolean;
    highlightedRepoIndex: number;
    onDropdownClose?: () => void;
  }

  let { repos, activeRepo, selectedWs, prStatus, wsChanges, onSelectRepo, onAddRepo, onSettings, onPrAction, onReview, reviewRunning, operationInProgress, highlightedRepoIndex, onDropdownClose }: Props =
    $props();

  let dropdownRef: Dropdown | undefined = $state();

  function selectRepo(repo: RepoDetail) {
    onSelectRepo(repo);
    dropdownRef?.close();
  }

  function handleAddRepo() {
    onAddRepo();
    dropdownRef?.close();
  }

  function startDrag(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button, input, label, [role="button"]')) return;
    if (e.buttons === 1) {
      e.preventDefault();
      getCurrentWindow().startDragging();
    }
  }

  function handleDoubleClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button, input, label, [role="button"]')) return;
    getCurrentWindow().toggleMaximize();
  }

  export function toggleRepoDropdown() {
    dropdownRef?.toggle();
  }

  export function isRepoDropdownOpen() {
    return dropdownRef?.isOpen() ?? false;
  }

  export function closeRepoDropdown() {
    dropdownRef?.close();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="titlebar"
  class:dev={import.meta.env.DEV}
  onmousedown={startDrag}
  ondblclick={handleDoubleClick}
>
  <div class="titlebar-left">
    <div class="btn-group">
      <Dropdown bind:this={dropdownRef} onclose={onDropdownClose}>
        {#snippet trigger()}
          <span class="repo-name">{activeRepo.display_name}</span>
          <kbd class="shortcut-hint">⌘E</kbd>
        {/snippet}

        {#each repos as repo, i}
          <button
            class="dropdown-item"
            class:active={repo.id === activeRepo.id}
            class:highlighted={i === highlightedRepoIndex}
            onclick={() => selectRepo(repo)}
          >
            <span class="dropdown-item-name">{repo.display_name}</span>
            {#if repo.id === activeRepo.id}
              <Check size={12} class="check-mark" />
            {/if}
            {#if i < 9}
              <span class="shortcut-pill">{i + 1}</span>
            {/if}
          </button>
        {/each}
        <div class="dropdown-divider"></div>
        <button class="dropdown-item add-item" class:highlighted={highlightedRepoIndex === repos.length} onclick={handleAddRepo}>
          <Plus size={12} />
          <span>Add repository</span>
        </button>
      </Dropdown>
      <button class="settings-btn" onclick={onSettings} title="Repository settings">
        <Settings size={14} />
      </button>
    </div>
  </div>

  <div class="titlebar-center">
    {#if selectedWs}
      <span class="breadcrumb">
        <span class="breadcrumb-branch">{selectedWs.branch}</span>
        <span class="breadcrumb-sep">›</span>
        <span class="breadcrumb-base">{activeRepo.default_branch}</span>
      </span>
    {:else}
      <span class="breadcrumb">
        <span class="breadcrumb-base">{activeRepo.default_branch}</span>
      </span>
    {/if}
  </div>

  <div class="titlebar-right">
    {#if selectedWs}
      {#if prStatus?.state === "open"}
        <div class="btn-group">
          <button class="pr-link-btn" onclick={() => openUrl(prStatus!.url)} title="Open PR #{prStatus.number} in browser">
            <ExternalLink size={12} />
          </button>
          <button class="action-badge review" onclick={onReview} disabled={selectedWs.status === "running" || reviewRunning || operationInProgress}>
            <Eye size={11} /> Review
          </button>
          {#if prStatus.mergeable === "conflicting"}
            <button class="action-badge conflicts" onclick={onPrAction} disabled={operationInProgress}><AlertTriangle size={11} /> Conflicts</button>
          {:else if prStatus.checks === "failing"}
            <button class="action-badge checks-fail" onclick={onPrAction} disabled={operationInProgress}><Wrench size={11} /> Fix issues</button>
          {:else if prStatus.checks === "pending"}
            <span class="status-label checks-pending"><Loader size={10} class="status-icon spinning" /> Checks pending</span>
          {:else if (prStatus.ahead_by ?? 0) > 0}
            <button class="action-badge push-needed" onclick={onPrAction} disabled={operationInProgress}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<ArrowUp size={11} />{/if} Push</button>
          {:else if wsChanges && (wsChanges.additions !== prStatus.additions || wsChanges.deletions !== prStatus.deletions)}
            <button class="action-badge push-needed" onclick={onPrAction} disabled={operationInProgress}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<ArrowUp size={11} />{/if} Commit & push</button>
          {:else}
            <button class="action-badge mergeable" onclick={onPrAction} disabled={operationInProgress}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<GitMerge size={11} />{/if} Merge</button>
          {/if}
        </div>
      {:else if prStatus?.state === "merged"}
        <span class="status-label merged"><Check size={10} class="status-icon" /> Done</span>
      {:else if wsChanges && (wsChanges.additions > 0 || wsChanges.deletions > 0)}
        <button class="action-badge create-pr" onclick={onPrAction} disabled={selectedWs.status === "running" || operationInProgress}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<GitPullRequestCreate size={11} />{/if} Push & create PR</button>
      {/if}
    {/if}
  </div>
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0.75rem;
    height: 40px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-titlebar);
    -webkit-user-select: none;
    user-select: none;
    cursor: default;
    flex-shrink: 0;
    position: relative;
  }

  .titlebar.dev {
    background: #191726;
    border-bottom-color: #252238;
  }

  .titlebar-left {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    /* leave room for traffic lights on macOS */
    padding-left: 78px;
  }

  .titlebar-center {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    pointer-events: none;
  }

  .titlebar-center .breadcrumb {
    pointer-events: auto;
  }

  .titlebar-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .repo-name {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
    text-align: left;
  }

  .dropdown-item:hover,
  .dropdown-item.highlighted {
    background: var(--border);
    color: var(--text-primary);
  }

  .dropdown-item.active {
    color: var(--text-bright);
  }

  .dropdown-item-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-item :global(.check-mark) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .shortcut-pill {
    font-size: 0.6rem;
    font-weight: 600;
    min-width: 1.1rem;
    height: 1.1rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 3px;
    background: var(--border);
    color: var(--text-dim);
    flex-shrink: 0;
  }

  .shortcut-hint {
    font-size: 0.6rem;
    font-family: inherit;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    background: var(--border);
    color: var(--text-dim);
  }

  .dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 0.25rem 0;
  }

  .add-item {
    color: var(--text-dim);
  }

  .add-item:hover {
    color: var(--text-primary);
  }

  .breadcrumb {
    font-size: 0.75rem;
    color: var(--text-dim);
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .breadcrumb-branch {
    color: var(--accent);
  }

  .breadcrumb-sep {
    color: var(--text-muted);
  }

  .breadcrumb-base {
    color: var(--text-dim);
  }

  /* ── Button groups (segmented) ───────────────── */

  .btn-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--border-light);
    border-radius: 5px;
  }

  .btn-group :global(.dropdown-trigger) {
    border: none;
    border-radius: 4px 0 0 4px;
    border-right: 1px solid var(--border-light);
  }

  .pr-link-btn {
    background: var(--bg-card);
    color: var(--text-secondary);
    cursor: pointer;
    padding: 0.4rem 0.45rem;
    border: none;
    border-right: 1px solid var(--border-light);
    border-radius: 4px 0 0 4px;
    display: flex;
    align-items: center;
  }

  .pr-link-btn:hover {
    color: var(--text-primary);
    background: var(--border);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .settings-btn {
    background: var(--bg-card);
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.9rem;
    padding: 0.4rem 0.45rem;
    border: none;
    border-radius: 0 4px 4px 0;
    display: flex;
    align-items: center;
  }

  .settings-btn:hover {
    color: var(--text-primary);
    background: var(--border);
  }

  /* ── Action badges (clickable) ─────────────────── */

  .action-badge {
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.35rem 0.55rem;
    border-radius: 5px;
    border: 1px solid;
    cursor: pointer;
    font-family: inherit;
    background: transparent;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .btn-group .action-badge,
  .btn-group .status-label {
    border: none;
    border-radius: 0 4px 4px 0;
  }

  .action-badge.create-pr {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }

  .action-badge.create-pr:hover:not(:disabled) {
    filter: brightness(1.2);
  }

  .action-badge.create-pr:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .action-badge.push-needed {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }

  .action-badge.push-needed:hover {
    filter: brightness(1.2);
  }

  .action-badge.mergeable {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 40%, transparent);
    background: color-mix(in srgb, var(--status-ok) 7%, transparent);
  }

  .action-badge.mergeable:hover {
    filter: brightness(1.2);
  }

  .action-badge.checks-fail {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 40%, transparent);
    background: color-mix(in srgb, var(--diff-del) 7%, transparent);
  }

  .action-badge.checks-fail:hover {
    filter: brightness(1.2);
  }

  .action-badge.review {
    color: var(--text-secondary);
    border-color: var(--border-light);
    background: var(--bg-card);
    border-left: none;
    border-right: 1px solid var(--border-light);
    border-radius: 0;
  }

  .action-badge.review:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--border);
  }

  .action-badge.review:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .action-badge.conflicts {
    color: #c87e7e;
    border-color: color-mix(in srgb, #c87e7e 40%, transparent);
    background: color-mix(in srgb, #c87e7e 7%, transparent);
  }

  .action-badge.conflicts:hover {
    filter: brightness(1.2);
  }

  /* ── Status labels (non-interactive, no bg/border) */

  .status-label {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--text-dim);
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .btn-group .status-label {
    padding: 0.35rem 0.55rem;
  }

  .status-label.merged {
    color: var(--status-ok);
  }

  .status-label.checks-pending {
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .status-label :global(.status-icon) {
    flex-shrink: 0;
  }

  .status-label :global(.status-icon.spinning) {
    animation: spin 1.5s linear infinite;
  }

  @keyframes badge-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }
</style>

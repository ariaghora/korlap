<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo } from "$lib/ipc";
  import { syncMain, getRepoHead, checkoutDefaultBranch, checkMainBehind } from "$lib/ipc";
  import { Settings, Check, Plus, RefreshCw, AlertTriangle, ChevronLeft, Zap } from "lucide-svelte";
  import Dropdown from "./Dropdown.svelte";
  import { addToast } from "$lib/stores/toasts.svelte";

  type AppMode = "work" | "plan";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    appMode: AppMode;
    onModeChange: (mode: AppMode) => void;
    onSelectRepo: (repo: RepoDetail) => void;
    onSettings: () => void;
    onGoHome: () => void;
    highlightedRepoIndex: number;
    onDropdownClose?: () => void;
    autopilotEnabled?: boolean;
    onAutopilotToggle?: () => void;
    autopilotStatus?: string;
  }

  let { repos, activeRepo, selectedWs, appMode, onModeChange, onSelectRepo, onSettings, onGoHome, highlightedRepoIndex, onDropdownClose, autopilotEnabled = false, onAutopilotToggle, autopilotStatus }: Props =
    $props();

  let dropdownRef: Dropdown | undefined = $state();
  let syncing = $state(false);
  let syncError = $state(false);
  let headBranch: string | null = $state(null);
  let checkingOut = $state(false);
  let behindCount: number = $state(0);
  let checking = $state(false);

  let onDefaultBranch = $derived(
    headBranch === null || headBranch === activeRepo.default_branch
  );

  // Check HEAD branch + behind count when entering plan mode or switching repos
  $effect(() => {
    const repoId = activeRepo.id;
    const mode = appMode;
    if (mode === "plan") {
      checking = true;
      getRepoHead(repoId)
        .then((b) => {
          headBranch = b;
          // Only check behind if on default branch
          if (b === activeRepo.default_branch) {
            return checkMainBehind(repoId).then((n) => { behindCount = n; });
          } else {
            behindCount = 0;
          }
        })
        .catch(() => { headBranch = null; behindCount = 0; })
        .finally(() => { checking = false; });
    }
  });

  async function handleSync() {
    if (syncing) return;
    syncing = true;
    syncError = false;
    try {
      await syncMain(activeRepo.id);
      addToast(`${activeRepo.default_branch} synced with origin`, "success");
      behindCount = 0;
    } catch {
      syncError = true;
      addToast(`Failed to sync ${activeRepo.default_branch}`, "error");
      setTimeout(() => { syncError = false; }, 2000);
    } finally {
      syncing = false;
    }
  }

  async function handleCheckout() {
    if (checkingOut) return;
    checkingOut = true;
    try {
      await checkoutDefaultBranch(activeRepo.id);
      headBranch = activeRepo.default_branch;
    } catch {
      // leave state as-is so warning persists
    } finally {
      checkingOut = false;
    }
  }

  function selectRepo(repo: RepoDetail) {
    onSelectRepo(repo);
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
    <button class="home-btn" onclick={onGoHome} title="Home"><ChevronLeft size={14} strokeWidth={2.5} /></button>
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
        <button class="dropdown-item add-item" class:highlighted={highlightedRepoIndex === repos.length} onclick={() => { onGoHome(); dropdownRef?.close(); }}>
          <Plus size={12} />
          <span>Add repository</span>
        </button>
      </Dropdown>
      <button class="settings-btn" onclick={onSettings} title="Repository settings">
        <Settings size={14} />
      </button>
    </div>
    <div class="mode-switcher">
      <button class="mode-btn" class:active={appMode === "plan"} onclick={() => onModeChange("plan")}>
        Plan <kbd class="mode-hint">⌘1</kbd>
      </button>
      <button class="mode-btn" class:active={appMode === "work"} onclick={() => onModeChange("work")}>
        Work <kbd class="mode-hint">⌘2</kbd>
      </button>
    </div>
    <button
      class="autopilot-btn"
      class:active={autopilotEnabled}
      onclick={onAutopilotToggle}
      title="Toggle autopilot (⌘3)"
    >
      <Zap size={12} />
      Autopilot
      <kbd class="mode-hint">⌘3</kbd>
    </button>
  </div>

  <div class="titlebar-center">
    {#if appMode === "work" && selectedWs}
      <span class="breadcrumb">
        <span class="breadcrumb-branch">{selectedWs.branch}</span>
        <span class="breadcrumb-sep">›</span>
        <span class="breadcrumb-base">{activeRepo.default_branch}</span>
      </span>
    {/if}
  </div>

  <div class="titlebar-right">
    {#if autopilotEnabled && autopilotStatus}
      <span class="autopilot-status">{autopilotStatus}</span>
    {/if}
    {#if appMode === "plan"}
      {#if !onDefaultBranch}
        <div class="branch-warning">
          <AlertTriangle size={13} />
          <span class="branch-warning-text">
            on <strong>{headBranch}</strong>, not <strong>{activeRepo.default_branch}</strong>
          </span>
          <button
            class="checkout-btn"
            onclick={handleCheckout}
            disabled={checkingOut}
          >
            {checkingOut ? "Switching…" : `Switch to ${activeRepo.default_branch}`}
          </button>
        </div>
      {:else if behindCount > 0}
        <button
          class="sync-btn"
          class:syncing
          class:error={syncError}
          onclick={handleSync}
          disabled={syncing}
          title="Sync local {activeRepo.default_branch} with origin"
        >
          <RefreshCw size={13} />
          <span class="sync-label">
            {syncError ? "Failed" : syncing ? "Syncing…" : `Sync (${behindCount} behind)`}
          </span>
        </button>
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
    background: var(--bg-dev);
    border-bottom-color: var(--border-dev);
  }

  .titlebar-left {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    /* leave room for traffic lights on macOS */
    padding-left: 78px;
  }

  .home-btn {
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: var(--bg-active);
    border: 1px solid var(--border-light);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: border-color 0.15s ease;
  }

  .home-btn:hover {
    border-color: var(--accent);
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

  .mode-switcher {
    display: flex;
    align-items: stretch;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 5px;
    overflow: hidden;
  }

  .mode-btn {
    padding: 0.4rem 0.55rem;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    transition: background 0.15s, color 0.15s;
  }

  .mode-btn:hover:not(.active) {
    color: var(--text-secondary);
    background: var(--bg-hover);
  }

  .mode-btn.active {
    background: var(--bg-active);
    color: var(--accent);
  }

  .mode-btn + .mode-btn {
    border-left: 1px solid var(--border);
  }

  .mode-hint {
    font-size: 0.55rem;
    font-family: inherit;
    padding: 0.05rem 0.2rem;
    border-radius: 2px;
    background: var(--border);
    color: var(--text-muted);
  }

  .titlebar-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .sync-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem 0.55rem;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 5px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
    transition: color 0.15s, background 0.15s, border-color 0.15s;
  }

  .sync-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .sync-btn:disabled {
    cursor: default;
  }

  .sync-btn.syncing {
    color: var(--accent);
  }

  .sync-btn.syncing :global(svg) {
    animation: spin 0.8s linear infinite;
  }

  .sync-btn.error {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 25%, transparent);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .sync-label {
    white-space: nowrap;
  }

  .branch-warning {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.25rem 0.5rem;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 5px;
    color: var(--accent);
    font-size: 0.75rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .branch-warning :global(svg) {
    flex-shrink: 0;
  }

  .branch-warning-text {
    color: var(--accent);
  }

  .branch-warning-text strong {
    color: var(--text-bright);
  }

  .checkout-btn {
    padding: 0.2rem 0.5rem;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 4px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
    white-space: nowrap;
  }

  .checkout-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
    color: var(--text-bright);
  }

  .checkout-btn:disabled {
    opacity: 0.6;
    cursor: default;
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

  .autopilot-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem 0.55rem;
    background: transparent;
    border: 1px solid var(--border-light);
    border-radius: 5px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .autopilot-btn:hover:not(.active) {
    color: var(--text-secondary);
    background: var(--bg-hover);
  }

  .autopilot-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    animation: autopilot-glow 3s ease-in-out infinite;
  }

  @keyframes autopilot-glow {
    0%, 100% { box-shadow: none; }
    50% { box-shadow: 0 0 8px color-mix(in srgb, var(--accent) 25%, transparent); }
  }

  .autopilot-status {
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--accent);
    white-space: nowrap;
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

</style>

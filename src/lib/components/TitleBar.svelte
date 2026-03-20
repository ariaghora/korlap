<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo } from "$lib/ipc";
  import { Settings, Check, Plus } from "lucide-svelte";
  import Dropdown from "./Dropdown.svelte";

  type AppMode = "work" | "plan";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    appMode: AppMode;
    onModeChange: (mode: AppMode) => void;
    onSelectRepo: (repo: RepoDetail) => void;
    onAddRepo: () => void;
    onSettings: () => void;
    highlightedRepoIndex: number;
    onDropdownClose?: () => void;
  }

  let { repos, activeRepo, selectedWs, appMode, onModeChange, onSelectRepo, onAddRepo, onSettings, highlightedRepoIndex, onDropdownClose }: Props =
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
    <div class="mode-switcher">
      <button class="mode-btn" class:active={appMode === "plan"} onclick={() => onModeChange("plan")}>
        Plan <kbd class="mode-hint">⌘1</kbd>
      </button>
      <button class="mode-btn" class:active={appMode === "work"} onclick={() => onModeChange("work")}>
        Work <kbd class="mode-hint">⌘2</kbd>
      </button>
    </div>
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

  <div class="titlebar-right"></div>
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

<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { Settings, ExternalLink, Check, X, Loader, Plus } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Dropdown from "./Dropdown.svelte";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    prStatus: PrStatus | undefined;
    onSelectRepo: (repo: RepoDetail) => void;
    onAddRepo: () => void;
    onSettings: () => void;
  }

  let { repos, activeRepo, selectedWs, prStatus, onSelectRepo, onAddRepo, onSettings }: Props =
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
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="titlebar"
  class:dev={import.meta.env.DEV}
  onmousedown={startDrag}
  ondblclick={handleDoubleClick}
>
  <div class="titlebar-left">
    <Dropdown bind:this={dropdownRef}>
      {#snippet trigger()}
        <span class="repo-dot" class:has-running={false}></span>
        <span class="repo-name">{activeRepo.display_name}</span>
      {/snippet}

      {#each repos as repo}
        <button
          class="dropdown-item"
          class:active={repo.id === activeRepo.id}
          onclick={() => selectRepo(repo)}
        >
          <span class="repo-dot" class:has-running={false}></span>
          <span class="dropdown-item-name">{repo.display_name}</span>
          {#if repo.id === activeRepo.id}
            <Check size={12} class="check-mark" />
          {/if}
        </button>
      {/each}
      <div class="dropdown-divider"></div>
      <button class="dropdown-item add-item" onclick={handleAddRepo}>
        <Plus size={12} />
        <span>Add repository</span>
      </button>
    </Dropdown>
  </div>

  <div class="titlebar-right">
    {#if selectedWs}
      <span class="breadcrumb">
        {#if prStatus && prStatus.state === "open"}
          <button
            class="breadcrumb-pr"
            onclick={() => openUrl(prStatus!.url)}
            title="Open PR in browser"
          >
            #{prStatus.number}
            {#if prStatus.checks === "passing"}
              <Check size={11} class="check-icon pass" />
            {:else if prStatus.checks === "failing"}
              <X size={11} class="check-icon fail" />
            {:else if prStatus.checks === "pending"}
              <Loader size={11} class="check-icon pending" />
            {/if}
          </button>
          <span class="breadcrumb-sep">·</span>
        {/if}
        <span class="breadcrumb-branch">{selectedWs.branch}</span>
        <span class="breadcrumb-sep">›</span>
        <span class="breadcrumb-base">{activeRepo.default_branch}</span>
      </span>
    {:else}
      <span class="breadcrumb">
        <span class="breadcrumb-base">{activeRepo.default_branch}</span>
      </span>
    {/if}
    <button class="settings-btn" onclick={onSettings} title="Repository settings">
      <Settings size={14} />
    </button>
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

  .dropdown-item:hover {
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

  .repo-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--border-light);
    flex-shrink: 0;
  }

  .repo-dot.has-running {
    background: var(--accent);
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

  .breadcrumb-pr {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    padding: 0;
  }

  .breadcrumb-pr:hover {
    text-decoration: underline;
  }

  .breadcrumb-pr :global(.check-icon.pass) {
    color: var(--status-ok);
  }

  .breadcrumb-pr :global(.check-icon.fail) {
    color: var(--diff-del);
  }

  .breadcrumb-pr :global(.check-icon.pending) {
    color: var(--text-dim);
    animation: spin 1.5s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .settings-btn {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.9rem;
    padding: 0.2rem 0.35rem;
    border-radius: 4px;
  }

  .settings-btn:hover {
    color: var(--text-primary);
    background: var(--border);
  }
</style>

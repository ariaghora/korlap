<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo } from "$lib/ipc";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    onSelectRepo: (repo: RepoDetail) => void;
    onAddRepo: () => void;
  }

  let { repos, activeRepo, selectedWs, onSelectRepo, onAddRepo }: Props =
    $props();

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
<header class="titlebar" onmousedown={startDrag} ondblclick={handleDoubleClick}>
  <div class="titlebar-left">
    <div class="repo-tabs">
      {#each repos as repo}
        <button
          class="repo-tab"
          class:active={repo.id === activeRepo.id}
          onclick={() => onSelectRepo(repo)}
        >
          <span
            class="repo-dot"
            class:has-running={false}
          ></span>
          {repo.display_name}
        </button>
      {/each}
      <button class="repo-tab add-tab" onclick={onAddRepo}>+</button>
    </div>
  </div>

  <div class="titlebar-right">
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
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0.75rem;
    height: 40px;
    border-bottom: 1px solid #2a2520;
    background: #1a1611;
    -webkit-user-select: none;
    user-select: none;
    cursor: default;
    flex-shrink: 0;
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

  .repo-tabs {
    display: flex;
    gap: 0.25rem;
    align-items: center;
  }

  .repo-tab {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem 0.6rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .repo-tab:hover {
    color: #d4c5a9;
    background: #2a2520;
  }

  .repo-tab.active {
    color: #e8dcc8;
    background: #2a2520;
    border-color: #3a3530;
  }

  .add-tab {
    font-size: 1rem;
    padding: 0.2rem 0.5rem;
    color: #6a6050;
  }

  .repo-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #3a3530;
    flex-shrink: 0;
  }

  .repo-dot.has-running {
    background: #c8a97e;
  }

  .breadcrumb {
    font-size: 0.75rem;
    color: #6a6050;
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .breadcrumb-branch {
    color: #c8a97e;
  }

  .breadcrumb-sep {
    color: #4a4540;
  }

  .breadcrumb-base {
    color: #6a6050;
  }
</style>

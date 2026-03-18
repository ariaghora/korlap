<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { Check, X, Loader } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    selectedWs: WorkspaceInfo | undefined;
    prStatus: PrStatus | undefined;
    onSelectRepo: (repo: RepoDetail) => void;
    onAddRepo: () => void;
  }

  let { repos, activeRepo, selectedWs, prStatus, onSelectRepo, onAddRepo }: Props =
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
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .repo-tab:hover {
    color: var(--text-primary);
    background: var(--border);
  }

  .repo-tab.active {
    color: var(--text-bright);
    background: var(--border);
    border-color: var(--border-light);
  }

  .add-tab {
    font-size: 1rem;
    padding: 0.2rem 0.5rem;
    color: var(--text-dim);
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

</style>

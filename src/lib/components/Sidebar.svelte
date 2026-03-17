<script lang="ts">
  import type { WorkspaceInfo } from "$lib/ipc";

  interface Props {
    workspaces: WorkspaceInfo[];
    selectedWsId: string | null;
    onSelect: (wsId: string) => void;
    onNewWorkspace: () => void;
  }

  let { workspaces, selectedWsId, onSelect, onNewWorkspace }: Props = $props();

  let activeWorkspaces = $derived(
    workspaces.filter((w) => w.status !== "archived"),
  );
</script>

<aside class="sidebar">
  <div class="sidebar-header">
    <span class="sidebar-label">Workspaces</span>
  </div>
  <div class="workspace-list">
    {#each activeWorkspaces as ws}
      <button
        class="ws-item"
        class:active={ws.id === selectedWsId}
        onclick={() => onSelect(ws.id)}
      >
        <span
          class="ws-dot"
          class:running={ws.status === "running"}
          class:waiting={ws.status === "waiting"}
        ></span>
        <span class="ws-name">{ws.name}</span>
        {#if ws.status === "running"}
          <span class="ws-status">running</span>
        {/if}
      </button>
    {/each}
  </div>
  <button class="new-ws-btn" onclick={onNewWorkspace}>
    + New workspace
  </button>
</aside>

<style>
  .sidebar {
    width: 220px;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    background: var(--bg-sidebar);
    flex-shrink: 0;
  }

  .sidebar-header {
    padding: 0.6rem 0.75rem 0.3rem;
  }

  .sidebar-label {
    font-size: 0.7rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .workspace-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem;
  }

  .ws-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.5rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    text-align: left;
  }

  .ws-item:hover {
    background: var(--bg-hover);
  }

  .ws-item.active {
    background: var(--border);
    border-color: var(--border-light);
  }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--border-light);
  }

  .ws-dot.running {
    background: var(--accent);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 53%, transparent);
    animation: pulse 2s ease-in-out infinite;
  }

  .ws-dot.waiting {
    background: var(--status-ok);
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  .ws-status {
    font-size: 0.65rem;
    color: var(--accent);
    margin-left: auto;
  }

  .ws-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .new-ws-btn {
    margin: 0.5rem;
    padding: 0.4rem;
    background: transparent;
    border: 1px dashed var(--border-light);
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .new-ws-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--bg-hover);
  }
</style>

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
    border-right: 1px solid #2a2520;
    display: flex;
    flex-direction: column;
    background: #0f0d0a;
    flex-shrink: 0;
  }

  .sidebar-header {
    padding: 0.6rem 0.75rem 0.3rem;
  }

  .sidebar-label {
    font-size: 0.7rem;
    color: #6a6050;
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
    color: #d4c5a9;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    text-align: left;
  }

  .ws-item:hover {
    background: #1e1b17;
  }

  .ws-item.active {
    background: #2a2520;
    border-color: #3a3530;
  }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: #3a3530;
  }

  .ws-dot.running {
    background: #c8a97e;
    box-shadow: 0 0 6px #c8a97e88;
    animation: pulse 2s ease-in-out infinite;
  }

  .ws-dot.waiting {
    background: #7e9e6b;
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
    color: #c8a97e;
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
    border: 1px dashed #3a3530;
    border-radius: 4px;
    color: #6a6050;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .new-ws-btn:hover {
    color: #c8a97e;
    border-color: #c8a97e;
    background: #1e1b17;
  }
</style>

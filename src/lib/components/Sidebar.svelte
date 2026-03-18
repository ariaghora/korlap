<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";

  interface Props {
    workspaces: WorkspaceInfo[];
    selectedWsId: string | null;
    creatingWsId: string | null;
    prStatusMap: Map<string, PrStatus>;
    onSelect: (wsId: string) => void;
    onNewWorkspace: () => void;
    onRename: (wsId: string, newName: string) => void;
    onRemove: (wsId: string) => void;
  }

  let { workspaces, selectedWsId, creatingWsId, prStatusMap, onSelect, onNewWorkspace, onRename, onRemove }: Props =
    $props();

  let menuOpenId = $state<string | null>(null);

  function toggleMenu(e: MouseEvent, wsId: string) {
    e.stopPropagation();
    menuOpenId = menuOpenId === wsId ? null : wsId;
  }

  function handleRemoveClick(e: MouseEvent, wsId: string) {
    e.stopPropagation();
    menuOpenId = null;
    onRemove(wsId);
  }

  function handleWindowClick() {
    if (menuOpenId) menuOpenId = null;
  }

  let activeWorkspaces = $derived(
    [...workspaces].sort((a, b) => a.created_at - b.created_at),
  );

  type GroupKey = "ready" | "review" | "done";

  let groups = $derived.by(() => {
    const ready: WorkspaceInfo[] = [];
    const review: WorkspaceInfo[] = [];
    const done: WorkspaceInfo[] = [];

    for (const ws of activeWorkspaces) {
      const pr = prStatusMap.get(ws.id);
      if (pr?.state === "merged") {
        done.push(ws);
      } else if (pr?.state === "open") {
        review.push(ws);
      } else {
        ready.push(ws);
      }
    }

    const result: { key: GroupKey; label: string; items: WorkspaceInfo[] }[] = [];
    if (ready.length) result.push({ key: "ready", label: "Ready", items: ready });
    if (review.length) result.push({ key: "review", label: "Review", items: review });
    if (done.length) result.push({ key: "done", label: "Done", items: done });
    return result;
  });

  let editingId = $state<string | null>(null);
  let editValue = $state("");

  function startEdit(ws: WorkspaceInfo) {
    editingId = ws.id;
    editValue = ws.name;
  }

  function commitEdit(wsId: string) {
    const trimmed = editValue.trim();
    if (trimmed && editingId === wsId) {
      onRename(wsId, trimmed);
    }
    editingId = null;
  }

  function handleEditKeydown(e: KeyboardEvent, wsId: string) {
    if (e.key === "Enter") {
      e.preventDefault();
      commitEdit(wsId);
    } else if (e.key === "Escape") {
      editingId = null;
    }
  }
</script>

<svelte:window onclick={handleWindowClick} />

<aside class="sidebar">
  <div class="workspace-list">
    {#each groups as group}
      <div class="group">
        <div class="group-header">
          <span class="group-label">{group.label}</span>
          <span class="group-count">{group.items.length}</span>
        </div>
        {#each group.items as ws (ws.id)}
          <div class="ws-item-wrap">
            <button
              class="ws-item"
              class:active={ws.id === selectedWsId}
              onclick={() => onSelect(ws.id)}
              ondblclick={() => ws.id !== creatingWsId && startEdit(ws)}
            >
              <span
                class="ws-dot"
                class:creating={ws.id === creatingWsId}
                class:running={ws.id !== creatingWsId && ws.status === "running" && (!prStatusMap.get(ws.id) || prStatusMap.get(ws.id)?.state === "none")}
                class:waiting={ws.id !== creatingWsId && ws.status === "waiting" && (!prStatusMap.get(ws.id) || prStatusMap.get(ws.id)?.state === "none")}
                class:pr-open={prStatusMap.get(ws.id)?.state === "open" && prStatusMap.get(ws.id)?.mergeable !== "conflicting" && prStatusMap.get(ws.id)?.checks !== "failing"}
                class:pr-fail={prStatusMap.get(ws.id)?.state === "open" && (prStatusMap.get(ws.id)?.checks === "failing" || prStatusMap.get(ws.id)?.mergeable === "conflicting")}
                class:pr-merge={prStatusMap.get(ws.id)?.state === "open" && prStatusMap.get(ws.id)?.mergeable === "mergeable" && prStatusMap.get(ws.id)?.checks === "passing"}
              ></span>
              {#if editingId === ws.id}
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="ws-rename-input"
                  bind:value={editValue}
                  onblur={() => commitEdit(ws.id)}
                  onkeydown={(e) => handleEditKeydown(e, ws.id)}
                  onclick={(e) => e.stopPropagation()}
                  autofocus
                />
              {:else}
                <span class="ws-name" class:creating-name={ws.id === creatingWsId}>{ws.name}</span>
                {#if ws.id !== creatingWsId && ws.status === "running"}
                  <span class="ws-status">running</span>
                {/if}
              {/if}
            </button>
            {#if ws.id !== creatingWsId}
              <button
                class="ws-ellipsis"
                class:open={menuOpenId === ws.id}
                onclick={(e) => toggleMenu(e, ws.id)}
              >⋯</button>
              {#if menuOpenId === ws.id}
                <div class="ws-menu">
                  <button class="ws-menu-item remove" onclick={(e) => handleRemoveClick(e, ws.id)}>Remove</button>
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/each}
  </div>
  <button class="new-ws-btn" onclick={onNewWorkspace} disabled={!!creatingWsId}>
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

  .workspace-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem;
  }

  .group + .group {
    margin-top: 0.4rem;
  }

  .group-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.35rem 0.5rem 0.2rem;
  }

  .group-label {
    font-size: 0.65rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-weight: 500;
  }

  .group-count {
    font-size: 0.6rem;
    color: var(--text-dim);
    opacity: 0.6;
  }

  .ws-item-wrap {
    position: relative;
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

  .ws-item:hover,
  .ws-item-wrap:hover .ws-item {
    background: var(--bg-hover);
  }

  .ws-item.active {
    background: var(--border);
    border-color: var(--border-light);
  }

  .ws-ellipsis {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.85rem;
    letter-spacing: 0.05em;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.1s;
  }

  .ws-item-wrap:hover .ws-ellipsis,
  .ws-ellipsis.open {
    opacity: 1;
    pointer-events: auto;
  }

  .ws-item-wrap:hover .ws-ellipsis {
    background: var(--bg-hover);
  }

  .ws-item.active + .ws-ellipsis,
  .ws-item-wrap:hover .ws-item.active + .ws-ellipsis {
    background: var(--border);
  }

  .ws-ellipsis:hover {
    background: var(--bg-active) !important;
    color: var(--text-bright);
  }

  .ws-menu {
    position: absolute;
    right: 4px;
    top: 100%;
    z-index: 100;
    min-width: 110px;
    background: var(--bg-sidebar);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    padding: 0.25rem;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.35);
  }

  .ws-menu-item {
    width: 100%;
    display: block;
    padding: 0.35rem 0.6rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.78rem;
    text-align: left;
  }

  .ws-menu-item:hover {
    background: var(--bg-hover);
  }

  .ws-menu-item.remove:hover {
    color: var(--diff-del);
  }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--border-light);
  }

  .ws-dot.creating {
    background: var(--accent);
    animation: pulse 1s ease-in-out infinite;
  }

  .ws-dot.running {
    background: var(--accent);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 53%, transparent);
    animation: pulse 2s ease-in-out infinite;
  }

  .ws-dot.waiting {
    background: var(--status-ok);
  }

  .ws-dot.pr-open {
    background: #7e8ec8;
  }

  .ws-dot.pr-fail {
    background: var(--diff-del);
    box-shadow: 0 0 6px color-mix(in srgb, var(--diff-del) 50%, transparent);
  }

  .ws-dot.pr-merge {
    background: var(--status-ok);
    box-shadow: 0 0 6px color-mix(in srgb, var(--status-ok) 50%, transparent);
    animation: pulse 2s ease-in-out infinite;
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

  .ws-name.creating-name {
    color: var(--text-dim);
    font-style: italic;
  }

  .ws-rename-input {
    flex: 1;
    background: var(--bg-base);
    border: 1px solid var(--accent);
    border-radius: 3px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.82rem;
    padding: 0.1rem 0.3rem;
    outline: none;
    min-width: 0;
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

  .new-ws-btn:hover:not(:disabled) {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--bg-hover);
  }

  .new-ws-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>

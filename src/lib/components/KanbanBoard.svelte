<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import type { PastedImage } from "$lib/chat-utils";
  import KanbanColumn from "./KanbanColumn.svelte";
  import KanbanCard from "./KanbanCard.svelte";
  import CardDetailOverlay from "./CardDetailOverlay.svelte";
  import TaskPopover, { type TaskData } from "./TaskPopover.svelte";
  import AutopilotPill, { type AutopilotEvent } from "./AutopilotPill.svelte";
  import { Plus, Ellipsis, Trash2 } from "lucide-svelte";

  interface TodoItem {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    imagePaths?: string[];
    mentionPaths?: string[];
    planMode?: boolean;
    thinkingMode?: boolean;
    ready?: boolean;
    depends_on?: string[];
    created_at: number;
  }

  interface Props {
    todos: TodoItem[];
    inProgress: WorkspaceInfo[];
    review: WorkspaceInfo[];
    done: WorkspaceInfo[];
    prStatusMap: Map<string, PrStatus>;
    changeCounts: Map<string, { additions: number; deletions: number }>;
    reviewingWsIds: Set<string>;
    creatingWsId: string | null;
    repoId?: string;
    repoName?: string;
    defaultThinkingMode?: boolean;
    onCardClick: (wsId: string) => void;
    onSpawnAgent: (todoId: string) => void;
    onNewTodo: (data: TaskData) => void;
    onAddAndStart: (data: TaskData) => void;
    onEditTodo: (todoId: string, data: TaskData) => void;
    onRemoveTodo: (todoId: string) => void;
    onToggleReady: (todoId: string) => void;
    onRemoveWorkspace: (wsId: string) => void;
    onRemoveAllDone: () => void;
    autopilotEnabled?: boolean;
    autopilotEvents?: AutopilotEvent[];
    autopilotActiveAgents?: number;
    autopilotMaxAgents?: number;
    autopilotTodoQueue?: number;
    autopilotPrioritizing?: boolean;
    autopilotRebuildingStaging?: boolean;
    onAutopilotCommand?: (command: string) => void;
    active?: boolean;
  }

  let {
    todos,
    inProgress,
    review,
    done,
    prStatusMap,
    changeCounts,
    reviewingWsIds,
    creatingWsId,
    repoId,
    repoName,
    defaultThinkingMode = false,
    onCardClick,
    onSpawnAgent,
    onNewTodo,
    onAddAndStart,
    onEditTodo,
    onRemoveTodo,
    onToggleReady,
    onRemoveWorkspace,
    onRemoveAllDone,
    autopilotEnabled = false,
    autopilotEvents = [],
    autopilotActiveAgents = 0,
    autopilotMaxAgents = 3,
    autopilotTodoQueue = 0,
    autopilotPrioritizing = false,
    autopilotRebuildingStaging = false,
    onAutopilotCommand,
    active = false,
  }: Props = $props();

  let showAddDialog = $state(false);
  let editingTodo = $state<TodoItem | null>(null);
  let showDoneMenu = $state(false);
  let doneMenuBtnEl = $state<HTMLButtonElement | null>(null);
  let doneMenuPos = $state({ top: 0, left: 0 });
  let detailWs = $state<WorkspaceInfo | null>(null);

  // Keyboard navigation
  let focusedCol = $state(-1); // -1 = no focus
  let focusedRow = $state(0);
  let boardEl = $state<HTMLDivElement | null>(null);

  // Column data as indexable array: [todo, inProgress, review, done]
  const columnItems = $derived([todos, inProgress, review, done] as const);

  function colLen(col: number): number {
    return columnItems[col]?.length ?? 0;
  }

  // Clamp focus when data changes (e.g. card removed)
  function ensureValidFocus() {
    if (focusedCol < 0) return;
    const len = colLen(focusedCol);
    if (len === 0) {
      focusedCol = -1;
      focusedRow = 0;
    } else {
      focusedRow = Math.min(focusedRow, len - 1);
    }
  }

  function findFirstNonEmptyCol(startDir: "forward" | "backward"): number {
    if (startDir === "forward") {
      for (let i = 0; i < 4; i++) { if (colLen(i) > 0) return i; }
    } else {
      for (let i = 3; i >= 0; i--) { if (colLen(i) > 0) return i; }
    }
    return -1;
  }

  function handleBoardKeydown(e: KeyboardEvent) {
    if (!active) return;
    if (e.defaultPrevented) return;
    if (showAddDialog || editingTodo || detailWs || showDoneMenu) return;

    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) return;

    const key = e.key;
    if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Enter", "Escape"].includes(key)) return;

    e.preventDefault();
    ensureValidFocus();

    switch (key) {
      case "ArrowDown":
      case "ArrowUp": {
        if (focusedCol === -1) {
          const col = findFirstNonEmptyCol("forward");
          if (col >= 0) {
            focusedCol = col;
            focusedRow = key === "ArrowDown" ? 0 : colLen(col) - 1;
          }
        } else {
          if (key === "ArrowDown") {
            focusedRow = Math.min(focusedRow + 1, colLen(focusedCol) - 1);
          } else {
            focusedRow = Math.max(focusedRow - 1, 0);
          }
        }
        break;
      }
      case "ArrowRight":
      case "ArrowLeft": {
        if (focusedCol === -1) {
          const col = findFirstNonEmptyCol(key === "ArrowRight" ? "forward" : "backward");
          if (col >= 0) { focusedCol = col; focusedRow = 0; }
        } else {
          const dir = key === "ArrowRight" ? 1 : -1;
          let next = focusedCol + dir;
          while (next >= 0 && next < 4) {
            if (colLen(next) > 0) {
              focusedCol = next;
              focusedRow = Math.min(focusedRow, colLen(next) - 1);
              break;
            }
            next += dir;
          }
        }
        break;
      }
      case "Enter": {
        if (focusedCol < 0) return;
        if (focusedCol === 0) {
          const todo = todos[focusedRow];
          if (todo) editingTodo = todo;
        } else {
          const lists = [null, inProgress, review, done];
          const ws = lists[focusedCol]?.[focusedRow];
          if (ws) {
            if (e.metaKey) onCardClick(ws.id);
            else detailWs = ws;
          }
        }
        break;
      }
      case "Escape": {
        focusedCol = -1;
        focusedRow = 0;
        break;
      }
    }

    // Scroll focused card into view
    if (focusedCol >= 0) {
      requestAnimationFrame(() => {
        boardEl?.querySelector(".card.focused")?.scrollIntoView({ block: "nearest", behavior: "smooth" });
      });
    }
  }

  function openDoneMenu(e: MouseEvent) {
    e.stopPropagation();
    if (doneMenuBtnEl) {
      const rect = doneMenuBtnEl.getBoundingClientRect();
      doneMenuPos = { top: rect.bottom + 4, left: rect.right };
    }
    showDoneMenu = !showDoneMenu;
  }

  function handleAddSubmit(data: TaskData) {
    onNewTodo(data);
    showAddDialog = false;
  }

  function handleAddAndStartSubmit(data: TaskData) {
    onAddAndStart(data);
    showAddDialog = false;
  }

  function handleEditSubmit(data: TaskData) {
    if (editingTodo) {
      onEditTodo(editingTodo.id, data);
      editingTodo = null;
    }
  }
</script>

<svelte:window onkeydown={handleBoardKeydown} />

<div class="kanban-wrapper">
<div class="kanban-board" bind:this={boardEl}>
  <KanbanColumn title="Todo" count={todos.length}>
    {#each todos as todo, i (todo.id)}
      <KanbanCard
        type="todo"
        todoId={todo.id}
        title={todo.title}
        description={todo.description}
        imagePaths={todo.imagePaths}
        planMode={todo.planMode}
        thinkingMode={todo.thinkingMode}
        ready={todo.ready ?? false}
        focused={focusedCol === 0 && focusedRow === i}
        {repoName}
        onAction={() => onSpawnAgent(todo.id)}
        onEdit={() => { editingTodo = todo; }}
        onRemove={() => onRemoveTodo(todo.id)}
        onToggleReady={() => onToggleReady(todo.id)}
      />
    {/each}
    {#if todos.length === 0}
      <div class="empty-hint">Add a task to get started</div>
    {/if}
    {#snippet footer()}
      <button class="add-task-btn" onclick={() => { showAddDialog = true; }}>
        <Plus size={12} /> New task
      </button>
    {/snippet}
  </KanbanColumn>

  <KanbanColumn title="In Progress" count={inProgress.length}>
    {#each inProgress as ws, i (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        isReviewing={reviewingWsIds.has(ws.id)}
        isCreating={ws.id === creatingWsId}
        focused={focusedCol === 1 && focusedRow === i}
        onClick={(e) => { e.metaKey ? onCardClick(ws.id) : detailWs = ws; }}
        onRemove={() => onRemoveWorkspace(ws.id)}
      />
    {/each}
    {#if inProgress.length === 0}
      <div class="empty-hint">No agents running</div>
    {/if}
  </KanbanColumn>

  <KanbanColumn title="Review" count={review.length} accent={review.length > 0}>
    {#each review as ws, i (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        isReviewing={reviewingWsIds.has(ws.id)}
        focused={focusedCol === 2 && focusedRow === i}
        onClick={(e) => { e.metaKey ? onCardClick(ws.id) : detailWs = ws; }}
        onRemove={() => onRemoveWorkspace(ws.id)}
      />
    {/each}
    {#if review.length === 0}
      <div class="empty-hint">Nothing to review</div>
    {/if}
  </KanbanColumn>

  <KanbanColumn title="Done" count={done.length} dimmed>
    {#each done as ws, i (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        focused={focusedCol === 3 && focusedRow === i}
        onClick={(e) => { e.metaKey ? onCardClick(ws.id) : detailWs = ws; }}
        onRemove={() => onRemoveWorkspace(ws.id)}
      />
    {/each}
    {#if done.length === 0}
      <div class="empty-hint">Completed tasks appear here</div>
    {/if}
    {#snippet headerAction()}
      {#if done.length > 0}
        <button
          class="column-menu-btn"
          bind:this={doneMenuBtnEl}
          onclick={openDoneMenu}
        >
          <Ellipsis size={14} />
        </button>
      {/if}
    {/snippet}
  </KanbanColumn>
</div>

<AutopilotPill
  enabled={autopilotEnabled}
  events={autopilotEvents}
  activeAgentCount={autopilotActiveAgents}
  maxAgents={autopilotMaxAgents}
  todoQueueLength={autopilotTodoQueue}
  prioritizing={autopilotPrioritizing}
  rebuildingStaging={autopilotRebuildingStaging}
  onSendCommand={(cmd) => onAutopilotCommand?.(cmd)}
  onCardClick={onCardClick}
/>
</div>

{#if showAddDialog}
  <TaskPopover
    {repoId}
    initialThinkingMode={defaultThinkingMode}
    submitLabel="Add"
    onSubmit={handleAddSubmit}
    onSubmitAndStart={handleAddAndStartSubmit}
    onCancel={() => { showAddDialog = false; }}
  />
{/if}

{#if editingTodo}
  <TaskPopover
    {repoId}
    initialTitle={editingTodo.title}
    initialDescription={editingTodo.description}
    initialImagePaths={editingTodo.imagePaths}
    initialMentions={editingTodo.mentionPaths?.map((p: string) => ({ type: "file" as const, path: p, displayName: p.split("/").pop() ?? p }))}
    initialPlanMode={editingTodo.planMode}
    initialThinkingMode={editingTodo.thinkingMode}
    submitLabel="Save"
    onSubmit={handleEditSubmit}
    onCancel={() => { editingTodo = null; }}
  />
{/if}

{#if detailWs}
  <CardDetailOverlay
    workspace={detailWs}
    prStatus={prStatusMap.get(detailWs.id)}
    changeCounts={changeCounts.get(detailWs.id)}
    isReviewing={reviewingWsIds.has(detailWs.id)}
    isCreating={detailWs.id === creatingWsId}
    onGoToWorkspace={() => { const id = detailWs!.id; detailWs = null; onCardClick(id); }}
    onClose={() => { detailWs = null; }}
  />
{/if}

{#if showDoneMenu}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="dropdown-backdrop" onmousedown={() => { showDoneMenu = false; }}></div>
  <div class="dropdown-menu" style="top: {doneMenuPos.top}px; left: {doneMenuPos.left}px;">
    <button
      class="dropdown-item danger"
      onclick={() => { showDoneMenu = false; onRemoveAllDone(); }}
    >
      <Trash2 size={12} />
      Remove all
    </button>
  </div>
{/if}

<style>
  .kanban-wrapper {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .kanban-board {
    display: flex;
    gap: 0.75rem;
    padding: 0.75rem;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }

  .empty-hint {
    font-size: 0.72rem;
    color: var(--text-muted);
    text-align: center;
    padding: 1.5rem 0.5rem;
  }

  .add-task-btn {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.3rem;
    padding: 0.4rem;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .add-task-btn:hover {
    color: var(--text-primary);
    background: var(--btn-subtle-hover);
  }

  .column-menu-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    opacity: 0.6;
  }

  .column-menu-btn:hover {
    background: var(--border);
    opacity: 1;
  }

  .dropdown-backdrop {
    position: fixed;
    inset: 0;
    z-index: 99;
  }

  .dropdown-menu {
    position: fixed;
    transform: translateX(-100%);
    min-width: 140px;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 4px;
    z-index: 100;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.35rem 0.5rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    text-align: left;
  }

  .dropdown-item:hover {
    background: var(--border);
    color: var(--text);
  }

  .dropdown-item.danger:hover {
    background: color-mix(in srgb, #e05252 15%, transparent);
    color: #e05252;
  }
</style>

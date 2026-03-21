<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import type { PastedImage } from "$lib/chat-utils";
  import KanbanColumn from "./KanbanColumn.svelte";
  import KanbanCard from "./KanbanCard.svelte";
  import CardDetailOverlay from "./CardDetailOverlay.svelte";
  import TaskPopover, { type TaskData } from "./TaskPopover.svelte";
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
    onEditTodo: (todoId: string, data: TaskData) => void;
    onRemoveTodo: (todoId: string) => void;
    onRemoveWorkspace: (wsId: string) => void;
    onRemoveAllDone: () => void;
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
    onEditTodo,
    onRemoveTodo,
    onRemoveWorkspace,
    onRemoveAllDone,
  }: Props = $props();

  let showAddDialog = $state(false);
  let editingTodo = $state<TodoItem | null>(null);
  let showDoneMenu = $state(false);
  let doneMenuBtnEl = $state<HTMLButtonElement | null>(null);
  let doneMenuPos = $state({ top: 0, left: 0 });
  let detailWs = $state<WorkspaceInfo | null>(null);

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

  function handleEditSubmit(data: TaskData) {
    if (editingTodo) {
      onEditTodo(editingTodo.id, data);
      editingTodo = null;
    }
  }
</script>

<div class="kanban-board">
  <KanbanColumn title="Todo" count={todos.length}>
    {#each todos as todo (todo.id)}
      <KanbanCard
        type="todo"
        todoId={todo.id}
        title={todo.title}
        description={todo.description}
        imagePaths={todo.imagePaths}
        planMode={todo.planMode}
        thinkingMode={todo.thinkingMode}
        {repoName}
        onAction={() => onSpawnAgent(todo.id)}
        onEdit={() => { editingTodo = todo; }}
        onRemove={() => onRemoveTodo(todo.id)}
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
    {#each inProgress as ws (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        isReviewing={reviewingWsIds.has(ws.id)}
        isCreating={ws.id === creatingWsId}
        onClick={() => { detailWs = ws; }}
      />
    {/each}
    {#if inProgress.length === 0}
      <div class="empty-hint">No agents running</div>
    {/if}
  </KanbanColumn>

  <KanbanColumn title="Review" count={review.length} accent={review.length > 0}>
    {#each review as ws (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        isReviewing={reviewingWsIds.has(ws.id)}
        onClick={() => { detailWs = ws; }}
      />
    {/each}
    {#if review.length === 0}
      <div class="empty-hint">Nothing to review</div>
    {/if}
  </KanbanColumn>

  <KanbanColumn title="Done" count={done.length} dimmed>
    {#each done as ws (ws.id)}
      <KanbanCard
        type="workspace"
        workspace={ws}
        prStatus={prStatusMap.get(ws.id)}
        changeCounts={changeCounts.get(ws.id)}
        onClick={() => { detailWs = ws; }}
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

{#if showAddDialog}
  <TaskPopover
    {repoId}
    initialThinkingMode={defaultThinkingMode}
    submitLabel="Add"
    onSubmit={handleAddSubmit}
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
  .kanban-board {
    display: flex;
    gap: 0.75rem;
    padding: 0.75rem;
    flex: 1;
    min-height: 0;
    overflow-x: auto;
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

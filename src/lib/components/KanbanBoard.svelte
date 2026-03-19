<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import type { PastedImage } from "./ChatPanel.svelte";
  import KanbanColumn from "./KanbanColumn.svelte";
  import KanbanCard from "./KanbanCard.svelte";
  import TaskPopover, { type TaskData } from "./TaskPopover.svelte";
  import { Plus } from "lucide-svelte";

  interface TodoItem {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    imagePaths?: string[];
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
    repoName?: string;
    onCardClick: (wsId: string) => void;
    onSpawnAgent: (todoId: string) => void;
    onNewTodo: (data: TaskData) => void;
    onEditTodo: (todoId: string, data: TaskData) => void;
    onRemoveTodo: (todoId: string) => void;
    onRemoveWorkspace: (wsId: string) => void;
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
    repoName,
    onCardClick,
    onSpawnAgent,
    onNewTodo,
    onEditTodo,
    onRemoveTodo,
    onRemoveWorkspace,
  }: Props = $props();

  let showAddDialog = $state(false);
  let editingTodo = $state<TodoItem | null>(null);

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
        onClick={() => onCardClick(ws.id)}
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
        onClick={() => onCardClick(ws.id)}
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
        onClick={() => onCardClick(ws.id)}
      />
    {/each}
    {#if done.length === 0}
      <div class="empty-hint">Completed tasks appear here</div>
    {/if}
  </KanbanColumn>
</div>

{#if showAddDialog}
  <TaskPopover
    submitLabel="Add"
    onSubmit={handleAddSubmit}
    onCancel={() => { showAddDialog = false; }}
  />
{/if}

{#if editingTodo}
  <TaskPopover
    initialTitle={editingTodo.title}
    initialDescription={editingTodo.description}
    initialImagePaths={editingTodo.imagePaths}
    submitLabel="Save"
    onSubmit={handleEditSubmit}
    onCancel={() => { editingTodo = null; }}
  />
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
    background: transparent;
    border: 1px dashed var(--border-light);
    border-radius: 4px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }

  .add-task-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }
</style>

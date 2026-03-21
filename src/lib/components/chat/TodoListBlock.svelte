<script lang="ts">
  import { ListChecks, CheckCircle2, Circle, Loader } from "lucide-svelte";
  import type { MessageChunk } from "$lib/stores/messages.svelte";

  interface TodoItem {
    id?: string;
    content: string;
    status: "pending" | "in_progress" | "completed";
    priority?: "high" | "medium" | "low";
  }

  interface Props {
    chunk: MessageChunk & { type: "tool" };
    isLatest: boolean;
    collapsed: boolean;
    onToggle: () => void;
  }

  let { chunk, isLatest, collapsed, onToggle }: Props = $props();

  let todos = $derived((() => {
    try {
      const parsed = JSON.parse(chunk.input);
      return Array.isArray(parsed) ? parsed as TodoItem[] : [];
    } catch {
      return null;
    }
  })());

  let completedCount = $derived(todos ? todos.filter(t => t.status === "completed").length : 0);
  let inProgressCount = $derived(todos ? todos.filter(t => t.status === "in_progress").length : 0);
  let totalCount = $derived(todos ? todos.length : 0);
</script>

<div class="todo-block">
  <button class="todo-header" onclick={onToggle}>
    <span class="todo-chevron" class:collapsed>▾</span>
    <span class="todo-icon"><ListChecks size={13} strokeWidth={2} /></span>
    <span class="todo-label">Task progress</span>
    {#if todos && totalCount > 0}
      <span class="todo-count">{completedCount}/{totalCount} completed</span>
    {/if}
  </button>

  {#if !collapsed}
    {#if todos === null}
      <div class="todo-body">
        <span class="todo-fallback">{chunk.input}</span>
      </div>
    {:else if todos.length === 0}
      <div class="todo-body">
        <span class="todo-empty">No tasks</span>
      </div>
    {:else}
      <div class="todo-progress-bar">
        {#each todos as todo, i (todo.id ?? i)}
          <div
            class="todo-segment"
            class:completed={todo.status === "completed"}
            class:in-progress={todo.status === "in_progress"}
            style="flex:{1}"
          ></div>
        {/each}
      </div>
      <div class="todo-body">
        {#each todos as todo, i (todo.id ?? i)}
          <div class="todo-item" class:completed={todo.status === "completed"}>
            {#if todo.priority === "high"}
              <span class="todo-priority-dot high"></span>
            {/if}
            <span class="todo-status-icon">
              {#if todo.status === "completed"}
                <CheckCircle2 size={14} strokeWidth={2} />
              {:else if todo.status === "in_progress"}
                <Loader size={14} strokeWidth={2} />
              {:else}
                <Circle size={14} strokeWidth={1.5} />
              {/if}
            </span>
            <span class="todo-content">{todo.content}</span>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .todo-block {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-card);
  }

  .todo-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.4rem 0.7rem;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 0.73rem;
    text-align: left;
  }

  .todo-header:hover {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .todo-chevron {
    font-size: 0.65rem;
    opacity: 0.5;
    transition: transform 0.15s ease;
  }

  .todo-chevron.collapsed {
    transform: rotate(-90deg);
  }

  .todo-icon {
    display: flex;
    align-items: center;
    opacity: 0.6;
    color: var(--accent);
  }

  .todo-label {
    color: var(--text-secondary);
  }

  .todo-count {
    margin-left: auto;
    color: var(--text-dim);
    font-size: 0.7rem;
  }

  .todo-progress-bar {
    display: flex;
    height: 3px;
    gap: 1px;
    background: var(--bg-hover);
  }

  .todo-segment {
    background: var(--bg-hover);
    transition: background 0.2s ease;
  }

  .todo-segment.completed {
    background: var(--status-ok, #7e9e6b);
  }

  .todo-segment.in-progress {
    background: var(--accent);
  }

  .todo-body {
    overflow-y: auto;
    max-height: 300px;
    padding: 0.35rem 0;
  }

  .todo-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.2rem 0.7rem;
    font-size: 0.78rem;
    color: var(--text-primary);
  }

  .todo-item.completed {
    color: var(--text-dim);
  }

  .todo-item.completed .todo-content {
    text-decoration: line-through;
    opacity: 0.6;
  }

  .todo-priority-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .todo-priority-dot.high {
    background: var(--status-fail, #b5564e);
  }

  .todo-status-icon {
    display: flex;
    align-items: center;
    flex-shrink: 0;
  }

  .todo-item:not(.completed) .todo-status-icon {
    color: var(--text-dim);
  }

  .todo-item.completed .todo-status-icon {
    color: var(--status-ok, #7e9e6b);
  }

  /* in_progress items get amber spinning icon */
  .todo-item:not(.completed) :global(.lucide-loader) {
    color: var(--accent);
    animation: spin 2s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .todo-content {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .todo-fallback {
    padding: 0.4rem 0.7rem;
    font-family: var(--font-mono);
    font-size: 0.73rem;
    color: var(--text-dim);
    word-break: break-all;
  }

  .todo-empty {
    padding: 0.4rem 0.7rem;
    font-size: 0.75rem;
    color: var(--text-dim);
    font-style: italic;
  }
</style>

<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { Play, X, Pencil, Lightbulb, BookOpen } from "lucide-svelte";

  interface Props {
    type: "todo" | "workspace";
    // Todo fields
    todoId?: string;
    title?: string;
    description?: string;
    imagePaths?: string[];
    planMode?: boolean;
    thinkingMode?: boolean;
    repoName?: string;
    // Workspace fields
    workspace?: WorkspaceInfo;
    prStatus?: PrStatus;
    changeCounts?: { additions: number; deletions: number };
    isReviewing?: boolean;
    isCreating?: boolean;
    // Common
    onClick?: () => void;
    onAction?: () => void;
    onEdit?: () => void;
    onRemove?: () => void;
  }

  let {
    type,
    todoId,
    title,
    description,
    imagePaths,
    planMode = false,
    thinkingMode = false,
    repoName,
    workspace,
    prStatus,
    changeCounts,
    isReviewing = false,
    isCreating = false,
    onClick,
    onAction,
    onEdit,
    onRemove,
  }: Props = $props();

  let elapsed = $state("");
  let interval: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    if (type === "workspace" && workspace?.status === "running") {
      const start = workspace.created_at * 1000;
      function update() {
        const mins = Math.floor((Date.now() - start) / 60000);
        elapsed = mins < 1 ? "<1m" : `${mins}m`;
      }
      update();
      interval = setInterval(update, 30000);
      return () => { if (interval) clearInterval(interval); };
    } else {
      elapsed = "";
      if (interval) { clearInterval(interval); interval = undefined; }
    }
  });
</script>

{#if type === "todo"}
  <div class="card todo-card">
    <span class="card-title">{title}</span>
    {#if description}
      <span class="card-desc">{description}</span>
    {/if}
    {#if planMode || thinkingMode}
      <div class="card-mode-badges">
        {#if thinkingMode}
          <span class="card-mode-badge"><Lightbulb size={10} strokeWidth={2} /> Thinking</span>
        {/if}
        {#if planMode}
          <span class="card-mode-badge"><BookOpen size={10} strokeWidth={2} /> Plan</span>
        {/if}
      </div>
    {/if}
    {#if imagePaths && imagePaths.length > 0}
      <div class="card-images">
        {#each imagePaths as path (path)}
          <img class="card-image-thumb" src={convertFileSrc(path)} alt="Attached" />
        {/each}
      </div>
    {/if}
    <div class="card-actions">
      <button class="spawn-btn" onclick={onAction} title="Start agent">
        <Play size={11} /> Start
      </button>
      {#if onEdit}
        <button class="edit-btn" onclick={onEdit} title="Edit">
          <Pencil size={11} />
        </button>
      {/if}
      <button class="remove-btn" onclick={onRemove} title="Remove">
        <X size={11} />
      </button>
    </div>
  </div>
{:else if workspace}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="card ws-card" onclick={onClick}>
    <div class="card-top" class:has-title={!!workspace.task_title}>
      <span
        class="ws-dot"
        class:running={workspace.status === "running" && !isReviewing}
        class:reviewing={isReviewing}
        class:creating={isCreating}
      ></span>
      <span class="card-name" class:has-title={!!workspace.task_title}>{workspace.task_title ?? workspace.name}</span>
      {#if workspace.status === "running" && !isReviewing}
        <span class="card-elapsed">{elapsed}</span>
      {/if}
    </div>
    {#if workspace.task_description}
      <div class="card-task-desc">{workspace.task_description}</div>
    {/if}
    <div class="card-bottom">
      <span class="card-branch">{workspace.task_title ? workspace.branch : (workspace.name !== workspace.branch ? workspace.branch : "")}</span>
      {#if changeCounts && (changeCounts.additions > 0 || changeCounts.deletions > 0)}
        <span class="card-diff">
          <span class="diff-add">+{changeCounts.additions}</span>
          <span class="diff-del">−{changeCounts.deletions}</span>
        </span>
      {/if}
    </div>
    {#if isReviewing}
      <div class="card-review-badge">reviewing</div>
    {/if}
  </div>
{/if}

<style>
  .card {
    padding: 0.6rem 0.65rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .ws-card {
    cursor: pointer;
    transition: border-color 0.15s;
  }

  .ws-card:hover {
    border-color: var(--border-light);
    background: var(--bg-hover);
  }

  .todo-card {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .card-title {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--text-bright);
    line-height: 1.3;
    word-break: break-word;
  }

  .card-desc {
    font-size: 0.72rem;
    color: var(--text-secondary);
    line-height: 1.35;
    white-space: pre-wrap;
    word-break: break-word;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-mode-badges {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }

  .card-mode-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    font-size: 0.62rem;
    font-weight: 500;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    border-radius: 8px;
    padding: 0.05rem 0.35rem;
    line-height: 1.3;
  }

  .card-images {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
    margin-top: 0.1rem;
  }

  .card-image-thumb {
    width: 36px;
    height: 36px;
    object-fit: cover;
    border-radius: 4px;
    border: 1px solid var(--border-light);
  }

  .card-actions {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin-top: 0.2rem;
  }

  .spawn-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.3rem 0.55rem;
    background: transparent;
    border: 1px dashed var(--border-light);
    border-radius: 4px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.7rem;
    cursor: pointer;
  }

  .spawn-btn:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .edit-btn {
    display: inline-flex;
    align-items: center;
    padding: 0.3rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .edit-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .remove-btn {
    display: inline-flex;
    align-items: center;
    padding: 0.3rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    margin-left: auto;
  }

  .remove-btn:hover {
    color: var(--diff-del);
    background: var(--bg-hover);
  }

  .card-top {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }

  .card-top.has-title {
    align-items: flex-start;
  }

  .card-top.has-title .ws-dot {
    margin-top: 0.35rem;
  }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: var(--status-ok);
  }

  .ws-dot.running {
    background: var(--accent);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 50%, transparent);
    animation: pulse 2s ease-in-out infinite;
  }

  .ws-dot.reviewing {
    background: var(--accent);
    animation: pulse-slow 3s ease-in-out infinite;
  }

  .ws-dot.creating {
    background: var(--accent);
    animation: pulse 1s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  @keyframes pulse-slow {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .card-name {
    flex: 1;
    font-size: 0.8rem;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-name.has-title {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--text-bright);
    line-height: 1.3;
    word-break: break-word;
    white-space: normal;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .card-elapsed {
    font-size: 0.65rem;
    color: var(--text-dim);
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .card-task-desc {
    font-size: 0.72rem;
    color: var(--text-secondary);
    line-height: 1.35;
    white-space: pre-wrap;
    word-break: break-word;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    margin-top: 0.2rem;
    padding-left: calc(6px + 0.4rem);
  }

  .card-bottom {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-top: 0.3rem;
    padding-left: calc(6px + 0.4rem); /* align with name after dot */
  }

  .card-branch {
    font-size: 0.65rem;
    font-family: var(--font-mono);
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .card-diff {
    font-size: 0.62rem;
    font-family: var(--font-mono);
    display: flex;
    gap: 0.3rem;
    flex-shrink: 0;
    margin-left: auto;
  }

  .diff-add { color: var(--diff-add); }
  .diff-del { color: var(--diff-del); }

  .card-review-badge {
    margin-top: 0.35rem;
    padding-left: calc(6px + 0.4rem);
    font-size: 0.62rem;
    color: var(--accent);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
</style>

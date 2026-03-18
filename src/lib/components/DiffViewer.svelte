<script lang="ts">
  import { getChangedFiles, getDiff, type ChangedFile } from "$lib/ipc";

  interface Props {
    workspaceId: string;
    refreshTrigger?: number;
    prState?: string;
    onCreatePr?: () => void;
    disabled?: boolean;
  }

  let { workspaceId, refreshTrigger = 0, prState, onCreatePr, disabled = false }: Props = $props();

  let files = $state<ChangedFile[]>([]);
  let selectedFile = $state<string | null>(null);
  let diff = $state("");
  let loading = $state(false);
  let error = $state("");

  async function loadFiles() {
    loading = true;
    error = "";
    try {
      files = await getChangedFiles(workspaceId);
      if (files.length === 0) {
        selectedFile = null;
        diff = "";
      } else if (selectedFile && files.some((f) => f.path === selectedFile)) {
        // Reload current file's diff
        diff = await getDiff(workspaceId, selectedFile);
      } else {
        // Selected file gone or no selection — pick first
        await selectFile(files[0].path);
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function selectFile(path: string) {
    selectedFile = path;
    try {
      diff = await getDiff(workspaceId, path);
    } catch (e) {
      diff = `Error: ${e}`;
    }
  }

  $effect(() => {
    workspaceId;
    refreshTrigger;
    loadFiles();
  });

  interface DiffLine {
    type: "add" | "remove" | "context" | "header" | "hunk";
    text: string;
  }

  function parseDiff(raw: string): DiffLine[] {
    if (!raw.trim()) return [];
    return raw.split("\n").map((line) => {
      if (
        line.startsWith("+++") ||
        line.startsWith("---") ||
        line.startsWith("diff ") ||
        line.startsWith("index ")
      ) {
        return { type: "header" as const, text: line };
      }
      if (line.startsWith("@@")) {
        return { type: "hunk" as const, text: line };
      }
      if (line.startsWith("+")) {
        return { type: "add" as const, text: line };
      }
      if (line.startsWith("-")) {
        return { type: "remove" as const, text: line };
      }
      return { type: "context" as const, text: line };
    });
  }

  let lines = $derived(parseDiff(diff));

  let totalAdditions = $derived(files.reduce((s, f) => s + f.additions, 0));
  let totalDeletions = $derived(files.reduce((s, f) => s + f.deletions, 0));

  function fileName(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function fileDir(path: string): string {
    const parts = path.split("/");
    if (parts.length <= 1) return "";
    return parts.slice(0, -1).join("/") + "/";
  }

  function statusIcon(status: string): string {
    switch (status) {
      case "A": return "+";
      case "D": return "−";
      default: return "~";
    }
  }
</script>

<div class="diff-viewer">
  {#if loading}
    <div class="diff-empty">Loading...</div>
  {:else if error}
    <div class="diff-empty diff-error">{error}</div>
  {:else if files.length === 0}
    <div class="diff-empty">
      <p>No changes yet.</p>
      <button class="refresh-btn" onclick={loadFiles}>Refresh</button>
    </div>
  {:else}
    <div class="diff-layout">
      <!-- File sidebar -->
      <div class="file-sidebar">
        <div class="file-sidebar-header">
          <span class="file-count">Changes {files.length}</span>
          <span class="file-stat">
            <span class="stat-add">+{totalAdditions}</span>
            <span class="stat-del">−{totalDeletions}</span>
          </span>
          <button class="refresh-btn-sm" onclick={loadFiles} title="Refresh">↻</button>
        </div>
        {#if onCreatePr && (!prState || prState === "none") && files.length > 0}
          <button class="create-pr-btn" onclick={onCreatePr} disabled={disabled}>
            Push & Create PR
          </button>
        {/if}
        <div class="file-list">
          {#each files as file}
            <button
              class="file-item"
              class:active={file.path === selectedFile}
              onclick={() => selectFile(file.path)}
            >
              <span class="file-status" class:add={file.status === "A"} class:del={file.status === "D"} class:mod={file.status === "M"}>
                {statusIcon(file.status)}
              </span>
              <span class="file-path">
                {#if fileDir(file.path)}<span class="file-dir">{fileDir(file.path)}</span>{/if}{fileName(file.path)}
              </span>
              <span class="file-stat-inline">
                <span class="stat-add">+{file.additions}</span>
                <span class="stat-del">−{file.deletions}</span>
              </span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Diff content -->
      <div class="diff-content">
        {#if selectedFile}
          {#each lines as line}
            {#if line.type !== "header"}
              <div class="diff-line {line.type}">
                <span class="diff-gutter">{line.type === "add" ? "+" : line.type === "remove" ? "−" : " "}</span>
                <span class="diff-text">{line.type === "hunk" ? line.text : line.text.slice(1) || " "}</span>
              </div>
            {/if}
          {/each}
        {:else}
          <div class="diff-empty">Select a file</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .diff-viewer {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .diff-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  .diff-error {
    color: var(--diff-del);
  }

  .diff-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ── File sidebar ──────────────────────── */

  .file-sidebar {
    width: 240px;
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  .file-sidebar-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.6rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.72rem;
  }

  .file-count {
    color: var(--text-secondary);
    font-weight: 600;
  }

  .file-stat {
    display: flex;
    gap: 0.3rem;
    margin-left: auto;
  }

  .stat-add {
    color: var(--status-ok);
    font-family: var(--font-mono);
    font-size: 0.7rem;
  }

  .stat-del {
    color: var(--diff-del);
    font-family: var(--font-mono);
    font-size: 0.7rem;
  }

  .refresh-btn-sm {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0 0.2rem;
  }

  .refresh-btn-sm:hover {
    color: var(--text-primary);
  }

  .create-pr-btn {
    margin: 0.4rem 0.5rem;
    padding: 0.35rem 0;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 5px;
    color: var(--accent);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    text-align: center;
  }

  .create-pr-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .create-pr-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .file-list {
    flex: 1;
    overflow-y: auto;
  }

  .file-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem 0.6rem;
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    text-align: left;
  }

  .file-item:hover {
    background: var(--bg-hover);
  }

  .file-item.active {
    background: var(--border);
  }

  .file-status {
    width: 1.2ch;
    font-family: var(--font-mono);
    font-weight: 700;
    font-size: 0.72rem;
    flex-shrink: 0;
    text-align: center;
  }

  .file-status.add {
    color: var(--status-ok);
  }

  .file-status.del {
    color: var(--diff-del);
  }

  .file-status.mod {
    color: var(--accent);
  }

  .file-path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--font-mono);
    font-size: 0.73rem;
  }

  .file-dir {
    color: var(--text-dim);
  }

  .file-stat-inline {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
    opacity: 0.7;
  }

  .refresh-btn {
    padding: 0.25rem 0.6rem;
    background: var(--border);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
  }

  .refresh-btn:hover {
    color: var(--text-primary);
  }

  /* ── Diff content ──────────────────────── */

  .diff-content {
    flex: 1;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 0.78rem;
    line-height: 1.6;
  }

  .diff-line {
    display: flex;
    padding: 0 0.75rem;
    white-space: pre;
  }

  .diff-line.add {
    background: var(--diff-add-bg);
    color: var(--status-ok);
  }

  .diff-line.remove {
    background: var(--diff-del-bg);
    color: var(--diff-del);
  }

  .diff-line.context {
    color: var(--text-dim);
  }

  .diff-line.hunk {
    color: var(--accent);
    background: var(--bg-card);
    padding-top: 0.4rem;
    padding-bottom: 0.4rem;
    margin-top: 0.5rem;
    font-size: 0.72rem;
  }

  .diff-gutter {
    width: 1.5ch;
    flex-shrink: 0;
    text-align: center;
    opacity: 0.5;
    user-select: none;
  }

  .diff-text {
    flex: 1;
  }
</style>

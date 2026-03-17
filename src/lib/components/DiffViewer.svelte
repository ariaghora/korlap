<script lang="ts">
  import { getChangedFiles, getDiff, type ChangedFile } from "$lib/ipc";

  interface Props {
    workspaceId: string;
    refreshTrigger?: number;
  }

  let { workspaceId, refreshTrigger = 0 }: Props = $props();

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
      if (files.length > 0 && !selectedFile) {
        await selectFile(files[0].path);
      } else if (files.length === 0) {
        selectedFile = null;
        diff = "";
      } else if (selectedFile) {
        // Reload current file's diff
        diff = await getDiff(workspaceId, selectedFile);
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
    color: #6a6050;
    font-size: 0.85rem;
  }

  .diff-error {
    color: #c87e7e;
  }

  .diff-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ── File sidebar ──────────────────────── */

  .file-sidebar {
    width: 240px;
    border-right: 1px solid #2a2520;
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  .file-sidebar-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.6rem;
    border-bottom: 1px solid #2a2520;
    font-size: 0.72rem;
  }

  .file-count {
    color: #8a7e6a;
    font-weight: 600;
  }

  .file-stat {
    display: flex;
    gap: 0.3rem;
    margin-left: auto;
  }

  .stat-add {
    color: #7e9e6b;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.7rem;
  }

  .stat-del {
    color: #c87e7e;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.7rem;
  }

  .refresh-btn-sm {
    background: none;
    border: none;
    color: #6a6050;
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0 0.2rem;
  }

  .refresh-btn-sm:hover {
    color: #d4c5a9;
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
    color: #d4c5a9;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    text-align: left;
  }

  .file-item:hover {
    background: #1e1b17;
  }

  .file-item.active {
    background: #2a2520;
  }

  .file-status {
    width: 1.2ch;
    font-family: "SF Mono", "Fira Code", monospace;
    font-weight: 700;
    font-size: 0.72rem;
    flex-shrink: 0;
    text-align: center;
  }

  .file-status.add {
    color: #7e9e6b;
  }

  .file-status.del {
    color: #c87e7e;
  }

  .file-status.mod {
    color: #c8a97e;
  }

  .file-path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.73rem;
  }

  .file-dir {
    color: #6a6050;
  }

  .file-stat-inline {
    display: flex;
    gap: 0.2rem;
    flex-shrink: 0;
    opacity: 0.7;
  }

  .refresh-btn {
    padding: 0.25rem 0.6rem;
    background: #2a2520;
    border: 1px solid #3a3530;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
  }

  .refresh-btn:hover {
    color: #d4c5a9;
  }

  /* ── Diff content ──────────────────────── */

  .diff-content {
    flex: 1;
    overflow: auto;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.78rem;
    line-height: 1.6;
  }

  .diff-line {
    display: flex;
    padding: 0 0.75rem;
    white-space: pre;
  }

  .diff-line.add {
    background: #1a2a1a;
    color: #7e9e6b;
  }

  .diff-line.remove {
    background: #2a1a1a;
    color: #c87e7e;
  }

  .diff-line.context {
    color: #6a6050;
  }

  .diff-line.hunk {
    color: #c8a97e;
    background: #1a1814;
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

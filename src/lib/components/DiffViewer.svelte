<script lang="ts">
  import { getChangedFiles, getDiff, type ChangedFile } from "$lib/ipc";
  import { MessageSquare, FileCode, SquareArrowOutUpRight } from "lucide-svelte";
  import ResizeHandle from "./ResizeHandle.svelte";

  interface Props {
    workspaceId: string;
    refreshTrigger?: number;
    onQuote?: (text: string) => void;
    onOpenFile?: (path: string) => void;
    onGoToLine?: (path: string, line: number) => void;
  }

  let { workspaceId, refreshTrigger = 0, onQuote, onOpenFile, onGoToLine }: Props = $props();

  let files = $state<ChangedFile[]>([]);
  let selectedFile = $state<string | null>(null);
  let diff = $state("");
  let loading = $state(false);
  let error = $state("");
  let hasLoaded = false;

  // Serialize file list for cheap equality check
  function filesKey(f: ChangedFile[]): string {
    return f.map(x => `${x.path}:${x.status}:${x.additions}:${x.deletions}`).join("\n");
  }
  let lastFilesKey = "";
  let lastDiff = "";

  async function loadFiles() {
    // Only show loading spinner on first load — background refreshes are silent
    if (!hasLoaded) {
      loading = true;
    }
    error = "";
    try {
      const newFiles = await getChangedFiles(workspaceId);
      const newKey = filesKey(newFiles);

      if (newFiles.length === 0) {
        if (files.length !== 0 || selectedFile !== null) {
          files = [];
          selectedFile = null;
          diff = "";
          lastFilesKey = "";
          lastDiff = "";
        }
      } else if (selectedFile && newFiles.some((f) => f.path === selectedFile)) {
        // Update file list only if it actually changed
        if (newKey !== lastFilesKey) {
          files = newFiles;
          lastFilesKey = newKey;
        }
        // Reload current file's diff, but skip update if unchanged
        const newDiff = await getDiff(workspaceId, selectedFile);
        if (newDiff !== lastDiff) {
          diff = newDiff;
          lastDiff = newDiff;
        }
      } else {
        // Selected file gone or no selection — pick first
        files = newFiles;
        lastFilesKey = newKey;
        await selectFile(newFiles[0].path);
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
      hasLoaded = true;
    }
  }

  async function selectFile(path: string) {
    selectedFile = path;
    try {
      const newDiff = await getDiff(workspaceId, path);
      diff = newDiff;
      lastDiff = newDiff;
    } catch (e) {
      diff = `Error: ${e}`;
      lastDiff = "";
    }
  }

  let prevWorkspaceId = "";
  $effect(() => {
    const wsId = workspaceId;
    const _trigger = refreshTrigger;
    // Reset on workspace switch so we show loading for new workspace
    if (wsId !== prevWorkspaceId) {
      hasLoaded = false;
      lastFilesKey = "";
      lastDiff = "";
      prevWorkspaceId = wsId;
    }
    loadFiles();
  });

  interface DiffLine {
    type: "add" | "remove" | "context" | "header" | "hunk";
    text: string;
    oldNo?: number;
    newNo?: number;
  }

  function parseDiff(raw: string): DiffLine[] {
    if (!raw.trim()) return [];
    let oldLine = 0;
    let newLine = 0;
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
        const m = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
        if (m) {
          oldLine = parseInt(m[1], 10);
          newLine = parseInt(m[2], 10);
        }
        return { type: "hunk" as const, text: line };
      }
      if (line.startsWith("+")) {
        return { type: "add" as const, text: line, newNo: newLine++ };
      }
      if (line.startsWith("-")) {
        return { type: "remove" as const, text: line, oldNo: oldLine++ };
      }
      const result: DiffLine = { type: "context" as const, text: line, oldNo: oldLine++, newNo: newLine++ };
      return result;
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

  let fileSidebarWidth = $state(240);
  const FILE_SIDEBAR_MIN = 140;
  const FILE_SIDEBAR_MAX = 500;

  function handleFileSidebarResize(delta: number) {
    fileSidebarWidth = Math.min(FILE_SIDEBAR_MAX, Math.max(FILE_SIDEBAR_MIN, fileSidebarWidth + delta));
  }

  function statusIcon(status: string): string {
    switch (status) {
      case "A": return "+";
      case "D": return "−";
      default: return "~";
    }
  }

  // ── Line selection for quoting ──────────────────────────
  // Indices are into the `visibleLines` array (lines minus headers)
  let selStart = $state<number | null>(null);
  let selEnd = $state<number | null>(null);

  let visibleLines = $derived(
    lines
      .map((line, i) => ({ line, idx: i }))
      .filter(({ line }) => line.type !== "header")
  );

  let selMin = $derived(selStart !== null && selEnd !== null ? Math.min(selStart, selEnd) : null);
  let selMax = $derived(selStart !== null && selEnd !== null ? Math.max(selStart, selEnd) : null);

  function handleLineMousedown(visIdx: number, e: MouseEvent) {
    // Prevent native text selection on shift+click
    if (e.shiftKey) e.preventDefault();
  }

  function handleLineClick(visIdx: number, e: MouseEvent) {
    if (e.shiftKey && selStart !== null) {
      selEnd = visIdx;
    } else {
      selStart = visIdx;
      selEnd = visIdx;
    }
  }

  function clearSelection() {
    selStart = null;
    selEnd = null;
  }

  // Clear selection when file or diff content changes
  $effect(() => {
    void selectedFile;
    void lines;
    clearSelection();
  });

  function buildQuotedText(): string {
    if (selMin === null || selMax === null || !selectedFile) return "";
    const selected = visibleLines.slice(selMin, selMax + 1);
    if (selected.length === 0) return "";
    const diffLines = selected.map(({ line }) => line.text).join("\n");

    // Determine line range for display
    const first = selected[0].line;
    const last = selected[selected.length - 1].line;
    const startLine = first.newNo ?? first.oldNo;
    const endLine = last.newNo ?? last.oldNo;
    const range = startLine !== undefined && endLine !== undefined && startLine !== endLine
      ? `L${startLine}-${endLine}`
      : startLine !== undefined
        ? `L${startLine}`
        : "";

    return `\`${selectedFile}\` ${range}\n\`\`\`diff\n${diffLines}\n\`\`\`\n\n`;
  }

  function handleQuote() {
    const text = buildQuotedText();
    if (text && onQuote) {
      onQuote(text);
      clearSelection();
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
      <div class="file-sidebar" style="width: {fileSidebarWidth}px">
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
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
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
              {#if onOpenFile}
                <button
                  class="open-file-btn"
                  title="Open in Files tab"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); onOpenFile(file.path); }}
                >
                  <FileCode size={13} />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </div>
      <ResizeHandle onResize={handleFileSidebarResize} />

      <!-- Diff content -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="diff-content" onclick={(e: MouseEvent) => { if (!(e.target as HTMLElement).closest('.line-no')) clearSelection(); }}>
        {#if selectedFile}
          {#each visibleLines as { line, idx: _origIdx }, visIdx}
            {@const selected = selMin !== null && selMax !== null && visIdx >= selMin && visIdx <= selMax}
            {@const goToLineNo = line.newNo ?? line.oldNo}
            <div class="diff-line {line.type}" class:selected>
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="line-no old" onmousedown={(e: MouseEvent) => handleLineMousedown(visIdx, e)} onclick={(e: MouseEvent) => { e.stopPropagation(); handleLineClick(visIdx, e); }}>{line.oldNo ?? ""}</span>
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <span class="line-no new" onmousedown={(e: MouseEvent) => handleLineMousedown(visIdx, e)} onclick={(e: MouseEvent) => { e.stopPropagation(); handleLineClick(visIdx, e); }}>{line.newNo ?? ""}</span>
              <span class="diff-gutter">{line.type === "add" ? "+" : line.type === "remove" ? "−" : " "}</span>
              <span class="diff-text">{line.type === "hunk" ? line.text : line.text.slice(1) || " "}</span>
              {#if onGoToLine && goToLineNo !== undefined && line.type !== "hunk"}
                <button
                  class="goto-line-btn"
                  title="Open at line {goToLineNo}"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); onGoToLine(selectedFile!, goToLineNo); }}
                >
                  <SquareArrowOutUpRight size={12} />
                </button>
              {/if}
            </div>
          {/each}

          {#if selMin !== null && onQuote}
            <button class="quote-btn" onclick={handleQuote}>
              <MessageSquare size={12} />
              Quote in Chat
            </button>
          {/if}
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

  .file-list {
    flex: 1;
    overflow-y: auto;
  }

  .file-item {
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
    min-width: 0;
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

  .open-file-btn {
    display: none;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0;
    background: none;
    border: none;
    border-radius: 3px;
    color: var(--text-dim);
    cursor: pointer;
  }

  .file-item:hover .file-stat-inline {
    display: none;
  }

  .file-item:hover .open-file-btn {
    display: flex;
  }

  .open-file-btn:hover {
    color: var(--accent);
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

  .line-no {
    width: 4ch;
    flex-shrink: 0;
    text-align: right;
    color: var(--text-dim);
    opacity: 0.5;
    user-select: none;
    padding-right: 0.4ch;
  }

  .line-no.new {
    border-right: 1px solid var(--border);
    margin-right: 0.5ch;
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

  /* ── Line selection ──────────────────────── */

  .line-no {
    cursor: pointer;
  }

  .line-no:hover {
    opacity: 0.9;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .diff-line.selected {
    background: color-mix(in srgb, var(--accent) 12%, transparent) !important;
    outline: none;
  }

  .diff-line.selected .line-no {
    opacity: 0.9;
    color: var(--accent);
  }

  /* ── Quote button ──────────────────────── */

  .quote-btn {
    position: sticky;
    bottom: 8px;
    left: 50%;
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    margin: 0.5rem auto 0;
    padding: 0.35rem 0.7rem;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.35);
    color: var(--accent);
    font-family: inherit;
    font-size: 0.72rem;
    font-weight: 600;
    cursor: pointer;
    z-index: 5;
  }

  .quote-btn:hover {
    background: var(--bg-hover);
    border-color: var(--accent);
  }

  /* ── Go-to-line button ──────────────────── */

  .goto-line-btn {
    display: none;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    padding: 0 0.3rem;
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    opacity: 0.6;
  }

  .diff-line:hover .goto-line-btn {
    display: inline-flex;
  }

  .goto-line-btn:hover {
    color: var(--accent);
    opacity: 1;
  }
</style>

<script lang="ts">
  import { grepWorkspace, grepRepo, readFile, readRepoFile, type GrepMatch } from "$lib/ipc";
  import { Search } from "lucide-svelte";

  interface Props {
    workspaceId?: string;
    repoId?: string;
    onClose: () => void;
    onAddToContext: (path: string, displayName: string, lineNumber: number) => void;
    onOpenInFiles?: (path: string) => void;
  }

  let { workspaceId, repoId, onClose, onAddToContext, onOpenInFiles }: Props = $props();

  function doGrep(q: string, regex: boolean, matchCase: boolean) {
    if (workspaceId) return grepWorkspace(workspaceId, q, regex, matchCase);
    if (repoId) return grepRepo(repoId, q, regex, matchCase);
    return Promise.reject("No workspace or repo ID");
  }

  function doReadFile(path: string) {
    if (workspaceId) return readFile(workspaceId, path);
    if (repoId) return readRepoFile(repoId, path);
    return Promise.reject("No workspace or repo ID");
  }

  let query = $state("");
  let isRegex = $state(false);
  let caseSensitive = $state(false);
  let results = $state<GrepMatch[]>([]);
  let truncated = $state(false);
  let selectedIndex = $state(0);
  let loading = $state(false);
  let errorMsg = $state("");
  let inputEl: HTMLInputElement | undefined = $state();

  // Preview state
  let previewContent = $state<string | null>(null);
  let previewPath = $state<string | null>(null);
  let previewLine = $state(0);
  let previewEl: HTMLPreElement | undefined = $state();

  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    inputEl?.focus();
  });

  // Debounced search
  $effect(() => {
    const q = query;
    const regex = isRegex;
    const matchCase = caseSensitive;
    clearTimeout(debounceTimer);
    if (!q.trim()) {
      results = [];
      truncated = false;
      selectedIndex = 0;
      previewContent = null;
      previewPath = null;
      errorMsg = "";
      return;
    }
    loading = true;
    debounceTimer = setTimeout(async () => {
      try {
        const res = await doGrep(q, regex, matchCase);
        results = res.matches;
        truncated = res.truncated;
        selectedIndex = 0;
        errorMsg = "";
      } catch (e) {
        errorMsg = String(e);
        results = [];
        truncated = false;
      } finally {
        loading = false;
      }
    }, 150);
  });

  // Fetch preview when selection changes
  $effect(() => {
    const match = results[selectedIndex];
    if (!match) {
      previewContent = null;
      previewPath = null;
      return;
    }
    previewLine = match.line_number;
    if (match.path === previewPath) {
      // Same file, just scroll
      scrollPreviewToLine();
      return;
    }
    previewPath = match.path;
    doReadFile(match.path)
      .then((content) => {
        previewContent = content;
        // Scroll after content renders
        requestAnimationFrame(() => scrollPreviewToLine());
      })
      .catch(() => {
        previewContent = null;
      });
  });

  function scrollPreviewToLine() {
    if (!previewEl) return;
    const lineEl = previewEl.querySelector(`[data-line="${previewLine}"]`);
    if (lineEl) {
      lineEl.scrollIntoView({ block: "center" });
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        onClose();
        break;
      case "ArrowDown":
        e.preventDefault();
        if (results.length > 0) {
          selectedIndex = Math.min(selectedIndex + 1, results.length - 1);
          scrollSelectedIntoView();
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (results.length > 0) {
          selectedIndex = Math.max(selectedIndex - 1, 0);
          scrollSelectedIntoView();
        }
        break;
      case "Enter": {
        e.preventDefault();
        const match = results[selectedIndex];
        if (!match) break;
        if (e.metaKey && onOpenInFiles) {
          onOpenInFiles(match.path);
        } else if (!e.metaKey) {
          const name = match.path.split("/").pop() ?? match.path;
          onAddToContext(match.path, name, match.line_number);
        }
        break;
      }
    }
  }

  function scrollSelectedIntoView() {
    requestAnimationFrame(() => {
      const el = document.querySelector(".grep-result-item.selected");
      el?.scrollIntoView({ block: "nearest" });
    });
  }

  function handleBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      onClose();
    }
  }

  function formatPath(path: string): { dir: string; file: string } {
    const lastSlash = path.lastIndexOf("/");
    if (lastSlash < 0) return { dir: "", file: path };
    return {
      dir: path.slice(0, lastSlash + 1),
      file: path.slice(lastSlash + 1),
    };
  }

  let previewLines = $derived(previewContent?.split("\n") ?? []);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="backdrop" onmousedown={handleBackdropClick}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="modal" onkeydown={handleKeydown}>
    <div class="search-bar">
      <Search size={14} strokeWidth={2} />
      <input
        bind:this={inputEl}
        bind:value={query}
        type="text"
        class="search-input"
        placeholder="Search file contents…"
        spellcheck="false"
        autocomplete="off"
      />
      <button
        class="regex-toggle"
        class:active={isRegex}
        onclick={() => { isRegex = !isRegex; }}
        title={isRegex ? "Regex mode (click for literal)" : "Literal mode (click for regex)"}
      >.*</button>
      <button
        class="regex-toggle"
        class:active={caseSensitive}
        onclick={() => { caseSensitive = !caseSensitive; }}
        title={caseSensitive ? "Case sensitive (click for smart case)" : "Smart case (click for case sensitive)"}
      >Aa</button>
    </div>

    <div class="body">
      <div class="results-pane">
        {#if errorMsg}
          <div class="results-error">{errorMsg}</div>
        {:else if results.length === 0 && query.trim() && !loading}
          <div class="results-empty">No matches</div>
        {:else}
          {#each results as match, i}
            {@const { dir, file } = formatPath(match.path)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              class="grep-result-item"
              class:selected={i === selectedIndex}
              role="option"
              aria-selected={i === selectedIndex}
              onmousedown={() => { selectedIndex = i; }}
              ondblclick={() => {
                const name = match.path.split("/").pop() ?? match.path;
                onAddToContext(match.path, name, match.line_number);
              }}
            >
              <div class="result-path">
                <span class="result-dir">{dir}</span><span class="result-file">{file}</span><span class="result-line">:{match.line_number}</span>
              </div>
              <div class="result-content">{match.line_content}</div>
            </div>
          {/each}
          {#if truncated}
            <div class="results-truncated">Results truncated (100+ matches)</div>
          {/if}
        {/if}
      </div>

      <div class="preview-pane">
        {#if previewContent !== null && previewPath}
          <div class="preview-header">{previewPath}</div>
          <pre class="preview-code" bind:this={previewEl}>{#each previewLines as line, i}<div
                class="preview-line"
                class:highlighted={i + 1 === previewLine}
                data-line={i + 1}
              ><span class="line-num">{i + 1}</span><span class="line-text">{line}</span></div>{/each}</pre>
        {:else if query.trim() && results.length > 0}
          <div class="preview-empty">Loading preview…</div>
        {/if}
      </div>
    </div>

    <div class="footer">
      <span class="hint"><kbd>↑↓</kbd> navigate</span>
      <span class="hint"><kbd>⏎</kbd> add to context</span>
      {#if onOpenInFiles}
        <span class="hint"><kbd>⌘⏎</kbd> open in files</span>
      {/if}
      <span class="hint"><kbd>esc</kbd> close</span>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: var(--overlay-bg);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 10vh;
  }

  .modal {
    width: 900px;
    max-width: calc(100vw - 4rem);
    max-height: 70vh;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 24px 48px rgba(0, 0, 0, 0.4);
  }

  /* ── Search bar ─────────────────────── */

  .search-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-dim);
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.9rem;
    outline: none;
  }

  .search-input::placeholder {
    color: var(--text-dim);
  }

  .regex-toggle {
    padding: 0.15rem 0.4rem;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    border-radius: 4px;
    border: 1px solid var(--border-light);
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    line-height: 1;
  }

  .regex-toggle.active {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: var(--accent);
    color: var(--accent);
  }

  /* ── Body (results + preview) ───────── */

  .body {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ── Results pane ───────────────────── */

  .results-pane {
    width: 40%;
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }

  .grep-result-item {
    padding: 0.4rem 0.75rem;
    cursor: pointer;
    border-bottom: 1px solid var(--border);
  }

  .grep-result-item:hover {
    background: var(--bg-hover);
  }

  .grep-result-item.selected {
    background: var(--bg-active);
  }

  .result-path {
    font-size: 0.75rem;
    line-height: 1.3;
    font-family: var(--font-mono);
  }

  .result-dir {
    color: var(--text-dim);
  }

  .result-file {
    color: var(--text-bright);
  }

  .result-line {
    color: var(--accent);
  }

  .result-content {
    font-size: 0.75rem;
    font-family: var(--font-mono);
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 0.1rem;
  }

  .results-empty,
  .results-error,
  .results-truncated {
    padding: 1rem;
    font-size: 0.8rem;
    color: var(--text-dim);
    text-align: center;
  }

  .results-error {
    color: var(--diff-del);
  }

  /* ── Preview pane ───────────────────── */

  .preview-pane {
    width: 60%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .preview-header {
    padding: 0.4rem 0.75rem;
    font-size: 0.7rem;
    font-family: var(--font-mono);
    color: var(--text-dim);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .preview-code {
    flex: 1;
    overflow: auto;
    margin: 0;
    padding: 0;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.5;
  }

  .preview-line {
    display: flex;
    padding: 0 0.75rem 0 0;
    min-height: 1.5em;
  }

  .preview-line.highlighted {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .line-num {
    display: inline-block;
    width: 3.5rem;
    text-align: right;
    padding-right: 0.75rem;
    color: var(--text-dim);
    user-select: none;
    flex-shrink: 0;
  }

  .line-text {
    white-space: pre;
    color: var(--text-primary);
  }

  .preview-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    font-size: 0.8rem;
  }

  /* ── Footer ─────────────────────────── */

  .footer {
    display: flex;
    gap: 1.25rem;
    padding: 0.5rem 1rem;
    border-top: 1px solid var(--border);
  }

  .hint {
    font-size: 0.7rem;
    color: var(--text-dim);
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  kbd {
    font-family: var(--font-mono);
    font-size: 0.65rem;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    border: 1px solid var(--border-light);
    background: var(--bg-hover);
    color: var(--text-secondary);
  }
</style>

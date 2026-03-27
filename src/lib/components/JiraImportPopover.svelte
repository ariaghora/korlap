<script lang="ts">
  import { getJiraIssues, type JiraIssue } from "$lib/ipc";
  import type { PastedImage } from "$lib/chat-utils";
  import type { Mention } from "./MentionInput.svelte";
  import { Loader2 } from "lucide-svelte";

  export interface JiraTaskData {
    title: string;
    description: string;
    newImages: PastedImage[];
    existingPaths: string[];
    mentions: Mention[];
    planMode: boolean;
    thinkingMode: boolean;
    model: string;
  }

  interface Props {
    onSubmit: (tasks: JiraTaskData[]) => void;
    onCancel: () => void;
  }

  let { onSubmit, onCancel }: Props = $props();

  let issues = $state<JiraIssue[]>([]);
  let selectedKeys = $state<Set<string>>(new Set());
  let loading = $state(true);
  let error = $state<string | null>(null);
  let filterText = $state("");
  let filterStatus = $state<string>("all");

  $effect(() => {
    loadIssues();
  });

  async function loadIssues() {
    loading = true;
    error = null;
    try {
      const result = await getJiraIssues();
      issues = result;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  let allSelected = $derived(
    issues.length > 0 && selectedKeys.size === issues.length
  );

  let selectedCount = $derived(selectedKeys.size);

  let uniqueStatuses = $derived(
    [...new Set(issues.map((i) => i.status))]
  );

  let filteredIssues = $derived(
    issues.filter((i) => {
      const matchesText = !filterText.trim() ||
        i.summary.toLowerCase().includes(filterText.toLowerCase()) ||
        i.key.toLowerCase().includes(filterText.toLowerCase());
      const matchesStatus = filterStatus === "all" || i.status === filterStatus;
      return matchesText && matchesStatus;
    })
  );

  let filteredCount = $derived(filteredIssues.length);

  function toggleAll() {
    if (allSelected) {
      selectedKeys = new Set();
    } else {
      selectedKeys = new Set(issues.map((i) => i.key));
    }
  }

  function toggleIssue(key: string) {
    const next = new Set(selectedKeys);
    if (next.has(key)) {
      next.delete(key);
    } else {
      next.add(key);
    }
    selectedKeys = next;
  }

  function handleImport() {
    const tasks: JiraTaskData[] = issues
      .filter((i) => selectedKeys.has(i.key))
      .map((i) => ({
        title: `[${i.key}] ${i.summary}`,
        description: i.description ?? "",
        newImages: [],
        existingPaths: [],
        mentions: [],
        planMode: false,
        thinkingMode: false,
        model: "",
      }));
    onSubmit(tasks);
  }

  function handleOverlayKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }

  function getStatusClass(status: string): string {
    const s = status.toLowerCase();
    if (s.includes("done") || s.includes("closed") || s.includes("resolved")) return "status-done";
    if (s.includes("progress") || s.includes("review") || s.includes("testing")) return "status-progress";
    if (s.includes("todo") || s.includes("open") || s.includes("backlog")) return "status-todo";
    return "";
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel} onkeydown={handleOverlayKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <div class="dialog-header">
      <span>Import from Jira</span>
      {#if !loading && !error}
        <label class="select-all">
          <input type="checkbox" checked={allSelected} onchange={toggleAll} />
          <span>All</span>
        </label>
      {/if}
    </div>

    <div class="dialog-body">
      {#if !loading && !error && issues.length > 0}
        <div class="filter-row">
          <input
            class="filter-input"
            type="text"
            placeholder="Filter by name..."
            bind:value={filterText}
          />
          {#if uniqueStatuses.length > 1}
            <select class="filter-select" bind:value={filterStatus}>
              <option value="all">All</option>
              {#each uniqueStatuses as status}
                <option value={status}>{status}</option>
              {/each}
            </select>
          {/if}
        </div>
      {/if}
      {#if loading}
        <div class="loading-state">
          <Loader2 size={20} class="spinner" />
          <span>Loading Jira issues...</span>
        </div>
      {:else if error}
        <div class="error-state">
          <span class="error-text">Failed to load issues</span>
          <span class="error-detail">{error}</span>
          <button class="retry-btn" onclick={loadIssues}>Retry</button>
        </div>
      {:else if issues.length === 0}
        <div class="empty-state">No issues found</div>
      {:else if filteredCount === 0}
        <div class="empty-state">No matching issues</div>
      {:else}
        <div class="issue-list">
          {#each filteredIssues as issue (issue.key)}
            <label class="issue-item">
              <input
                type="checkbox"
                checked={selectedKeys.has(issue.key)}
                onchange={() => toggleIssue(issue.key)}
              />
              <div class="issue-content">
                <div class="issue-header">
                  <span class="issue-key">{issue.key}</span>
                  <span class="issue-type">{issue.issue_type}</span>
                  <span class="issue-status {getStatusClass(issue.status)}">{issue.status}</span>
                </div>
                <div class="issue-summary">{issue.summary}</div>
              </div>
            </label>
          {/each}
        </div>
        {#if filterText && filteredCount !== issues.length}
          <div class="filter-count">{filteredCount} of {issues.length}</div>
        {/if}
      {/if}
    </div>

    <div class="dialog-footer">
      <span class="footer-hint">{selectedCount} selected</span>
      <div class="footer-actions">
        <button class="cancel-btn" onclick={onCancel}>Cancel</button>
        <button class="submit-btn" onclick={handleImport} disabled={selectedCount === 0}>
          Import
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dialog {
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    padding: 1rem;
    background: color-mix(in srgb, var(--bg-sidebar) 97%, white);
    border: 0.5px solid color-mix(in srgb, var(--border-light) 60%, transparent);
    border-radius: 12px;
    box-shadow:
      0 0 0 0.5px rgba(0, 0, 0, 0.3),
      0 8px 32px rgba(0, 0, 0, 0.45),
      0 2px 8px rgba(0, 0, 0, 0.2);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--text-secondary);
    padding: 0 0.1rem 0.5rem;
  }

  .select-all {
    display: flex;
    align-items: center;
    gap: 0.3rem;
    font-size: 0.72rem;
    font-weight: 500;
    color: var(--text-dim);
    cursor: pointer;
  }

  .select-all input {
    accent-color: var(--accent);
  }

  .dialog-body {
    flex: 1;
    min-height: 200px;
    max-height: 400px;
    overflow-y: auto;
    margin: 0 -0.25rem;
    padding: 0 0.25rem;
  }

  .filter-row {
    display: flex;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
  }

  .filter-input {
    flex: 1;
    box-sizing: border-box;
    padding: 0.45rem 0.6rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
  }

  .filter-input::placeholder {
    color: var(--text-muted);
  }

  .filter-input:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .filter-select {
    padding: 0.45rem 0.6rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
    cursor: pointer;
  }

  .filter-select:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .filter-count {
    text-align: center;
    font-size: 0.7rem;
    color: var(--text-muted);
    padding: 0.4rem 0;
    margin-top: 0.25rem;
    border-top: 1px solid var(--border);
  }

  .loading-state,
  .error-state,
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 2rem;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  :global(.spinner) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .error-text {
    color: #e05252;
    font-weight: 500;
  }

  .error-detail {
    font-size: 0.75rem;
    color: var(--text-muted);
    text-align: center;
    max-width: 300px;
  }

  .retry-btn {
    padding: 0.35rem 0.7rem;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .retry-btn:hover {
    background: var(--btn-subtle-hover);
  }

  .issue-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .issue-item {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.5rem;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.1s;
  }

  .issue-item:hover {
    background: var(--border);
  }

  .issue-item input {
    margin-top: 0.15rem;
    accent-color: var(--accent);
    flex-shrink: 0;
  }

  .issue-content {
    flex: 1;
    min-width: 0;
  }

  .issue-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.2rem;
  }

  .issue-key {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    font-weight: 700;
    color: var(--text-bright);
  }

  .issue-type {
    font-size: 0.65rem;
    color: var(--text-muted);
    background: var(--border);
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
  }

  .issue-status {
    font-size: 0.65rem;
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
    margin-left: auto;
  }

  .status-todo {
    color: var(--text-muted);
    background: var(--border);
  }

  .status-progress {
    color: #d4a017;
    background: color-mix(in srgb, #d4a017 15%, transparent);
  }

  .status-done {
    color: #3eb76a;
    background: color-mix(in srgb, #3eb76a 15%, transparent);
  }

  .issue-summary {
    font-size: 0.8rem;
    color: var(--text-secondary);
    line-height: 1.35;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 0.5rem;
    padding-top: 0.5rem;
    border-top: 1px solid var(--border);
  }

  .footer-hint {
    font-size: 0.7rem;
    color: var(--text-muted);
  }

  .footer-actions {
    display: flex;
    gap: 0.35rem;
    align-items: center;
  }

  .cancel-btn {
    padding: 0.35rem 0.7rem;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .cancel-btn:hover {
    background: var(--btn-subtle-hover);
    color: var(--text-primary);
  }

  .submit-btn {
    padding: 0.35rem 0.9rem;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: var(--bg-base);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
  }

  .submit-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>

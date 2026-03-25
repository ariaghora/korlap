<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { renderMarkdown } from "$lib/markdown";
  import { externalLinks } from "$lib/actions";
  import { X, ExternalLink, ArrowRight, GitBranch, Clock, GitPullRequest, CheckCircle, AlertTriangle, Circle } from "lucide-svelte";

  interface Props {
    workspace: WorkspaceInfo;
    prStatus?: PrStatus;
    changeCounts?: { additions: number; deletions: number };
    isReviewing?: boolean;
    isCreating?: boolean;
    onGoToWorkspace: () => void;
    onClose: () => void;
  }

  let {
    workspace,
    prStatus,
    changeCounts,
    isReviewing = false,
    isCreating = false,
    onGoToWorkspace,
    onClose,
  }: Props = $props();

  function statusLabel(): string {
    if (isCreating) return "Creating";
    if (isReviewing) return "Reviewing";
    if (workspace.status === "running") return "Running";
    return "Waiting";
  }

  function statusClass(): string {
    if (isCreating) return "creating";
    if (isReviewing) return "reviewing";
    if (workspace.status === "running") return "running";
    return "waiting";
  }

  function elapsed(): string {
    if (workspace.status !== "running") return "";
    const mins = Math.floor((Date.now() - workspace.created_at * 1000) / 60000);
    return mins < 1 ? "<1m" : `${mins}m`;
  }

  function prStateLabel(): string {
    if (!prStatus || prStatus.state === "none") return "";
    if (prStatus.state === "open") return "Open";
    if (prStatus.state === "merged") return "Merged";
    if (prStatus.state === "closed") return "Closed";
    return "";
  }

  function checksLabel(): string {
    if (!prStatus || prStatus.checks === "none") return "";
    if (prStatus.checks === "passing") return "Passing";
    if (prStatus.checks === "failing") return "Failing";
    if (prStatus.checks === "pending") return "Pending";
    return "";
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onClose}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <div class="dialog-header">
      <div class="status-badge {statusClass()}">
        <span class="status-dot"></span>
        {statusLabel()}
        {#if workspace.status === "running" && !isReviewing}
          <span class="status-elapsed">{elapsed()}</span>
        {/if}
      </div>
      <button class="close-btn" onclick={onClose}>
        <X size={16} />
      </button>
    </div>

    <div class="dialog-body">
      <h2 class="ws-title">{workspace.task_title ?? workspace.name}</h2>

      {#if workspace.task_description}
        <div class="ws-description markdown-body" use:externalLinks>{@html renderMarkdown(workspace.task_description)}</div>
      {/if}

      <div class="detail-rows">
        <div class="detail-row">
          <GitBranch size={13} />
          <span class="detail-label">Branch</span>
          <span class="detail-value mono">{workspace.branch}</span>
        </div>

        {#if changeCounts && (changeCounts.additions > 0 || changeCounts.deletions > 0)}
          <div class="detail-row">
            <Clock size={13} />
            <span class="detail-label">Changes</span>
            <span class="detail-value">
              <span class="diff-add">+{changeCounts.additions}</span>
              <span class="diff-del">−{changeCounts.deletions}</span>
            </span>
          </div>
        {/if}

        {#if prStatus && prStatus.state !== "none"}
          <div class="detail-row">
            <GitPullRequest size={13} />
            <span class="detail-label">PR #{prStatus.number}</span>
            <span class="detail-value">
              <span class="pr-state pr-{prStatus.state}">{prStateLabel()}</span>
              {#if prStatus.checks !== "none"}
                <span class="pr-checks pr-checks-{prStatus.checks}">
                  {#if prStatus.checks === "passing"}
                    <CheckCircle size={11} />
                  {:else if prStatus.checks === "failing"}
                    <AlertTriangle size={11} />
                  {:else}
                    <Circle size={11} />
                  {/if}
                  {checksLabel()}
                </span>
              {/if}
            </span>
          </div>

          {#if prStatus.url}
            <div class="detail-row">
              <ExternalLink size={13} />
              <span class="detail-label">Link</span>
              <a class="detail-value pr-link" href={prStatus.url} target="_blank" rel="noopener">
                {prStatus.url.replace(/^https?:\/\//, '')}
              </a>
            </div>
          {/if}
        {/if}
      </div>
    </div>

    <div class="dialog-footer">
      <span class="shortcut-hint">Tip: <kbd>&#8984;</kbd>+click card to open directly</span>
      <button class="go-btn" onclick={onGoToWorkspace}>
        Go to workspace
        <ArrowRight size={14} />
      </button>
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
    width: 420px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--bg-sidebar) 97%, white);
    border: 0.5px solid color-mix(in srgb, var(--border-light) 60%, transparent);
    border-radius: 12px;
    box-shadow:
      0 0 0 0.5px rgba(0, 0, 0, 0.3),
      0 8px 32px rgba(0, 0, 0, 0.45),
      0 2px 8px rgba(0, 0, 0, 0.2);
    overflow: hidden;
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 0.75rem 0 0.85rem;
  }

  .status-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-dim);
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .status-badge.running .status-dot {
    background: var(--accent);
    box-shadow: 0 0 6px color-mix(in srgb, var(--accent) 50%, transparent);
    animation: pulse 2s ease-in-out infinite;
  }
  .status-badge.running { color: var(--accent); }

  .status-badge.reviewing .status-dot {
    background: var(--accent);
    animation: pulse-slow 3s ease-in-out infinite;
  }
  .status-badge.reviewing { color: var(--accent); }

  .status-badge.creating .status-dot {
    background: var(--accent);
    animation: pulse 1s ease-in-out infinite;
  }
  .status-badge.creating { color: var(--accent); }

  .status-badge.waiting .status-dot {
    background: var(--status-ok);
  }
  .status-badge.waiting { color: var(--status-ok); }

  .status-elapsed {
    font-family: var(--font-mono);
    font-size: 0.62rem;
    opacity: 0.7;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  @keyframes pulse-slow {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-dim);
    cursor: pointer;
  }

  .close-btn:hover {
    background: var(--border);
    color: var(--text-primary);
  }

  .dialog-body {
    padding: 0.6rem 0.85rem 0.75rem;
    overflow-y: auto;
  }

  .ws-title {
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-bright);
    line-height: 1.35;
    margin: 0;
    word-break: break-word;
  }

  .ws-description {
    font-size: 0.8rem;
    color: var(--text-secondary);
    line-height: 1.45;
    margin: 0.4rem 0 0;
    word-break: break-word;
  }

  /* ── Markdown body (task description) ─── */

  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3),
  .markdown-body :global(h4) {
    margin: 0.5rem 0 0.2rem;
    color: var(--text-bright);
    font-weight: 600;
    line-height: 1.3;
  }

  .markdown-body :global(h1) { font-size: 0.95rem; }
  .markdown-body :global(h2) { font-size: 0.88rem; }
  .markdown-body :global(h3) { font-size: 0.82rem; }
  .markdown-body :global(h4) { font-size: 0.8rem; }

  .markdown-body :global(> h1:first-child),
  .markdown-body :global(> h2:first-child),
  .markdown-body :global(> h3:first-child),
  .markdown-body :global(> h4:first-child) {
    margin-top: 0;
  }

  .markdown-body :global(p) {
    margin: 0.25rem 0;
    line-height: 1.5;
  }

  .markdown-body :global(> p:first-child) {
    margin-top: 0;
  }

  .markdown-body :global(> p:last-child) {
    margin-bottom: 0;
  }

  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    margin: 0.25rem 0;
    padding-left: 1.3rem;
  }

  .markdown-body :global(li) {
    margin: 0.1rem 0;
    line-height: 1.45;
  }

  .markdown-body :global(li > p) {
    margin: 0.05rem 0;
  }

  .markdown-body :global(strong) {
    color: var(--text-bright);
    font-weight: 600;
  }

  .markdown-body :global(em) {
    font-style: italic;
    color: var(--text-primary);
  }

  .markdown-body :global(a) {
    color: var(--accent);
    text-decoration: none;
  }

  .markdown-body :global(a:hover) {
    text-decoration: underline;
  }

  .markdown-body :global(code) {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    background: var(--bg-active);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0.08rem 0.3rem;
    color: var(--text-bright);
  }

  .markdown-body :global(pre) {
    margin: 0.3rem 0;
    padding: 0.5rem 0.65rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow-x: auto;
    line-height: 1.45;
  }

  .markdown-body :global(pre code) {
    background: none;
    border: none;
    border-radius: 0;
    padding: 0;
    font-size: 0.73rem;
    color: var(--text-primary);
  }

  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0.5rem 0;
  }

  .markdown-body :global(blockquote) {
    margin: 0.3rem 0;
    padding: 0.1rem 0.6rem;
    border-left: 3px solid var(--border-light);
    color: var(--text-dim);
  }

  .detail-rows {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    margin-top: 0.85rem;
    padding-top: 0.7rem;
    border-top: 1px solid var(--border);
  }

  .detail-row {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    font-size: 0.75rem;
    color: var(--text-dim);
  }

  .detail-label {
    min-width: 50px;
    color: var(--text-dim);
  }

  .detail-value {
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .detail-value.mono {
    font-family: var(--font-mono);
    font-size: 0.72rem;
  }

  .diff-add { color: var(--diff-add); font-family: var(--font-mono); font-size: 0.72rem; }
  .diff-del { color: var(--diff-del); font-family: var(--font-mono); font-size: 0.72rem; margin-left: 0.3rem; }

  .pr-state {
    font-weight: 600;
    font-size: 0.72rem;
  }
  .pr-open { color: var(--status-ok); }
  .pr-merged { color: #a78bfa; }
  .pr-closed { color: var(--diff-del); }

  .pr-checks {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    margin-left: 0.4rem;
    font-size: 0.68rem;
  }
  .pr-checks-passing { color: var(--status-ok); }
  .pr-checks-failing { color: var(--diff-del); }
  .pr-checks-pending { color: var(--text-dim); }

  .pr-link {
    color: var(--accent);
    text-decoration: none;
    font-size: 0.72rem;
    font-family: var(--font-mono);
  }
  .pr-link:hover {
    text-decoration: underline;
  }

  .dialog-footer {
    padding: 0 0.85rem 0.75rem;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 0.6rem;
  }

  .shortcut-hint {
    font-size: 0.68rem;
    color: var(--text-dim);
    opacity: 0.6;
    margin-right: auto;
  }

  .shortcut-hint kbd {
    font-family: var(--font-mono);
    font-size: 0.64rem;
    padding: 0.08rem 0.25rem;
    background: color-mix(in srgb, var(--border) 50%, transparent);
    border: 0.5px solid var(--border);
    border-radius: 3px;
  }

  .go-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.45rem 0.85rem;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 6px;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }

  .go-btn:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }
</style>

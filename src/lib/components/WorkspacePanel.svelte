<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import type { WorkspaceInfo, RepoSettings, PrStatus } from "$lib/ipc";
  import type { ReviewState } from "$lib/components/ReviewPill.svelte";
  import type { ChatPanelApi, QueueDisplayItem, PastedImage } from "$lib/components/ChatPanel.svelte";
  import type { Mention } from "$lib/components/MentionInput.svelte";
  import { ExternalLink, Check, Loader, GitPullRequestCreate, GitMerge, ArrowUp, ArrowDown, AlertTriangle, Wrench, Eye } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import ChatPanel from "$lib/components/ChatPanel.svelte";
  import DiffViewer from "$lib/components/DiffViewer.svelte";
  import FileBrowser from "$lib/components/FileBrowser.svelte";
  import TerminalView from "$lib/components/Terminal.svelte";
  import ReviewPill from "$lib/components/ReviewPill.svelte";

  export type PanelTab = "chat" | "diff" | "files" | "terminal";

  interface Props {
    activeTab: PanelTab;
    fileNavigatePath: string | null;
    selectedWs: WorkspaceInfo | undefined;
    selectedWsId: string | null;
    activeWorkspaces: WorkspaceInfo[];
    creatingWsId: string | null;
    changeCounts: SvelteMap<string, { additions: number; deletions: number }>;
    planModeByWorkspace: SvelteMap<string, boolean>;
    thinkingModeByWorkspace: SvelteMap<string, boolean>;
    reviewByWorkspace: SvelteMap<string, ReviewState>;
    repoSettings: RepoSettings | null;
    diffRefreshTrigger: number;
    prStatus: PrStatus | undefined;
    wsChanges: { additions: number; deletions: number } | undefined;
    baseBehindBy: number;
    updatingBranch: boolean;
    onPrAction: () => void;
    onUpdateBranch: () => void;
    onReview: () => void;
    reviewRunning: boolean;
    operationInProgress: boolean;
    getQueueItems: (wsId: string) => QueueDisplayItem[];
    onSend: (prompt: string, images: PastedImage[], mentions: Mention[], planMode: boolean) => void;
    onSendImmediate: (prompt: string) => void;
    onStop: () => void;
    onRemoveFromQueue: (wsId: string, id: string) => void;
    onPlanModeChange: (wsId: string, enabled: boolean) => void;
    onThinkingModeChange: (wsId: string, enabled: boolean) => void;
    onExecutePlan: (wsId: string) => void;
    onChatReady: (wsId: string, api: ChatPanelApi) => void;
    onReviewCancel: (wsId: string) => void;
    onReviewSendToChat: (wsId: string, markdown: string) => void;
  }

  let {
    activeTab = $bindable("chat"),
    fileNavigatePath = $bindable(null),
    selectedWs,
    selectedWsId,
    activeWorkspaces,
    creatingWsId,
    changeCounts,
    planModeByWorkspace,
    thinkingModeByWorkspace,
    reviewByWorkspace,
    repoSettings,
    diffRefreshTrigger,
    prStatus,
    wsChanges,
    baseBehindBy,
    updatingBranch,
    onPrAction,
    onUpdateBranch,
    onReview,
    reviewRunning,
    operationInProgress,
    getQueueItems,
    onSend,
    onSendImmediate,
    onStop,
    onRemoveFromQueue,
    onPlanModeChange,
    onThinkingModeChange,
    onExecutePlan,
    onChatReady,
    onReviewCancel,
    onReviewSendToChat,
  }: Props = $props();

  let isBusy = $derived(selectedWs?.status === "running" || reviewRunning || operationInProgress);
</script>

<main class="panel">
  {#if selectedWs}
    <div class="tab-bar">
      <div class="tabs">
        {#each ["chat", "diff", "files", "terminal"] as tab}
          <button
            class="tab"
            class:active={activeTab === tab}
            onclick={() => { activeTab = tab as PanelTab; if (tab !== "files") fileNavigatePath = null; }}
          >
            {tab.charAt(0).toUpperCase() + tab.slice(1)}
            {#if tab === "diff" && changeCounts.get(selectedWs.id)}
              {@const cc = changeCounts.get(selectedWs.id)}
              {#if cc && (cc.additions > 0 || cc.deletions > 0)}
                <span class="diff-badge">
                  <span class="diff-add">+{cc.additions}</span>
                  <span class="diff-del">-{cc.deletions}</span>
                </span>
              {/if}
            {/if}
          </button>
        {/each}
      </div>

      <div class="tab-actions">
        {#if baseBehindBy > 0}
          <button
            class="action-badge update-branch"
            onclick={onUpdateBranch}
            disabled={isBusy || updatingBranch}
            title="Merge {baseBehindBy} new commit{baseBehindBy === 1 ? '' : 's'} from base branch"
          >
            {#if updatingBranch}<Loader size={11} class="status-icon spinning" />{:else}<ArrowDown size={11} />{/if}
            Update{#if !updatingBranch}&nbsp;<span class="update-count">{baseBehindBy}</span>{/if}
          </button>
        {/if}
        {#if prStatus?.state === "open"}
          <div class="action-group">
            <button class="pr-link-btn" onclick={() => openUrl(prStatus!.url)} title="Open PR #{prStatus.number} in browser">
              <ExternalLink size={12} />
            </button>
            <button class="action-badge review" onclick={onReview} disabled={isBusy}>
              <Eye size={11} /> Review
            </button>
            {#if prStatus.mergeable === "conflicting"}
              <button class="action-badge conflicts" onclick={onPrAction} disabled={isBusy}><AlertTriangle size={11} /> Resolve Conflicts</button>
            {:else if prStatus.checks === "failing"}
              <button class="action-badge checks-fail" onclick={onPrAction} disabled={isBusy}><Wrench size={11} /> Fix issues</button>
            {:else if prStatus.checks === "pending"}
              <span class="status-label checks-pending"><Loader size={10} class="status-icon spinning" /> Checks pending</span>
            {:else if (prStatus.ahead_by ?? 0) > 0}
              <button class="action-badge push-needed" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<ArrowUp size={11} />{/if} Push</button>
            {:else if wsChanges && (wsChanges.additions !== prStatus.additions || wsChanges.deletions !== prStatus.deletions)}
              <button class="action-badge push-needed" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<ArrowUp size={11} />{/if} Commit & push</button>
            {:else}
              <button class="action-badge mergeable" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<GitMerge size={11} />{/if} Merge</button>
            {/if}
          </div>
        {:else if prStatus?.state === "merged"}
          <span class="status-label merged"><Check size={10} class="status-icon" /> Done</span>
        {:else if wsChanges && (wsChanges.additions > 0 || wsChanges.deletions > 0)}
          <div class="action-group">
            <button class="action-badge review" onclick={onReview} disabled={isBusy}>
              <Eye size={11} /> Review
            </button>
            <button class="action-badge create-pr" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<Loader size={11} class="status-icon spinning" />{:else}<GitPullRequestCreate size={11} />{/if} Push & create PR</button>
          </div>
        {/if}
      </div>
    </div>

    <div class="tab-content">
      <!-- Chat: always mounted, stacked via absolute positioning.
           Visibility toggle = no reflow. display:none → flex forces full layout recomputation. -->
      {#each activeWorkspaces as ws (ws.id)}
        {@const isVisible = activeTab === "chat" && ws.id === selectedWsId}
        <div
          class="ws-chat-layer"
          class:visible={isVisible}
          inert={!isVisible}
        >
          <ChatPanel
            workspaceId={ws.id}
            creating={ws.id === creatingWsId}
            planMode={planModeByWorkspace.get(ws.id) ?? repoSettings?.default_plan ?? false}
            thinkingMode={thinkingModeByWorkspace.get(ws.id) ?? repoSettings?.default_thinking ?? false}
            queue={getQueueItems(ws.id)}
            onSend={(prompt, images, mentions, planMode) => onSend(prompt, images, mentions, planMode)}
            onSendImmediate={(prompt) => onSendImmediate(prompt)}
            {onStop}
            onRemoveFromQueue={(id) => { if (ws.id) onRemoveFromQueue(ws.id, id); }}
            onPlanModeChange={(enabled) => onPlanModeChange(ws.id, enabled)}
            onThinkingModeChange={(enabled) => onThinkingModeChange(ws.id, enabled)}
            onExecutePlan={() => onExecutePlan(ws.id)}
            onMentionClick={(path) => { fileNavigatePath = path; activeTab = "files"; }}
            onReady={(api) => onChatReady(ws.id, api)}
          />
          {#if reviewByWorkspace.has(ws.id)}
            <ReviewPill
              state={reviewByWorkspace.get(ws.id)!}
              onCancel={() => onReviewCancel(ws.id)}
              onSendToChat={(markdown) => {
                activeTab = "chat";
                onReviewSendToChat(ws.id, markdown);
              }}
            />
          {/if}
        </div>
      {/each}

      <!-- Diff/Terminal: mount on demand, positioned absolute to fill tab-content -->
      {#if activeTab === "diff" && selectedWs}
        <div class="ws-tab-container active-layer">
          <DiffViewer
            workspaceId={selectedWs.id}
            refreshTrigger={diffRefreshTrigger}
          />
        </div>
      {/if}

      <!-- Files: mount on demand like diff -->
      {#if activeTab === "files" && selectedWs}
        <div class="ws-tab-container active-layer">
          <FileBrowser workspaceId={selectedWs.id} navigateTo={fileNavigatePath} />
        </div>
      {/if}

      <!-- Terminal: always mounted per workspace, toggle display.
           Uses display:none (not visibility:hidden) so xterm.js only
           inits when it has real dimensions via ResizeObserver. -->
      {#each activeWorkspaces as ws (ws.id)}
        {@const isVisible = activeTab === "terminal" && ws.id === selectedWsId}
        <div
          class="ws-terminal-layer"
          class:visible={isVisible}
          inert={!isVisible}
        >
          <TerminalView workspaceId={ws.id} />
        </div>
      {/each}
    </div>
  {:else}
    <div class="panel-empty">
      <p>Create a workspace to start an agent.</p>
    </div>
  {/if}
</main>

<style>
  /* ── Main panel ──────────────────────────────────── */

  .panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .panel-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  /* ── Tab bar ───────────────────────────────────── */

  .tab-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 1rem;
    height: 38px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .tabs {
    display: flex;
    gap: 0.15rem;
  }

  .tab {
    padding: 0.35rem 0.65rem;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .diff-badge {
    font-size: 0.65rem;
    font-family: var(--font-mono);
    display: flex;
    gap: 0.2rem;
  }

  .diff-add {
    color: var(--diff-add);
  }

  .diff-del {
    color: var(--diff-del);
  }

  .tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .tab.active {
    color: var(--text-bright);
    background: var(--border);
  }

  /* ── Tab actions (right side of tab bar) ────────── */

  .tab-actions {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .action-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--border-light);
    border-radius: 5px;
  }

  .pr-link-btn {
    background: var(--bg-card);
    color: var(--text-secondary);
    cursor: pointer;
    padding: 0.35rem 0.45rem;
    border: none;
    border-right: 1px solid var(--border-light);
    border-radius: 4px 0 0 4px;
    display: flex;
    align-items: center;
  }

  .pr-link-btn:hover {
    color: var(--text-primary);
    background: var(--border);
  }

  .action-badge {
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.3rem 0.5rem;
    border-radius: 5px;
    border: 1px solid;
    cursor: pointer;
    font-family: inherit;
    background: transparent;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .action-group .action-badge,
  .action-group .status-label {
    border: none;
    border-radius: 0 4px 4px 0;
  }

  .action-badge:disabled {
    opacity: 0.35;
    cursor: not-allowed;
    pointer-events: none;
  }

  .action-badge.create-pr,
  .action-badge.push-needed {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
  }

  .action-badge.create-pr:hover:not(:disabled),
  .action-badge.push-needed:hover:not(:disabled) {
    filter: brightness(1.2);
  }

  .action-badge.mergeable {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 40%, transparent);
    background: color-mix(in srgb, var(--status-ok) 7%, transparent);
  }

  .action-badge.mergeable:hover:not(:disabled) {
    filter: brightness(1.2);
  }

  .action-badge.checks-fail {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 40%, transparent);
    background: color-mix(in srgb, var(--diff-del) 7%, transparent);
  }

  .action-badge.checks-fail:hover:not(:disabled) {
    filter: brightness(1.2);
  }

  .action-badge.review {
    color: var(--text-secondary);
    border-color: var(--border-light);
    background: var(--bg-card);
    border-left: none;
    border-right: 1px solid var(--border-light);
    border-radius: 0;
  }

  .action-badge.review:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--border);
  }

  .action-badge.update-branch {
    color: var(--text-secondary);
    border-color: var(--border-light);
    background: var(--bg-card);
  }

  .action-badge.update-branch:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--border);
  }

  .update-count {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    opacity: 0.7;
  }

  .action-badge.conflicts {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 40%, transparent);
    background: color-mix(in srgb, var(--diff-del) 7%, transparent);
  }

  .action-badge.conflicts:hover:not(:disabled) {
    filter: brightness(1.2);
  }

  .status-label {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--text-dim);
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .action-group .status-label {
    padding: 0.3rem 0.5rem;
  }

  .status-label.merged {
    color: var(--status-ok);
  }

  .status-label.checks-pending {
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .status-label :global(.status-icon) {
    flex-shrink: 0;
  }

  .status-label :global(.status-icon.spinning) {
    animation: spin 1.5s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  @keyframes badge-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  /* ── Tab content ──────────────────────────────────── */

  .tab-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
    position: relative;
  }

  /* Chat layers: stacked absolutely so all stay laid out.
     Switching = visibility toggle (compositor-only, no reflow). */
  .ws-chat-layer {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    visibility: hidden;
    pointer-events: none;
    z-index: 0;
  }

  .ws-chat-layer.visible {
    visibility: visible;
    pointer-events: auto;
    z-index: 1;
  }

  .ws-tab-container {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  /* Diff/terminal: also absolute to coexist with stacked chat layers */
  .ws-tab-container.active-layer {
    position: absolute;
    inset: 0;
    z-index: 2;
  }

  /* Terminal layers: kept alive per workspace, toggled via display.
     display:none gives zero dimensions so xterm.js defers init until visible. */
  .ws-terminal-layer {
    position: absolute;
    inset: 0;
    display: none;
    flex-direction: column;
    z-index: 0;
  }

  .ws-terminal-layer.visible {
    display: flex;
    z-index: 2;
  }
</style>

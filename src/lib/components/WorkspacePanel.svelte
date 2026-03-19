<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import type { WorkspaceInfo, RepoSettings } from "$lib/ipc";
  import type { ReviewState } from "$lib/components/ReviewPill.svelte";
  import type { ChatPanelApi, QueueDisplayItem, PastedImage } from "$lib/components/ChatPanel.svelte";
  import type { Mention } from "$lib/components/MentionInput.svelte";
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

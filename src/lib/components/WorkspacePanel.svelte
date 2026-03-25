<script lang="ts">
  import { SvelteMap } from "svelte/reactivity";
  import type { WorkspaceInfo, RepoSettings, PrStatus, ScriptEvent, NamedScript } from "$lib/ipc";
  import { runScript, stopScript, closeTerminal } from "$lib/ipc";
  import type { ReviewState } from "$lib/components/ReviewPill.svelte";
  import type { ChatPanelApi, QueueDisplayItem, PastedImage } from "$lib/chat-utils";
  import type { Mention } from "$lib/components/MentionInput.svelte";
  import { ExternalLink, Check, GitPullRequestCreate, GitMerge, ArrowUp, ArrowDown, AlertTriangle, Wrench, Eye, Play, Square, CircleX, MessageSquare, Minus, ChevronUp, ChevronDown, Timer, RefreshCcw, Plus, X } from "lucide-svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import ChatPanel from "$lib/components/ChatPanel.svelte";
  import DiffViewer from "$lib/components/DiffViewer.svelte";
  import FileBrowser from "$lib/components/FileBrowser.svelte";
  import TerminalView from "$lib/components/Terminal.svelte";
  import ReviewPill from "$lib/components/ReviewPill.svelte";
  import ResizeHandle from "$lib/components/ResizeHandle.svelte";
  import { draggable, resizable, tooltip, type DragOffset } from "$lib/actions";

  export type PanelTab = "diff" | "files";

  interface Props {
    activeTab: PanelTab;
    fileNavigatePath: string | null;
    fileNavigateLine: number | null;
    selectedWs: WorkspaceInfo | undefined;
    selectedWsId: string | null;
    activeWorkspaces: WorkspaceInfo[];
    creatingWsId: string | null;
    changeCounts: SvelteMap<string, { additions: number; deletions: number }>;
    planModeByWorkspace: SvelteMap<string, boolean>;
    thinkingModeByWorkspace: SvelteMap<string, boolean>;
    reviewByWorkspace: SvelteMap<string, ReviewState>;
    agentTaskByWorkspace: SvelteMap<string, string>;
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
    defaultBranch: string;
    chatExpanded: boolean;
    onChatExpandedChange: (expanded: boolean) => void;
    onDiffQuote?: (text: string) => void;
    isStaging?: boolean;
    stagingMergedCount?: number;
    stagingConflictingCount?: number;
    contextWarning?: boolean;
  }

  let {
    activeTab = $bindable("diff"),
    fileNavigatePath = $bindable(null),
    fileNavigateLine = $bindable(null),
    selectedWs,
    selectedWsId,
    activeWorkspaces,
    creatingWsId,
    changeCounts,
    planModeByWorkspace,
    thinkingModeByWorkspace,
    reviewByWorkspace,
    agentTaskByWorkspace,
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
    defaultBranch,
    chatExpanded,
    onChatExpandedChange,
    onDiffQuote,
    isStaging = false,
    stagingMergedCount = 0,
    stagingConflictingCount = 0,
    contextWarning = false,
  }: Props = $props();

  let availableTabs = $derived(
    isStaging ? ["files"] as const : ["diff", "files"] as const
  );

  let isBusy = $derived(selectedWs?.status === "running" || reviewRunning || operationInProgress);

  // ── ANSI → HTML converter ───────────────────────────────────────
  // Maps ANSI SGR color codes to CSS custom properties (set by theme system)
  const ANSI_COLORS: Record<number, string> = {
    30: "var(--ansi-black)",   31: "var(--ansi-red)",     32: "var(--ansi-green)",   33: "var(--ansi-yellow)",
    34: "var(--ansi-blue)",    35: "var(--ansi-magenta)", 36: "var(--ansi-cyan)",    37: "var(--ansi-white)",
    90: "var(--ansi-bright-black)",  91: "var(--ansi-bright-red)",     92: "var(--ansi-bright-green)",  93: "var(--ansi-bright-yellow)",
    94: "var(--ansi-bright-blue)",   95: "var(--ansi-bright-magenta)", 96: "var(--ansi-bright-cyan)",   97: "var(--ansi-bright-white)",
  };

  function ansiToHtml(text: string): string {
    // Escape HTML entities first
    let html = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    let result = "";
    let open = false;

    // Split on ANSI escape sequences, keeping the delimiters
    const parts = html.split(/(\x1b\[[0-9;]*m)/);
    for (const part of parts) {
      const m = part.match(/^\x1b\[([0-9;]*)m$/);
      if (m) {
        const codes = m[1].split(";").map(Number);
        if (open) { result += "</span>"; open = false; }
        const styles: string[] = [];
        for (const c of codes) {
          if (c === 0) continue; // reset — span already closed
          if (c === 1) styles.push("font-weight:bold");
          else if (c === 2) styles.push("opacity:0.6");
          else if (c === 3) styles.push("font-style:italic");
          else if (c === 4) styles.push("text-decoration:underline");
          else if (ANSI_COLORS[c]) styles.push(`color:${ANSI_COLORS[c]}`);
        }
        if (styles.length) {
          result += `<span style="${styles.join(";")}">`;
          open = true;
        }
      } else {
        result += part;
      }
    }
    if (open) result += "</span>";
    return result;
  }

  // ── Script runner state ──────────────────────────────────────────
  type ScriptStatus = "idle" | "running" | "success" | "error";
  let scriptStatusMap = new SvelteMap<string, ScriptStatus>();
  let scriptOutputMap = new SvelteMap<string, string[]>();
  let scriptExitCodeMap = new SvelteMap<string, number | null>();
  let scriptPopoverOpen = new SvelteMap<string, boolean>();

  let currentScriptStatus = $derived(
    selectedWsId ? scriptStatusMap.get(selectedWsId) ?? "idle" : "idle"
  );
  let currentScriptOutput = $derived(
    selectedWsId ? scriptOutputMap.get(selectedWsId) ?? [] : []
  );
  let currentScriptExitCode = $derived(
    selectedWsId ? scriptExitCodeMap.get(selectedWsId) ?? null : null
  );
  let isPopoverOpen = $derived(
    selectedWsId ? scriptPopoverOpen.get(selectedWsId) ?? false : false
  );
  let scriptDropdownOpen = new SvelteMap<string, boolean>();
  let runningScriptName = new SvelteMap<string, string>();
  let isDropdownOpen = $derived(
    selectedWsId ? scriptDropdownOpen.get(selectedWsId) ?? false : false
  );
  let currentRunningName = $derived(
    selectedWsId ? runningScriptName.get(selectedWsId) ?? "" : ""
  );
  let hasRunScripts = $derived(
    (repoSettings?.run_scripts?.length ?? 0) > 0 &&
    repoSettings!.run_scripts.some((s: NamedScript) => s.command.trim())
  );

  let outputEl = $state<HTMLPreElement | null>(null);

  function scrollOutputToBottom() {
    if (outputEl) {
      outputEl.scrollTop = outputEl.scrollHeight;
    }
  }

  // ── Floating panel focus & drag offsets ─────────────────────────
  let focusedPanel = $state<"review" | "chat">("chat");
  let reviewDragOffset = $state<DragOffset>({ x: 0, y: 0 });
  let chatDragOffset = $state<DragOffset>({ x: 0, y: 0 });
  let chatSize = $state<{ w: number; h: number } | null>(null);
  let resizeStartDrag = { x: 0, y: 0 };

  function onChatResizeStart(size: { w: number; h: number }) {
    if (!chatSize) chatSize = size;
    resizeStartDrag = { ...chatDragOffset };
  }

  function onChatResize(size: { w: number; h: number }, posDelta: { dx: number; dy: number }) {
    chatSize = size;
    chatDragOffset = { x: resizeStartDrag.x + posDelta.dx, y: resizeStartDrag.y + posDelta.dy };
  }

  let terminalPaneWidth = $state(400);
  const TERMINAL_PANE_MIN = 200;
  const TERMINAL_PANE_MAX = 800;

  function handleTerminalResize(delta: number) {
    terminalPaneWidth = Math.min(TERMINAL_PANE_MAX, Math.max(TERMINAL_PANE_MIN, terminalPaneWidth - delta));
  }

  // ── Multi-terminal tab state ──────────────────────────────────────

  interface TerminalTab {
    id: string;
    label: string;
  }

  let terminalTabs = new SvelteMap<string, TerminalTab[]>();
  let activeTerminalTab = new SvelteMap<string, string>();
  let terminalCounters = new SvelteMap<string, number>();

  // Ensure every active workspace has at least one terminal tab
  $effect(() => {
    for (const ws of activeWorkspaces) {
      if (!terminalTabs.has(ws.id)) {
        const id = crypto.randomUUID();
        terminalTabs.set(ws.id, [{ id, label: "Terminal 1" }]);
        activeTerminalTab.set(ws.id, id);
        terminalCounters.set(ws.id, 1);
      }
    }
  });

  // Clean up stale entries when workspaces are removed
  $effect(() => {
    const wsIds = new Set(activeWorkspaces.map((ws) => ws.id));
    for (const id of terminalTabs.keys()) {
      if (!wsIds.has(id)) {
        terminalTabs.delete(id);
        activeTerminalTab.delete(id);
        terminalCounters.delete(id);
      }
    }
  });

  function addTerminalTab(wsId: string) {
    const count = (terminalCounters.get(wsId) ?? 0) + 1;
    terminalCounters.set(wsId, count);
    const id = crypto.randomUUID();
    const tabs = terminalTabs.get(wsId) ?? [];
    terminalTabs.set(wsId, [...tabs, { id, label: `Terminal ${count}` }]);
    activeTerminalTab.set(wsId, id);
  }

  function removeTerminalTab(wsId: string, tabId: string) {
    const tabs = terminalTabs.get(wsId);
    if (!tabs || tabs.length <= 1) return;

    const idx = tabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;

    const newTabs = tabs.filter((t) => t.id !== tabId);
    terminalTabs.set(wsId, newTabs);

    // Close backend PTY
    closeTerminal(wsId, tabId).catch(() => {});

    // Switch active tab if needed
    if (activeTerminalTab.get(wsId) === tabId) {
      const newIdx = Math.min(idx, newTabs.length - 1);
      activeTerminalTab.set(wsId, newTabs[newIdx].id);
    }
  }

  let currentTerminalTabs = $derived(
    selectedWsId ? terminalTabs.get(selectedWsId) ?? [] : []
  );

  let currentActiveTermTab = $derived(
    selectedWsId ? activeTerminalTab.get(selectedWsId) ?? null : null
  );

  function handleRunNamedScript(script: NamedScript) {
    if (!selectedWs || !script.command.trim()) return;
    const wsId = selectedWs.id;
    scriptStatusMap.set(wsId, "running");
    scriptOutputMap.set(wsId, []);
    scriptExitCodeMap.delete(wsId);
    scriptPopoverOpen.set(wsId, true);
    scriptDropdownOpen.set(wsId, false);
    runningScriptName.set(wsId, script.name || script.command);

    runScript(wsId, script.command, (event: ScriptEvent) => {
      if (event.type === "output") {
        const prev = scriptOutputMap.get(wsId) ?? [];
        const lines = prev.length >= 500 ? [...prev.slice(1), event.data] : [...prev, event.data];
        scriptOutputMap.set(wsId, lines);
        requestAnimationFrame(scrollOutputToBottom);
      } else if (event.type === "exit") {
        const ok = event.code === 0;
        scriptExitCodeMap.set(wsId, event.code);
        scriptStatusMap.set(wsId, ok ? "success" : "error");
        setTimeout(() => {
          if (scriptStatusMap.get(wsId) === (ok ? "success" : "error")) {
            scriptStatusMap.set(wsId, "idle");
          }
        }, 2000);
      }
    }).catch(() => {
      scriptExitCodeMap.set(wsId, -1);
      scriptStatusMap.set(wsId, "error");
      setTimeout(() => {
        if (scriptStatusMap.get(wsId) === "error") {
          scriptStatusMap.set(wsId, "idle");
        }
      }, 2000);
    });
  }

  function handleRunDefault() {
    const scripts = repoSettings?.run_scripts ?? [];
    const first = scripts.find((s: NamedScript) => s.command.trim());
    if (first) handleRunNamedScript(first);
  }

  async function handleStopScript() {
    if (!selectedWs) return;
    try {
      await stopScript(selectedWs.id);
    } catch {
      // Process may have already exited — the exit event will handle state cleanup
    }
  }

  function toggleDropdown() {
    if (!selectedWsId) return;
    const open = scriptDropdownOpen.get(selectedWsId) ?? false;
    scriptDropdownOpen.set(selectedWsId, !open);
  }

  function closePopover() {
    if (!selectedWsId) return;
    scriptPopoverOpen.set(selectedWsId, false);
  }

  function handlePopoverClickOutside(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (!target.closest(".run-script-wrapper")) {
      closePopover();
      if (selectedWsId) scriptDropdownOpen.set(selectedWsId, false);
    }
  }
</script>

<svelte:window onclick={handlePopoverClickOutside} />

<main class="panel">
  {#if selectedWs}
    <!-- Action bar: breadcrumb + Run + PR actions -->
    <div class="action-bar">
      <span class="breadcrumb">
        <span class="breadcrumb-branch">{selectedWs.branch}</span>
        <span class="breadcrumb-sep">›</span>
        <span class="breadcrumb-base">{defaultBranch}</span>
      </span>
      {#if hasRunScripts}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="run-script-wrapper" onclick={(e) => e.stopPropagation()}>
          <div class="run-script-group">
            {#if currentScriptStatus === "running"}
              <button
                class="run-script-btn stop"
                onclick={handleStopScript}
                use:tooltip={{ text: "Stop script" }}
              >
                <Square size={10} />
                Stop {#if selectedWs}<span class="run-branch">{selectedWs.branch}</span>{/if}
              </button>
            {:else}
              <button
                class="run-script-btn"
                class:success={currentScriptStatus === "success"}
                class:error={currentScriptStatus === "error"}
                onclick={handleRunDefault}
                use:tooltip={{ text: `Run: ${repoSettings?.run_scripts?.[0]?.name || repoSettings?.run_scripts?.[0]?.command || "Script"}` }}
              >
                {#if currentScriptStatus === "success"}
                  <Check size={12} />
                {:else if currentScriptStatus === "error"}
                  <CircleX size={12} />
                {:else}
                  <Play size={12} />
                {/if}
                Run {#if selectedWs}<span class="run-branch">{selectedWs.branch}</span>{/if}
              </button>
            {/if}
            <button
              class="run-script-toggle"
              class:running={currentScriptStatus === "running"}
              class:success={currentScriptStatus === "success"}
              class:error={currentScriptStatus === "error"}
              onclick={toggleDropdown}
              use:tooltip={{ text: "Select script" }}
            >
              <ChevronDown size={10} />
            </button>
          </div>

          {#if isDropdownOpen}
            <div class="script-dropdown">
              {#each repoSettings?.run_scripts ?? [] as script, i}
                {#if script.command.trim()}
                  <button
                    class="script-dropdown-item"
                    onclick={() => handleRunNamedScript(script)}
                    disabled={currentScriptStatus === "running"}
                  >
                    <Play size={11} />
                    <span class="script-dropdown-name">{script.name || script.command}</span>
                    {#if i === 0}<span class="script-dropdown-default">default</span>{/if}
                  </button>
                {/if}
              {/each}
              {#if currentScriptOutput.length > 0 && !isPopoverOpen}
                <div class="script-dropdown-divider"></div>
                <button
                  class="script-dropdown-item"
                  onclick={() => { if (selectedWsId) { scriptPopoverOpen.set(selectedWsId, true); scriptDropdownOpen.set(selectedWsId, false); } }}
                >
                  <span class="script-dropdown-name">Show last output</span>
                </button>
              {/if}
            </div>
          {/if}

          {#if isPopoverOpen}
            <div class="script-popover">
              <div class="script-popover-header">
                <span class="script-popover-title">
                  {#if currentScriptStatus === "running"}
                    <span class="btn-spinner btn-spinner-sm"></span>
                  {/if}
                  {currentRunningName || "Script"}
                </span>
                <button class="script-popover-close" onclick={closePopover}>
                  <X size={12} />
                </button>
              </div>
              <pre class="script-popover-output" bind:this={outputEl}>{#each currentScriptOutput as line}{@html ansiToHtml(line)}{/each}</pre>
              {#if currentScriptExitCode !== null}
                <div class="script-popover-exit" class:ok={currentScriptExitCode === 0} class:fail={currentScriptExitCode !== 0}>
                  {currentScriptExitCode === 0 ? "✓ Exited successfully" : `✗ Exit code ${currentScriptExitCode}`}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}

      <div class="tab-actions">
        {#if baseBehindBy > 0}
          <button
            class="action-badge update-branch"
            onclick={onUpdateBranch}
            disabled={isBusy || updatingBranch}
            use:tooltip={{ text: `Merge ${baseBehindBy} new commit${baseBehindBy === 1 ? '' : 's'} from base branch` }}
          >
            {#if updatingBranch}<span class="btn-spinner"></span>{:else}<ArrowDown size={11} />{/if}
            Update{#if !updatingBranch}&nbsp;<span class="update-count">{baseBehindBy}</span>{/if}
          </button>
        {/if}
        {#if prStatus?.state === "open"}
          <div class="action-group">
            <button class="pr-link-btn" onclick={() => openUrl(prStatus!.url)} use:tooltip={{ text: `Open PR #${prStatus.number} in browser` }}>
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
              <span class="status-label checks-pending"><span class="btn-spinner btn-spinner-sm"></span> Checks pending</span>
            {:else if (prStatus.ahead_by ?? 0) > 0}
              <button class="action-badge push-needed" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<span class="btn-spinner"></span>{:else}<ArrowUp size={11} />{/if} Push</button>
            {:else if wsChanges && (wsChanges.additions !== prStatus.additions || wsChanges.deletions !== prStatus.deletions)}
              <button class="action-badge push-needed" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<span class="btn-spinner"></span>{:else}<ArrowUp size={11} />{/if} Commit & push</button>
            {:else}
              <button class="action-badge mergeable" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<span class="btn-spinner"></span>{:else}<GitMerge size={11} />{/if} Merge</button>
            {/if}
          </div>
        {:else if prStatus?.state === "merged"}
          <span class="status-label merged"><Check size={10} class="status-icon" /> Done</span>
        {:else if wsChanges && (wsChanges.additions > 0 || wsChanges.deletions > 0)}
          <div class="action-group">
            <button class="action-badge review" onclick={onReview} disabled={isBusy}>
              <Eye size={11} /> Review
            </button>
            <button class="action-badge create-pr" onclick={onPrAction} disabled={isBusy}>{#if operationInProgress}<span class="btn-spinner"></span>{:else}<GitPullRequestCreate size={11} />{/if} Push & create PR</button>
          </div>
        {/if}
      </div>
    </div>

    {#if isStaging}
      <div class="staging-banner">
        <span class="staging-label">Staging</span>
        <span class="staging-info">{stagingMergedCount} branch{stagingMergedCount === 1 ? '' : 'es'} merged</span>
        {#if stagingConflictingCount > 0}
          <span class="staging-conflict">{stagingConflictingCount} skipped (conflicts)</span>
        {/if}
      </div>
    {/if}
    <div class="tab-content">
      <!-- Left pane: Diff / Files -->
      <div class="content-left">
        <div class="pane-tabs">
          {#each availableTabs as tab}
            <button
              class="pane-tab"
              class:active={activeTab === tab}
              onclick={() => { activeTab = tab as PanelTab; if (tab !== "files") { fileNavigatePath = null; fileNavigateLine = null; } }}
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
        <div class="content-left-body">
          {#if selectedWs}
            <div class="ws-tab-container active-layer" style:display={activeTab === "diff" ? undefined : "none"}>
              <DiffViewer
                workspaceId={selectedWs.id}
                refreshTrigger={diffRefreshTrigger}
                onQuote={onDiffQuote}
                onOpenFile={(path) => { fileNavigatePath = path; activeTab = "files"; }}
                onGoToLine={(path, line) => { fileNavigatePath = path; fileNavigateLine = line; activeTab = "files"; }}
              />
            </div>
            <div class="ws-tab-container active-layer" style:display={activeTab === "files" ? undefined : "none"}>
              <FileBrowser scope={{ type: "workspace", workspaceId: selectedWs.id }} navigateTo={fileNavigatePath} navigateToLine={fileNavigateLine} />
            </div>
          {/if}
        </div>
      </div>

      <ResizeHandle onResize={handleTerminalResize} />

      <!-- Right pane: Terminal -->
      <div class="terminal-pane" style="width: {terminalPaneWidth}px">
        <div class="pane-tabs terminal-tabs">
          {#each currentTerminalTabs as tab (tab.id)}
            <button
              class="pane-tab"
              class:active={tab.id === currentActiveTermTab}
              onclick={() => selectedWsId && activeTerminalTab.set(selectedWsId, tab.id)}
            >
              {tab.label}
              {#if currentTerminalTabs.length > 1}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <span
                  class="tab-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e) => { e.stopPropagation(); selectedWsId && removeTerminalTab(selectedWsId, tab.id); }}
                >×</span>
              {/if}
            </button>
          {/each}
          <button
            class="term-add-btn"
            use:tooltip={{ text: "New terminal" }}
            onclick={() => selectedWsId && addTerminalTab(selectedWsId)}
          >
            <Plus size={12} />
          </button>
        </div>
        <div class="terminal-body">
          {#each activeWorkspaces as ws (ws.id)}
            {#each (terminalTabs.get(ws.id) ?? []) as tab (tab.id)}
              {@const isVisible = ws.id === selectedWsId && tab.id === activeTerminalTab.get(ws.id)}
              <div
                class="ws-terminal-layer"
                class:visible={isVisible}
                inert={!isVisible}
              >
                <TerminalView scope={{ type: "workspace", workspaceId: ws.id }} terminalId={tab.id} visible={isVisible} />
              </div>
            {/each}
          {/each}
        </div>
      </div>

      <!-- Review pill: floats top-right over left pane -->
      {#if selectedWsId && reviewByWorkspace.has(selectedWsId)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="floating-panel-wrapper review-pos"
          class:panel-focused={focusedPanel === "review"}
          onmousedown={() => { focusedPanel = "review"; }}
          use:draggable={{ offset: reviewDragOffset, onDrag: (o) => { reviewDragOffset = o; } }}
        >
          <ReviewPill
            state={reviewByWorkspace.get(selectedWsId)!}
            onCancel={() => onReviewCancel(selectedWsId!)}
            onSendToChat={(markdown) => {
              onChatExpandedChange(true);
              onReviewSendToChat(selectedWsId!, markdown);
            }}
          />
        </div>
      {/if}

      <!-- Chat overlay: floating panel, per-workspace, always mounted -->
      {#each activeWorkspaces as ws (ws.id)}
        {@const isActive = ws.id === selectedWsId}
        {@const isAgentRunning = ws.status === "running"}
        {#if isActive}
          {#if chatExpanded}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="chat-overlay"
              class:panel-focused={focusedPanel === "chat"}
              onmousedown={() => { focusedPanel = "chat"; }}
              use:draggable={{ handle: ".chat-overlay-header", offset: chatDragOffset, onDrag: (o) => { chatDragOffset = o; } }}
              use:resizable={{ minWidth: 380, minHeight: 280, onResizeStart: onChatResizeStart, onResize: onChatResize }}
              style:width={chatSize ? `${chatSize.w}px` : undefined}
              style:height={chatSize ? `${chatSize.h}px` : undefined}
            >
              <div class="chat-overlay-header">
                <span class="chat-overlay-title">
                  <MessageSquare size={13} strokeWidth={2} />
                  Chat
                </span>
                <div class="chat-overlay-actions">
                  {#if chatDragOffset.x || chatDragOffset.y || chatSize}
                    <button
                      class="chat-overlay-btn"
                      onclick={() => { chatDragOffset = { x: 0, y: 0 }; chatSize = null; }}
                      use:tooltip={{ text: "Reset position and size" }}
                    >
                      <RefreshCcw size={12} />
                    </button>
                  {/if}
                  <button
                    class="chat-overlay-btn"
                    onclick={() => onChatExpandedChange(false)}
                    use:tooltip={{ text: "Collapse chat" }}
                  >
                    <Minus size={14} />
                  </button>
                </div>
              </div>
              <div class="chat-overlay-body">
                <ChatPanel
                  workspaceId={ws.id}
                  creating={ws.id === creatingWsId}
                  planMode={planModeByWorkspace.get(ws.id) ?? repoSettings?.default_plan ?? false}
                  thinkingMode={thinkingModeByWorkspace.get(ws.id) ?? repoSettings?.default_thinking ?? false}
                  queue={getQueueItems(ws.id)}
                  {contextWarning}
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
              </div>
            </div>
          {:else}
            <!-- Collapsed pill -->
            {@const agentTask = agentTaskByWorkspace.get(ws.id)}
            <button
              class="chat-pill"
              class:chat-pill-active={isAgentRunning}
              onclick={() => { focusedPanel = "chat"; onChatExpandedChange(true); }}
              use:tooltip={{ text: "Open chat" }}
              style={chatDragOffset.x || chatDragOffset.y ? `transform: translate(${chatDragOffset.x}px, ${chatDragOffset.y}px)` : ""}
            >
              {#if isAgentRunning}
                <span class="btn-spinner btn-spinner-pill"></span>
              {:else}
                <MessageSquare size={13} strokeWidth={2} />
              {/if}
              <span class="chat-pill-label">{#if isAgentRunning && agentTask}{agentTask}{:else}Chat{/if}</span>
              <ChevronUp size={13} />
            </button>
          {/if}
        {:else}
          <!-- Hidden but mounted for non-active workspaces to preserve state -->
          <div class="chat-overlay-hidden" inert>
            <ChatPanel
              workspaceId={ws.id}
              creating={ws.id === creatingWsId}
              planMode={planModeByWorkspace.get(ws.id) ?? repoSettings?.default_plan ?? false}
              thinkingMode={thinkingModeByWorkspace.get(ws.id) ?? repoSettings?.default_thinking ?? false}
              queue={getQueueItems(ws.id)}
              {contextWarning}
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
          </div>
        {/if}
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

  /* ── Action bar (top) ──────────────────────────── */

  .action-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0 1rem;
    height: 38px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .breadcrumb {
    font-size: 0.75rem;
    color: var(--text-dim);
    display: flex;
    align-items: center;
    gap: 0.35rem;
    min-width: 0;
    overflow: hidden;
  }

  .breadcrumb-branch {
    color: var(--accent);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .breadcrumb-sep {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .breadcrumb-base {
    color: var(--text-dim);
    flex-shrink: 0;
  }

  /* ── Pane tab bars (shared by left + right panes) ── */

  .pane-tabs {
    display: flex;
    align-items: center;
    padding: 0 0.5rem;
    height: 30px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: 0.15rem;
  }

  .pane-tab {
    padding: 0.25rem 0.55rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.72rem;
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

  .pane-tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .pane-tab.active {
    color: var(--text-bright);
    background: var(--border);
  }

  /* ── Run script button ──────────────────────────── */

  .run-branch {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: 500;
    padding: 0.05rem 0.3rem;
    border-radius: 3px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    margin-left: 0.3em;
  }

  .run-script-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    padding: 0 0.55rem;
    height: 26px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 6px;
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--accent);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.15s, border-color 0.15s;
  }

  .run-script-btn:hover:not(:disabled) {
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .run-script-btn:disabled {
    cursor: default;
  }

  .run-script-btn.success {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 30%, transparent);
    background: color-mix(in srgb, var(--status-ok) 12%, transparent);
  }

  .run-script-btn.error {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 30%, transparent);
    background: color-mix(in srgb, var(--diff-del) 12%, transparent);
  }

  .run-script-btn.stop {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 30%, transparent);
    background: color-mix(in srgb, var(--diff-del) 10%, transparent);
  }

  .run-script-btn.stop:hover {
    border-color: color-mix(in srgb, var(--diff-del) 50%, transparent);
    background: color-mix(in srgb, var(--diff-del) 18%, transparent);
  }

  /* ── Run script wrapper + popover ────────────────── */

  .run-script-wrapper {
    position: relative;
  }

  .run-script-group {
    display: flex;
    align-items: stretch;
  }

  .run-script-group .run-script-btn:not(:last-child) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
    border-right: none;
  }

  .run-script-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    padding: 0;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-left: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    border-radius: 0 6px 6px 0;
    color: var(--accent);
    cursor: pointer;
    flex-shrink: 0;
    transition: background 0.15s;
  }

  .run-script-toggle:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .run-script-toggle.success {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 30%, transparent);
    border-left-color: color-mix(in srgb, var(--status-ok) 20%, transparent);
    background: color-mix(in srgb, var(--status-ok) 12%, transparent);
  }

  .run-script-toggle.error {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 30%, transparent);
    border-left-color: color-mix(in srgb, var(--diff-del) 20%, transparent);
    background: color-mix(in srgb, var(--diff-del) 12%, transparent);
  }

  .run-script-toggle.running {
    cursor: default;
  }

  /* ── Script dropdown ──────────────────────────── */

  .script-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 200px;
    background: var(--bg-base);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    z-index: 51;
    overflow: hidden;
    padding: 0.25rem 0;
  }

  .script-dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.35rem 0.6rem;
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: 0.72rem;
    font-family: inherit;
    cursor: pointer;
    text-align: left;
  }

  .script-dropdown-item:hover:not(:disabled) {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .script-dropdown-item:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .script-dropdown-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .script-dropdown-default {
    font-size: 0.6rem;
    color: var(--text-muted);
    background: var(--bg-sidebar);
    padding: 0.05rem 0.35rem;
    border-radius: 3px;
    flex-shrink: 0;
  }

  .script-dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 0.25rem 0;
  }

  .script-popover {
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    width: 420px;
    max-height: 300px;
    background: var(--bg-base);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
    display: flex;
    flex-direction: column;
    z-index: 50;
    overflow: hidden;
  }

  .script-popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.35rem 0.5rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .script-popover-title {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.68rem;
    font-family: var(--font-mono);
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .script-popover-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .script-popover-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .script-popover-output {
    flex: 1;
    margin: 0;
    padding: 0.4rem 0.5rem;
    font-family: var(--font-mono);
    font-size: 0.68rem;
    line-height: 1.45;
    color: var(--text-secondary);
    overflow-y: auto;
    overflow-x: hidden;
    white-space: pre-wrap;
    word-break: break-all;
    min-height: 40px;
    max-height: 230px;
  }

  .script-popover-exit {
    padding: 0.3rem 0.5rem;
    font-size: 0.68rem;
    font-weight: 600;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .script-popover-exit.ok {
    color: var(--status-ok);
    background: color-mix(in srgb, var(--status-ok) 5%, transparent);
  }

  .script-popover-exit.fail {
    color: var(--diff-del);
    background: color-mix(in srgb, var(--diff-del) 5%, transparent);
  }

  .btn-spinner {
    width: 11px;
    height: 11px;
    border: 1.5px solid color-mix(in srgb, currentColor 25%, transparent);
    border-top-color: currentColor;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }

  .btn-spinner-sm {
    width: 9px;
    height: 9px;
  }

  .btn-spinner-pill {
    width: 12px;
    height: 12px;
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
    overflow: hidden;
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
    flex-direction: row;
    min-height: 0;
    position: relative;
  }

  /* ── Left pane: Diff / Files ────────────────────── */

  .content-left {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .content-left-body {
    flex: 1;
    position: relative;
    min-height: 0;
  }

  .ws-tab-container {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .ws-tab-container.active-layer {
    position: absolute;
    inset: 0;
    z-index: 1;
  }

  /* ── Right pane: Terminal ────────────────────────── */

  .terminal-pane {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    min-height: 0;
  }

  .terminal-tabs {
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }

  .terminal-tabs::-webkit-scrollbar {
    display: none;
  }

  .tab-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    border-radius: 3px;
    font-size: 11px;
    line-height: 1;
    color: var(--text-dim);
    cursor: pointer;
    margin-left: 2px;
    flex-shrink: 0;
  }

  .tab-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .term-add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    flex-shrink: 0;
  }

  .term-add-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .terminal-body {
    flex: 1;
    position: relative;
    min-height: 0;
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
    z-index: 1;
  }

  /* ── Floating panel focus (OS-style z-ordering) ── */

  .floating-panel-wrapper {
    position: absolute;
    z-index: 10;
  }

  .floating-panel-wrapper.panel-focused {
    z-index: 11;
  }

  .floating-panel-wrapper.review-pos {
    top: 12px;
    right: 12px;
  }

  /* ── Chat overlay (floating) ───────────────────── */

  .chat-overlay {
    position: absolute;
    bottom: 12px;
    right: 12px;
    width: 380px;
    height: 55%;
    min-height: 280px;
    max-height: calc(100% - 24px);
    z-index: 10;
    display: flex;
    flex-direction: column;
    background: var(--bg-base);
    border: 1px solid var(--border-light);
    border-radius: 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.45);
    overflow: hidden;
  }

  .chat-overlay.panel-focused {
    z-index: 11;
  }

  .chat-overlay-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem 0.65rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .chat-overlay-title {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .chat-overlay-actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .chat-overlay-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .chat-overlay-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .chat-overlay-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  /* Hidden chat panels for non-active workspaces (preserve state) */
  .chat-overlay-hidden {
    position: absolute;
    width: 0;
    height: 0;
    overflow: hidden;
    visibility: hidden;
    pointer-events: none;
  }

  /* ── Collapsed chat pill ───────────────────────── */

  .chat-pill {
    position: absolute;
    bottom: 12px;
    right: 12px;
    z-index: 10;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.4rem 0.65rem;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 20px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 500;
    color: var(--text-secondary);
    transition: background 0.15s, border-color 0.15s;
  }

  .chat-pill:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
    border-color: var(--border);
  }

  .chat-pill-active {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }

  .chat-pill-active:hover {
    background: color-mix(in srgb, var(--accent) 85%, #000);
    border-color: color-mix(in srgb, var(--accent) 85%, #000);
    color: #fff;
  }

  .chat-pill-active .btn-spinner {
    border-color: color-mix(in srgb, #fff 25%, transparent);
    border-top-color: #fff;
  }

  .chat-pill-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
  }

  .staging-banner {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.35rem 1rem;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    flex-shrink: 0;
  }

  .staging-label {
    font-size: 0.72rem;
    font-weight: 700;
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .staging-info {
    font-size: 0.72rem;
    color: var(--text-secondary);
  }

  .staging-conflict {
    font-size: 0.72rem;
    color: var(--diff-del);
    font-weight: 600;
  }
</style>

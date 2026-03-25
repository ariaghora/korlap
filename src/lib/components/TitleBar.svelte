<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { RepoDetail, RepoSettings, NamedScript, ScriptEvent } from "$lib/ipc";
  import { syncMain, getRepoHead, checkoutDefaultBranch, checkMainBehind, runRepoScript, stopRepoScript } from "$lib/ipc";
  import { Settings, Check, Plus, RefreshCw, AlertTriangle, ChevronLeft, Zap, Network, LayoutGrid, FolderOpen, SquareTerminal, Play, Square, CircleX, ChevronDown, X } from "lucide-svelte";
  import { SvelteMap } from "svelte/reactivity";
  import Dropdown from "./Dropdown.svelte";
  import { addToast } from "$lib/stores/toasts.svelte";
  import { tooltip } from "$lib/actions";

  interface Props {
    repos: RepoDetail[];
    activeRepo: RepoDetail;
    inWorkspace: boolean;
    workspaceTitle: string | null;
    onGoToPlan: () => void;
    onSelectRepo: (repo: RepoDetail) => void;
    onSettings: () => void;
    onGoHome: () => void;
    highlightedRepoIndex: number;
    onDropdownClose?: () => void;
    autopilotEnabled?: boolean;
    onAutopilotToggle?: () => void;
    autopilotStatus?: string;
    onShowDepGraph?: () => void;
    planView?: "kanban" | "files" | "terminal";
    onPlanViewChange?: (view: "kanban" | "files" | "terminal") => void;
    repoSettings?: RepoSettings | null;
  }

  let { repos, activeRepo, inWorkspace, workspaceTitle, onGoToPlan, onSelectRepo, onSettings, onGoHome, highlightedRepoIndex, onDropdownClose, autopilotEnabled = false, onAutopilotToggle, autopilotStatus, onShowDepGraph, planView = "kanban", onPlanViewChange, repoSettings = null }: Props =
    $props();

  let dropdownRef: Dropdown | undefined = $state();
  let syncing = $state(false);
  let syncError = $state(false);
  let headBranch: string | null = $state(null);
  let checkingOut = $state(false);
  let behindCount: number = $state(0);
  let checking = $state(false);

  let onDefaultBranch = $derived(
    headBranch === null || headBranch === activeRepo.default_branch
  );

  // Check HEAD branch + behind count when entering plan mode or switching repos
  $effect(() => {
    const repoId = activeRepo.id;
    const viewing = inWorkspace;
    if (!viewing) {
      checking = true;
      getRepoHead(repoId)
        .then((b) => {
          headBranch = b;
          // Only check behind if on default branch
          if (b === activeRepo.default_branch) {
            return checkMainBehind(repoId).then((n) => { behindCount = n; });
          } else {
            behindCount = 0;
          }
        })
        .catch(() => { headBranch = null; behindCount = 0; })
        .finally(() => { checking = false; });
    }
  });

  async function handleSync() {
    if (syncing) return;
    syncing = true;
    syncError = false;
    try {
      await syncMain(activeRepo.id);
      addToast(`${activeRepo.default_branch} synced with origin`, "success");
      behindCount = 0;
    } catch {
      syncError = true;
      addToast(`Failed to sync ${activeRepo.default_branch}`, "error");
      setTimeout(() => { syncError = false; }, 2000);
    } finally {
      syncing = false;
    }
  }

  async function handleCheckout() {
    if (checkingOut) return;
    checkingOut = true;
    try {
      await checkoutDefaultBranch(activeRepo.id);
      headBranch = activeRepo.default_branch;
    } catch {
      // leave state as-is so warning persists
    } finally {
      checkingOut = false;
    }
  }

  function selectRepo(repo: RepoDetail) {
    onSelectRepo(repo);
    dropdownRef?.close();
  }

  function startDrag(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button, input, label, [role="button"]')) return;
    if (e.buttons === 1) {
      e.preventDefault();
      getCurrentWindow().startDragging();
    }
  }

  function handleDoubleClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (target.closest('button, input, label, [role="button"]')) return;
    getCurrentWindow().toggleMaximize();
  }

  export function toggleRepoDropdown() {
    dropdownRef?.toggle();
  }

  export function isRepoDropdownOpen() {
    return dropdownRef?.isOpen() ?? false;
  }

  export function closeRepoDropdown() {
    dropdownRef?.close();
  }

  // ── Repo-level script runner (per-repo state) ────────
  type ScriptStatus = "idle" | "running" | "success" | "error";
  let scriptStatusMap = new SvelteMap<string, ScriptStatus>();
  let scriptOutputMap = new SvelteMap<string, string[]>();
  let scriptExitCodeMap = new SvelteMap<string, number | null>();
  let scriptPopoverMap = new SvelteMap<string, boolean>();
  let scriptNameMap = new SvelteMap<string, string>();
  let repoScriptDropdownOpen = $state(false);
  let outputEl = $state<HTMLPreElement | null>(null);

  let repoScriptStatus = $derived(scriptStatusMap.get(activeRepo.id) ?? "idle");
  let repoScriptOutput = $derived(scriptOutputMap.get(activeRepo.id) ?? []);
  let repoScriptExitCode = $derived(scriptExitCodeMap.get(activeRepo.id) ?? null);
  let repoScriptPopoverOpen = $derived(scriptPopoverMap.get(activeRepo.id) ?? false);
  let repoScriptRunningName = $derived(scriptNameMap.get(activeRepo.id) ?? "");

  // Close dropdown when switching repos
  $effect(() => {
    const _id = activeRepo.id;
    repoScriptDropdownOpen = false;
  });

  const ANSI_COLORS: Record<number, string> = {
    30: "var(--ansi-black)",   31: "var(--ansi-red)",     32: "var(--ansi-green)",   33: "var(--ansi-yellow)",
    34: "var(--ansi-blue)",    35: "var(--ansi-magenta)", 36: "var(--ansi-cyan)",    37: "var(--ansi-white)",
    90: "var(--ansi-bright-black)",  91: "var(--ansi-bright-red)",     92: "var(--ansi-bright-green)",  93: "var(--ansi-bright-yellow)",
    94: "var(--ansi-bright-blue)",   95: "var(--ansi-bright-magenta)", 96: "var(--ansi-bright-cyan)",   97: "var(--ansi-bright-white)",
  };

  function ansiToHtml(text: string): string {
    let html = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    let result = "";
    let open = false;
    const parts = html.split(/(\x1b\[[0-9;]*m)/);
    for (const part of parts) {
      const m = part.match(/^\x1b\[([0-9;]*)m$/);
      if (m) {
        const codes = m[1].split(";").map(Number);
        if (open) { result += "</span>"; open = false; }
        const styles: string[] = [];
        for (const c of codes) {
          if (c === 0) continue;
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

  function scrollOutputToBottom() {
    if (outputEl) outputEl.scrollTop = outputEl.scrollHeight;
  }

  let hasRepoScripts = $derived(
    !inWorkspace &&
    (repoSettings?.run_scripts?.length ?? 0) > 0 &&
    repoSettings!.run_scripts.some((s: NamedScript) => s.command.trim())
  );

  function runDefaultRepoScript() {
    const scripts = repoSettings?.run_scripts ?? [];
    const first = scripts.find((s: NamedScript) => s.command.trim());
    if (first) runNamedRepoScript(first);
  }

  function runNamedRepoScript(script: NamedScript) {
    if (!script.command.trim()) return;
    const rid = activeRepo.id;
    scriptStatusMap.set(rid, "running");
    repoScriptDropdownOpen = false;
    scriptOutputMap.set(rid, []);
    scriptExitCodeMap.delete(rid);
    scriptPopoverMap.set(rid, true);
    scriptNameMap.set(rid, script.name || script.command);

    runRepoScript(rid, script.command, (event: ScriptEvent) => {
      if (event.type === "output") {
        const prev = scriptOutputMap.get(rid) ?? [];
        scriptOutputMap.set(rid, prev.length >= 500 ? [...prev.slice(1), event.data] : [...prev, event.data]);
        requestAnimationFrame(scrollOutputToBottom);
      } else if (event.type === "exit") {
        const ok = event.code === 0;
        scriptExitCodeMap.set(rid, event.code);
        scriptStatusMap.set(rid, ok ? "success" : "error");
        setTimeout(() => {
          if (scriptStatusMap.get(rid) === (ok ? "success" : "error")) {
            scriptStatusMap.set(rid, "idle");
          }
        }, 2000);
      }
    }).catch(() => {
      scriptExitCodeMap.set(rid, -1);
      scriptStatusMap.set(rid, "error");
      setTimeout(() => {
        if (scriptStatusMap.get(rid) === "error") scriptStatusMap.set(rid, "idle");
      }, 2000);
    });
  }

  async function handleStopRepoScript() {
    try {
      await stopRepoScript(activeRepo.id);
    } catch {
      // Process may have already exited
    }
  }

  function handleWindowClick(e: MouseEvent) {
    const target = e.target as HTMLElement;
    if (repoScriptDropdownOpen && !target.closest(".repo-run-wrapper")) {
      repoScriptDropdownOpen = false;
    }
    if (repoScriptPopoverOpen && !target.closest(".repo-run-wrapper")) {
      scriptPopoverMap.set(activeRepo.id, false);
    }
  }
</script>

<svelte:window onclick={handleWindowClick} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
  class="titlebar"
  class:dev={import.meta.env.DEV}
  onmousedown={startDrag}
  ondblclick={handleDoubleClick}
>
  <div class="titlebar-left">
    <button class="home-btn" onclick={onGoHome} use:tooltip={{ text: "Home" }}><ChevronLeft size={14} strokeWidth={2.5} /></button>
    <div class="btn-group">
      <Dropdown bind:this={dropdownRef} onclose={onDropdownClose} tooltipOpts={{ text: "Switch repository", shortcut: "⌘E" }}>
        {#snippet trigger()}
          <span class="repo-name">{activeRepo.display_name}</span>
        {/snippet}

        {#each repos as repo, i}
          <button
            class="dropdown-item"
            class:active={repo.id === activeRepo.id}
            class:highlighted={i === highlightedRepoIndex}
            onclick={() => selectRepo(repo)}
          >
            <span class="dropdown-item-name">{repo.display_name}</span>
            {#if repo.id === activeRepo.id}
              <Check size={12} class="check-mark" />
            {/if}
            {#if i < 9}
              <span class="shortcut-pill">{i + 1}</span>
            {/if}
          </button>
        {/each}
        <div class="dropdown-divider"></div>
        <button class="dropdown-item add-item" class:highlighted={highlightedRepoIndex === repos.length} onclick={() => { onGoHome(); dropdownRef?.close(); }}>
          <Plus size={12} />
          <span>Add repository</span>
        </button>
      </Dropdown>
      <button class="settings-btn" onclick={onSettings} use:tooltip={{ text: "Settings", shortcut: "⌘," }}>
        <Settings size={14} />
      </button>
    </div>
    <div class="breadcrumb">
      {#if inWorkspace}
        <button class="breadcrumb-segment" onclick={onGoToPlan} use:tooltip={{ text: "Board", shortcut: "⌘1" }}>
          <LayoutGrid size={13} />
        </button>
        {#if workspaceTitle}
          <span class="breadcrumb-segment current task-title">{workspaceTitle}</span>
        {/if}
      {:else}
        <button
          class="breadcrumb-segment"
          class:current={planView === "kanban"}
          onclick={() => onPlanViewChange?.("kanban")}
          use:tooltip={{ text: "Kanban", shortcut: "⌘1" }}
        >
          <LayoutGrid size={13} />
        </button>
        <button
          class="breadcrumb-segment"
          class:current={planView === "files"}
          onclick={() => onPlanViewChange?.("files")}
          use:tooltip={{ text: "Files", shortcut: "⌘2" }}
        >
          <FolderOpen size={13} />
        </button>
        <button
          class="breadcrumb-segment"
          class:current={planView === "terminal"}
          onclick={() => onPlanViewChange?.("terminal")}
          use:tooltip={{ text: "Terminal", shortcut: "⌘3" }}
        >
          <SquareTerminal size={13} />
        </button>
      {/if}
    </div>
    {#if hasRepoScripts}
      <div class="repo-run-wrapper">
        <div class="repo-run-group">
          {#if repoScriptStatus === "running"}
            <button
              class="repo-run-btn stop"
              onclick={handleStopRepoScript}
              use:tooltip={{ text: "Stop script" }}
            >
              <Square size={10} />
              <span class="run-label">Stop</span>
            </button>
          {:else}
            <button
              class="repo-run-btn"
              class:success={repoScriptStatus === "success"}
              class:error={repoScriptStatus === "error"}
              onclick={runDefaultRepoScript}
              use:tooltip={{ text: `Run: ${repoSettings?.run_scripts?.[0]?.name || repoSettings?.run_scripts?.[0]?.command || "Script"}` }}
            >
              {#if repoScriptStatus === "success"}
                <Check size={12} />
              {:else if repoScriptStatus === "error"}
                <CircleX size={12} />
              {:else}
                <Play size={12} />
              {/if}
              <span class="run-label">Run{#if headBranch} <span class="run-branch">{headBranch}</span>{/if}</span>
            </button>
          {/if}
          <button
            class="repo-run-toggle"
            class:running={repoScriptStatus === "running"}
            class:success={repoScriptStatus === "success"}
            class:error={repoScriptStatus === "error"}
            onclick={() => { repoScriptDropdownOpen = !repoScriptDropdownOpen; }}
            use:tooltip={{ text: "Select script" }}
          >
            <ChevronDown size={10} />
          </button>
        </div>
        {#if repoScriptDropdownOpen}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="repo-script-dropdown" onclick={(e) => e.stopPropagation()}>
            {#each repoSettings?.run_scripts ?? [] as script, i}
              {#if script.command.trim()}
                <button
                  class="dropdown-item"
                  onclick={() => runNamedRepoScript(script)}
                  disabled={repoScriptStatus === "running"}
                >
                  <Play size={11} />
                  <span class="dropdown-item-name">{script.name || script.command}</span>
                  {#if i === 0}<span class="shortcut-pill">default</span>{/if}
                </button>
              {/if}
            {/each}
            {#if repoScriptOutput.length > 0 && !repoScriptPopoverOpen}
              <div class="dropdown-divider"></div>
              <button
                class="dropdown-item"
                onclick={() => { scriptPopoverMap.set(activeRepo.id, true); repoScriptDropdownOpen = false; }}
              >
                <span class="dropdown-item-name">Show last output</span>
              </button>
            {/if}
          </div>
        {/if}
        {#if repoScriptPopoverOpen}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="repo-script-popover" onclick={(e) => e.stopPropagation()}>
            <div class="repo-popover-header">
              <span class="repo-popover-title">
                {#if repoScriptStatus === "running"}
                  <span class="repo-spinner"></span>
                {/if}
                {repoScriptRunningName || "Script"}
              </span>
              <button class="repo-popover-close" onclick={() => { scriptPopoverMap.set(activeRepo.id, false); }}>
                <X size={12} />
              </button>
            </div>
            <pre class="repo-popover-output" bind:this={outputEl}>{#each repoScriptOutput as line}{@html ansiToHtml(line)}{/each}</pre>
            {#if repoScriptExitCode !== null}
              <div class="repo-popover-exit" class:ok={repoScriptExitCode === 0} class:fail={repoScriptExitCode !== 0}>
                {repoScriptExitCode === 0 ? "✓ Exited successfully" : `✗ Exit code ${repoScriptExitCode}`}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="titlebar-center"></div>

  <div class="titlebar-right">
    {#if autopilotEnabled && autopilotStatus}
      <span class="autopilot-status">{autopilotStatus}</span>
    {/if}
    {#if !inWorkspace}
      {#if !onDefaultBranch}
        <div class="branch-warning">
          <AlertTriangle size={13} />
          <span class="branch-warning-text">
            on <strong>{headBranch}</strong>, not <strong>{activeRepo.default_branch}</strong>
          </span>
          <button
            class="checkout-btn"
            onclick={handleCheckout}
            disabled={checkingOut}
          >
            {checkingOut ? "Switching…" : `Switch to ${activeRepo.default_branch}`}
          </button>
        </div>
      {:else if behindCount > 0}
        <button
          class="sync-btn"
          class:syncing
          class:error={syncError}
          onclick={handleSync}
          disabled={syncing}
          use:tooltip={{ text: `Sync local ${activeRepo.default_branch} with origin` }}
        >
          <RefreshCw size={13} />
          <span class="sync-label">
            {syncError ? "Failed" : syncing ? "Syncing…" : `Sync (${behindCount} behind)`}
          </span>
        </button>
      {/if}
    {/if}
    <button
      class="autopilot-btn"
      class:active={autopilotEnabled}
      onclick={onAutopilotToggle}
      use:tooltip={{ text: "Autopilot", shortcut: "⌘4" }}
    >
      <Zap size={12} />
      Autopilot
    </button>
    {#if autopilotEnabled}
      <button
        class="dep-graph-btn"
        onclick={onShowDepGraph}
        use:tooltip={{ text: "Task dependencies" }}
      >
        <Network size={12} />
      </button>
    {/if}
  </div>
</header>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0.5rem;
    height: 40px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-titlebar);
    -webkit-user-select: none;
    user-select: none;
    cursor: default;
    flex-shrink: 0;
    position: relative;
  }

  .titlebar.dev {
    background: var(--bg-dev);
    border-bottom-color: var(--border-dev);
  }

  .titlebar-left {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    /* leave room for traffic lights on macOS */
    padding-left: 78px;
  }

  .home-btn {
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: var(--bg-active);
    border: 1px solid var(--border-light);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    flex-shrink: 0;
    transition: border-color 0.15s ease;
  }

  .home-btn:hover {
    border-color: var(--accent);
  }

  .titlebar-center {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    pointer-events: none;
  }

  .breadcrumb {
    display: flex;
    align-items: stretch;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 5px;
    overflow: hidden;
  }

  .breadcrumb-segment {
    padding: 0.4rem 0.55rem;
    background: transparent;
    border: none;
    border-radius: 0;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    transition: background 0.15s, color 0.15s;
    white-space: nowrap;
  }

  .breadcrumb-segment:hover:not(.current):not(:disabled) {
    color: var(--text-secondary);
    background: var(--bg-hover);
  }

  .breadcrumb-segment.current {
    background: var(--bg-active);
    color: var(--accent);
    cursor: default;
  }

  .breadcrumb-segment:disabled:not(.current) {
    cursor: default;
  }

  .breadcrumb-segment.task-title {
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    padding-right: 0.65rem;
  }

  .titlebar-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .sync-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem 0.55rem;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 5px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.75rem;
    font-weight: 600;
    cursor: pointer;
    transition: color 0.15s, background 0.15s, border-color 0.15s;
  }

  .sync-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .sync-btn:disabled {
    cursor: default;
  }

  .sync-btn.syncing {
    color: var(--accent);
  }

  .sync-btn.syncing :global(svg) {
    animation: spin 0.8s linear infinite;
  }

  .sync-btn.error {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 25%, transparent);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .sync-label {
    white-space: nowrap;
  }

  .branch-warning {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.25rem 0.5rem;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 5px;
    color: var(--accent);
    font-size: 0.75rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .branch-warning :global(svg) {
    flex-shrink: 0;
  }

  .branch-warning-text {
    color: var(--accent);
  }

  .branch-warning-text strong {
    color: var(--text-bright);
  }

  .checkout-btn {
    padding: 0.2rem 0.5rem;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 4px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 700;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
    white-space: nowrap;
  }

  .checkout-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
    color: var(--text-bright);
  }

  .checkout-btn:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .repo-name {
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.4rem 0.5rem;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
    text-align: left;
  }

  .dropdown-item:hover,
  .dropdown-item.highlighted {
    background: var(--border);
    color: var(--text-primary);
  }

  .dropdown-item.active {
    color: var(--text-bright);
  }

  .dropdown-item-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-item :global(.check-mark) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .shortcut-pill {
    font-size: 0.6rem;
    font-weight: 600;
    min-width: 1.1rem;
    height: 1.1rem;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 3px;
    background: var(--border);
    color: var(--text-dim);
    flex-shrink: 0;
  }

  .dropdown-divider {
    height: 1px;
    background: var(--border);
    margin: 0.25rem 0;
  }

  .add-item {
    color: var(--text-dim);
  }

  .add-item:hover {
    color: var(--text-primary);
  }

  .autopilot-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.4rem 0.55rem;
    background: transparent;
    border: 1px solid var(--border-light);
    border-radius: 5px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .autopilot-btn:hover:not(.active) {
    color: var(--text-secondary);
    background: var(--bg-hover);
  }

  .autopilot-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    animation: autopilot-glow 3s ease-in-out infinite;
  }

  @keyframes autopilot-glow {
    0%, 100% { box-shadow: none; }
    50% { box-shadow: 0 0 8px color-mix(in srgb, var(--accent) 25%, transparent); }
  }

  .dep-graph-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 5px;
    color: var(--accent);
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
  }

  .dep-graph-btn:hover {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .autopilot-status {
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--accent);
    white-space: nowrap;
  }

  /* ── Button groups (segmented) ───────────────── */

  .btn-group {
    display: flex;
    align-items: stretch;
    border: 1px solid var(--border-light);
    border-radius: 5px;
  }

  .btn-group :global(.dropdown-trigger) {
    border: none;
    border-radius: 4px 0 0 4px;
    border-right: 1px solid var(--border-light);
  }

  .settings-btn {
    background: var(--bg-card);
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.9rem;
    padding: 0.4rem 0.45rem;
    border: none;
    border-radius: 0 4px 4px 0;
    display: flex;
    align-items: center;
  }

  .settings-btn:hover {
    color: var(--text-primary);
    background: var(--border);
  }

  /* ── Repo-level run script ────────────────────── */

  .repo-run-wrapper {
    position: relative;
  }

  .repo-run-group {
    display: flex;
    align-items: stretch;
    border-radius: 5px;
    overflow: hidden;
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }

  .run-label {
    font-size: 0.7rem;
    font-weight: 600;
    white-space: nowrap;
  }

  .run-branch {
    font-family: var(--font-mono);
    font-size: 0.6rem;
    font-weight: 500;
    padding: 0.05rem 0.3rem;
    border-radius: 3px;
    background: color-mix(in srgb, currentColor 12%, transparent);
    margin-left: 0.3em;
  }

  .repo-run-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.25rem;
    padding: 0 0.55rem;
    height: 26px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: none;
    color: var(--accent);
    cursor: pointer;
    transition: background 0.15s;
  }

  .repo-run-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .repo-run-btn.success {
    color: var(--status-ok);
  }

  .repo-run-btn.error {
    color: var(--diff-del);
  }

  .repo-run-btn.stop {
    color: var(--diff-del);
    background: color-mix(in srgb, var(--diff-del) 10%, transparent);
  }

  .repo-run-btn.stop:hover {
    background: color-mix(in srgb, var(--diff-del) 18%, transparent);
  }

  .repo-run-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 26px;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: none;
    border-left: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    color: var(--accent);
    cursor: pointer;
    transition: background 0.15s;
  }

  .repo-run-toggle:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .repo-run-toggle.running {
    color: var(--accent);
  }

  .repo-run-toggle.success {
    color: var(--status-ok);
    border-left-color: color-mix(in srgb, var(--status-ok) 25%, transparent);
  }

  .repo-run-toggle.error {
    color: var(--diff-del);
    border-left-color: color-mix(in srgb, var(--diff-del) 25%, transparent);
  }

  .repo-script-dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 4px;
    min-width: 180px;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    padding: 0.25rem;
    z-index: 100;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  }

  /* ── Script output popover ────────────────────── */

  .repo-script-popover {
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

  .repo-popover-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.35rem 0.5rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .repo-popover-title {
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

  .repo-popover-close {
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

  .repo-popover-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .repo-popover-output {
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

  .repo-popover-exit {
    padding: 0.3rem 0.5rem;
    font-size: 0.68rem;
    font-weight: 600;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .repo-popover-exit.ok {
    color: var(--status-ok);
    background: color-mix(in srgb, var(--status-ok) 5%, transparent);
  }

  .repo-popover-exit.fail {
    color: var(--diff-del);
    background: color-mix(in srgb, var(--diff-del) 5%, transparent);
  }

  .repo-spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--border);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    flex-shrink: 0;
  }

</style>

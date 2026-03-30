<script lang="ts">
  import { open, confirm } from "@tauri-apps/plugin-dialog";
  import { listen } from "@tauri-apps/api/event";
  import { SvelteMap } from "svelte/reactivity";
  import {
    addRepo,
    removeRepo,
    listRepos,
    createWorkspace,
    removeWorkspace,
    listWorkspaces,
    sendMessage,
    listModels,
    saveImage,
    onAgentStatus,
    onWorkspaceUpdated,
    onTodosChanged,
    stopAgent,
    renameBranch,
    getRepoSettings,
    getPrTemplate,
    getChangedFiles,
    readWorkspaceFile,
    gitCommit,
    gitPush,
    ghPrMerge,
    generateCommitMessage,
    updateFromBase,
    saveTodos,
    loadTodos,
    checkGhCli,
    ghAuthLogin,
    cancelGhAuthLogin,
    listGhRepos,
    cloneRepo,
    createGhRepo,
    setRepoProfile,
    checkRepoGhAccess,
    createStagingWorkspace,
    removeStagingWorkspace,
    getSystemResources,
    prioritizeTodos,
    determineDependencies,
    interpretAutopilotCommand,
    type RepoDetail,
    type RepoSettings,
    type WorkspaceInfo,
    type AgentEvent,
    type PrStatus,
    type GhCliStatus,
    type GhRepoEntry,
    type AutopilotAction,
    suggestReplies,
    regenerateHot,
    getContextMeta,
    checkInvariants,
    updateContextAfterMerge,
    lspStartServer,
    lspGetStatus,
    type ContextBuildStatus,
    type ProviderInfo,
    type AgentProvider,
    getWorkspaceProviderInfo,
    switchWorkspaceProvider,
  } from "$lib/ipc";
  import {
    addUserMessage,
    addAssistantMessage,
    addActionMessage,
    addTurnTokens,
    finalizeTurnTokens,
    loadPersistedMessages,
    clearWorkspaceData,
    setSending,
    sendingByWorkspace,
    getMessages,
  } from "$lib/stores/messages.svelte";
  import { onMount, tick } from "svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import WorkspacePanel, { type PanelTab } from "$lib/components/workspace/WorkspacePanel.svelte";
  import KanbanBoard from "$lib/components/kanban/KanbanBoard.svelte";
  import ReviewAlertBar from "$lib/components/ReviewAlertBar.svelte";
  import { type PastedImage, type ChatPanelApi } from "$lib/chat-utils";
  import type { Mention } from "$lib/components/chat/MentionInput.svelte";
  import RepoSettingsPanel from "$lib/components/RepoSettings.svelte";
  import SearchModal from "$lib/components/SearchModal.svelte";
  import DependencyGraph from "$lib/components/DependencyGraph.svelte";
  import FileBrowser from "$lib/components/workspace/FileBrowser.svelte";
  import TerminalView from "$lib/components/workspace/Terminal.svelte";
  import { closeRepoTerminal } from "$lib/ipc";
  import { Plus } from "lucide-svelte";
  import { type ReviewState } from "$lib/components/workspace/ReviewPill.svelte";
  import Toasts from "$lib/components/Toasts.svelte";
  import { addToast, removeToast } from "$lib/stores/toasts.svelte";
  import { DEFAULT_REVIEW_PROMPT } from "$lib/review-prompt";
  import {
    hydratePrStatusFromCache,
    removePrStatusCacheEntry,
    clearPrStatusCacheForRepo,
    refreshPrStatus as _refreshPrStatus,
    refreshChangeCounts as _refreshChangeCounts,
    refreshBaseUpdates as _refreshBaseUpdates,
  } from "$lib/pr-status";

  // ── State ──────────────────────────────────────────────

  let repos = $state<RepoDetail[]>([]);
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeRepo = $state<RepoDetail | null>(null);
  let selectedWsId = $state<string | null>(null);
  let activeTab = $state<PanelTab>("diff");
  let chatExpanded = $state(true);
  let terminalPaneVisible = $state(false);

  // App-wide LSP status tracking (keyed by server_id)
  let lspStatusMap = $state(new Map<string, { status: string; message: string; repo_id: string }>());
  let diffRefreshTrigger = $state(0);
  let showSettings = $state(false);
  let creatingWsId = $state<string | null>(null);
  let repoSettings = $state<RepoSettings | null>(null);
  let contextBuildStatus = $state<ContextBuildStatus>("not_built");
  let prStatusMap = new SvelteMap<string, PrStatus>();
  let changeCounts = new SvelteMap<string, { additions: number; deletions: number }>();
  let planModeByWorkspace = new SvelteMap<string, boolean>();
  let thinkingModeByWorkspace = new SvelteMap<string, boolean>();
  let modelByWorkspace = new SvelteMap<string, string>();
  let fileNavigatePath = $state<string | null>(null);
  let fileNavigateLine = $state<number | null>(null);
  let showSearchModal = $state(false);
  let chatPanelApis = new SvelteMap<string, ChatPanelApi>();
  let reviewByWorkspace = new SvelteMap<string, ReviewState>();
  let agentTaskByWorkspace = new SvelteMap<string, string>();
  let providerInfoByWorkspace = new SvelteMap<string, ProviderInfo>();
  let gitOpInProgress = new SvelteMap<string, boolean>();
  let baseBehindMap = new SvelteMap<string, number>();
  let updatingBranchMap = new SvelteMap<string, boolean>();
  let titleBarRef: TitleBar | undefined = $state();
  let wsPanelRef: WorkspacePanel | undefined = $state();
  let kanbanRef: KanbanBoard | undefined = $state();
  let repoDropdownIndex = $state(-1);

  // ── Autopilot state (per-repo) ──────────────────────────
  // TODO: remove after autopilot feature is merged
  const AUTOPILOT_BLACKLISTED_BRANCHES = new Set(["feat/autopilot-mode"]);

  let autopilotEnabled = $state(false);
  let autopilotEvaluating = $state(false);
  let autopilotPrioritizedIds = $state<string[]>([]);
  let autopilotPrioritizing = $state(false);
  let maxConcurrentAgents = $state(3);
  const MAX_AUTO_REVIEW_CYCLES = 5;
  const autoReviewCount = new SvelteMap<string, number>();
  /** Workspaces where autopilot already triggered PR creation — skip further reviews until PR appears in prStatusMap. */
  const autopilotPrPending = new Set<string>();
  const autopilotErrorWs = new Set<string>();

  interface AutopilotEvent {
    id: string;
    time: number;
    type: "spawn" | "auto_answer" | "review_start" | "review_done" | "pr_created" | "conflict_resolve" | "prioritized" | "staging_rebuild" | "user_command" | "orchestrator_response" | "error";
    message: string;
    wsId?: string;
    wsName?: string;
  }
  let autopilotEvents = $state<AutopilotEvent[]>([]);

  function addAutopilotEvent(type: AutopilotEvent["type"], message: string, wsId?: string, wsName?: string) {
    autopilotEvents.push({ id: crypto.randomUUID(), time: Date.now(), type, message, wsId, wsName });
    if (autopilotEvents.length > 200) autopilotEvents.splice(0, autopilotEvents.length - 200);
  }

  // ── Dependency graph overlay ─────────────────────────────
  let showDepGraph = $state(false);
  $effect(() => { if (!autopilotEnabled) showDepGraph = false; });

  // ── Staging state ──────────────────────────────────────
  let stagingWsId = $state<string | null>(null);
  let stagingError = $state<string | null>(null);
  let rebuildingStaging = $state(false);
  let stagingMergedBranches = $state<string[]>([]);
  let stagingConflictingBranches = $state<string[]>([]);

  // ── Per-repo autopilot persistence ─────────────────────
  // Saves/restores autopilot + staging state when switching repos so each repo
  // has its own independent autopilot lifecycle.
  interface SavedAutopilotState {
    enabled: boolean;
    evaluating: boolean;
    prioritizedIds: string[];
    prioritizing: boolean;
    events: AutopilotEvent[];
    stagingWsId: string | null;
    stagingError: string | null;
    rebuildingStaging: boolean;
    stagingMergedBranches: string[];
    stagingConflictingBranches: string[];
  }
  const autopilotByRepo = new Map<string, SavedAutopilotState>();

  function saveAutopilotForRepo(repoId: string) {
    autopilotByRepo.set(repoId, {
      enabled: autopilotEnabled,
      evaluating: autopilotEvaluating,
      prioritizedIds: [...autopilotPrioritizedIds],
      prioritizing: autopilotPrioritizing,
      events: [...autopilotEvents],
      stagingWsId,
      stagingError,
      rebuildingStaging,
      stagingMergedBranches: [...stagingMergedBranches],
      stagingConflictingBranches: [...stagingConflictingBranches],
    });
  }

  function restoreAutopilotForRepo(repoId: string) {
    const saved = autopilotByRepo.get(repoId);
    if (saved) {
      autopilotEnabled = saved.enabled;
      autopilotEvaluating = saved.evaluating;
      autopilotPrioritizedIds = saved.prioritizedIds;
      autopilotPrioritizing = saved.prioritizing;
      autopilotEvents = saved.events;
      stagingWsId = saved.stagingWsId;
      stagingError = saved.stagingError;
      rebuildingStaging = saved.rebuildingStaging;
      stagingMergedBranches = saved.stagingMergedBranches;
      stagingConflictingBranches = saved.stagingConflictingBranches;
    } else {
      autopilotEnabled = false;
      autopilotEvaluating = false;
      autopilotPrioritizedIds = [];
      autopilotPrioritizing = false;
      autopilotEvents = [];
      stagingWsId = null;
      stagingError = null;
      rebuildingStaging = false;
      stagingMergedBranches = [];
      stagingConflictingBranches = [];
    }
  }

  // ── Home screen state ──────────────────────────────────
  let ghStatus = $state<GhCliStatus | null>(null);
  let selectedProfile = $state<string | null>(null);
  let ghRepos = $state<GhRepoEntry[]>([]);
  let repoSearch = $state("");
  let loadingRepos = $state(false);
  let cloning = $state(false);
  let ghCheckLoading = $state(true);
  let ghAuthInProgress = $state(false);
  let ghAuthCode = $state<string | null>(null);
  let ghSearchTriggered = $state(false);
  let showCreateForm = $state(false);
  let createName = $state("");
  let createPrivate = $state(true);
  let createDescription = $state("");
  let createReadme = $state(true);
  let creating = $state(false);

  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Filtered local repos based on search
  let filteredRepos = $derived(
    repoSearch
      ? repos.filter((r) => {
          const q = repoSearch.toLowerCase();
          return r.display_name.toLowerCase().includes(q) || r.path.toLowerCase().includes(q);
        })
      : repos,
  );

  // GitHub results excluding repos already added locally (match by name)
  let filteredGhRepos = $derived(
    ghRepos.filter((gh) => {
      const ghName = gh.full_name.split("/").pop()?.toLowerCase() ?? "";
      return !repos.some((r) => r.display_name.toLowerCase() === ghName);
    }),
  );

  async function checkGhStatus() {
    ghCheckLoading = true;
    try {
      ghStatus = await checkGhCli();
      // Auto-select: prefer the active profile, fall back to first
      if (ghStatus.profiles.length > 0) {
        const active = ghStatus.profiles.find((p) => p.active);
        selectedProfile = active ? active.login : ghStatus.profiles[0].login;
      }
      // Auto-detect profiles for existing repos that don't have one
      if (ghStatus.authenticated) {
        bindProfilesForExistingRepos(ghStatus.profiles);
      }
    } catch (e) {
      addToast(String(e));
    } finally {
      ghCheckLoading = false;
    }
  }

  async function handleGhLogin() {
    ghAuthInProgress = true;
    ghAuthCode = null;
    let unlisten: (() => void) | undefined;
    try {
      unlisten = await listen<string>("gh-auth-code", (e) => {
        ghAuthCode = e.payload;
      });
      await ghAuthLogin();
      await checkGhStatus();
    } catch (e) {
      addToast(String(e));
    } finally {
      ghAuthInProgress = false;
      ghAuthCode = null;
      unlisten?.();
    }
  }

  async function bindProfilesForExistingRepos(profiles: { login: string }[]) {
    const logins = profiles.map((p) => p.login);
    const unbound = repos.filter((r) => !r.gh_profile);
    if (unbound.length === 0) return;

    const results = await Promise.allSettled(
      unbound.map(async (repo) => {
        const matchedLogin = await checkRepoGhAccess(repo.path, logins);
        if (matchedLogin) {
          await setRepoProfile(repo.id, matchedLogin);
          repo.gh_profile = matchedLogin;
          return true;
        }
        return false;
      }),
    );

    const anyChanged = results.some((r) => r.status === "fulfilled" && r.value);
    if (anyChanged) repos = [...repos];

    for (const r of results) {
      if (r.status === "rejected") {
        addToast(`Failed to check GitHub access: ${r.reason}`);
      }
    }
  }

  async function fetchGhRepos(search?: string) {
    if (!selectedProfile) return;
    loadingRepos = true;
    ghSearchTriggered = true;
    try {
      ghRepos = await listGhRepos(selectedProfile, search);
    } catch (e) {
      addToast(String(e));
    } finally {
      loadingRepos = false;
    }
  }

  function handleRepoSearch(value: string) {
    repoSearch = value;
    clearTimeout(searchDebounceTimer);
    if (selectedProfile && value) {
      searchDebounceTimer = setTimeout(() => {
        loadingRepos = true;
        fetchGhRepos(value);
      }, 400);
    } else if (!value) {
      ghRepos = [];
      ghSearchTriggered = false;
      loadingRepos = false;
    }
  }

  async function handleCloneRepo(repo: GhRepoEntry) {
    if (!selectedProfile) return;
    cloning = true;
    try {
      const repoName = repo.full_name.split("/").pop() ?? repo.full_name;
      const result = await cloneRepo(repo.clone_url, repoName, selectedProfile);
      if (!repos.find((r) => r.id === result.id)) {
        repos = [...repos, result];
      }
      await selectRepo(result);
    } catch (e) {
      addToast(String(e));
    } finally {
      cloning = false;
    }
  }

  async function handleCreateRepo() {
    if (!selectedProfile || !createName.trim()) return;
    const name = createName.trim();
    const description = createDescription.trim() || null;
    const isPrivate = createPrivate;
    const readme = createReadme;
    const profile = selectedProfile;

    // Close form immediately so the UI isn't blocked
    showCreateForm = false;
    createName = "";
    createDescription = "";
    creating = true;

    const toastId = addToast(`Creating "${name}"...`, "info", 0);

    try {
      const result = await createGhRepo(
        { name, private: isPrivate, description, add_readme: readme },
        profile,
      );
      removeToast(toastId);
      addToast(`Repository "${name}" created`, "success");
      if (!repos.find((r) => r.id === result.id)) {
        repos = [...repos, result];
      }
      await selectRepo(result);
    } catch (e) {
      removeToast(toastId);
      addToast(String(e));
    } finally {
      creating = false;
    }
  }

  async function handleOpenRepoWithProfile() {
    try {
      const selected = await open({
        directory: true,
        title: "Open a git repository",
      });
      if (!selected) return;

      // Check access against the selected profile only
      if (selectedProfile) {
        const matched = await checkRepoGhAccess(selected, [selectedProfile]);
        if (!matched) {
          addToast(`"${selectedProfile}" does not have access to this repository.`);
          return;
        }
        const repo = await addRepo(selected);
        await setRepoProfile(repo.id, selectedProfile);
        repo.gh_profile = selectedProfile;
        if (!repos.find((r) => r.id === repo.id)) {
          repos = [...repos, repo];
        }
        await selectRepo(repo);
        return;
      }

      // No profile selected — add without profile binding
      const repo = await addRepo(selected);
      if (!repos.find((r) => r.id === repo.id)) {
        repos = [...repos, repo];
      }
      await selectRepo(repo);
    } catch (e) {
      addToast(String(e));
    }
  }

  async function handleRemoveRepoFromHome(repoId: string) {
    try {
      await removeRepo(repoId);
      repos = repos.filter((r) => r.id !== repoId);
    } catch (e) {
      addToast(String(e));
    }
  }

  // ── App mode + Todos ────────────────────────────────────
  type AppMode = "work" | "plan";
  let appMode = $state<AppMode>("plan");

  // ── Plan sub-views (kanban / files / terminal) ─────────
  type PlanView = "kanban" | "files" | "terminal";
  let planView = $state<PlanView>("kanban");

  interface PlanTerminalTab {
    id: string;
    label: string;
  }
  let planTerminalTabs = $state<PlanTerminalTab[]>([]);
  let planActiveTerminalTab = $state<string | null>(null);
  let planTerminalCounter = $state(0);

  function addPlanTerminalTab() {
    const count = ++planTerminalCounter;
    const id = crypto.randomUUID();
    planTerminalTabs = [...planTerminalTabs, { id, label: `Terminal ${count}` }];
    planActiveTerminalTab = id;
  }

  function removePlanTerminalTab(tabId: string) {
    if (planTerminalTabs.length <= 1) return;
    const idx = planTerminalTabs.findIndex((t) => t.id === tabId);
    if (idx === -1) return;
    planTerminalTabs = planTerminalTabs.filter((t) => t.id !== tabId);
    if (activeRepo) closeRepoTerminal(activeRepo.id, tabId).catch(() => {});
    if (planActiveTerminalTab === tabId) {
      const newIdx = Math.min(idx, planTerminalTabs.length - 1);
      planActiveTerminalTab = planTerminalTabs[newIdx].id;
    }
  }

  function cleanupPlanTerminals() {
    for (const tab of planTerminalTabs) {
      if (activeRepo) closeRepoTerminal(activeRepo.id, tab.id).catch(() => {});
    }
    planTerminalTabs = [];
    planActiveTerminalTab = null;
    planTerminalCounter = 0;
  }

  // Lazy-init first terminal tab when switching to terminal view
  $effect(() => {
    if (planView === "terminal" && planTerminalTabs.length === 0) {
      addPlanTerminalTab();
    }
  });

  interface TodoItem {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    imagePaths?: string[];
    mentionPaths?: string[];
    planMode?: boolean;
    thinkingMode?: boolean;
    model?: string;
    provider?: AgentProvider;
    ready?: boolean;
    depends_on?: string[];
    created_at: number;
  }
  let todos = $state<TodoItem[]>([]);

  // ── Message queue ──────────────────────────────────────
  interface QueuedMessage {
    id: string;
    prompt: string;        // raw user text (for display in queue UI)
    fullPrompt: string;    // expanded prompt (with context blocks, image refs)
    images: PastedImage[];
    imageDataUrls?: string[];
    mentions: Mention[];
    msgMentions?: { type: "file" | "folder"; path: string; displayName: string }[];
    planMode: boolean;
    thinkingMode: boolean;
    model: string;
    actionLabel?: string;
    hidden?: boolean;      // hide user message from chat (e.g. auto-sent TODO prompts)
  }
  const queueByWorkspace = new SvelteMap<string, QueuedMessage[]>();
  /** Set to true by Channel `done` event; checked by `agent-status: waiting` to trigger drain. */
  const pendingDrain = new SvelteMap<string, boolean>();

  let selectedWs = $derived(workspaces.find((w) => w.id === selectedWsId));
  let reviewingWsIds = $derived(
    new Set([...reviewByWorkspace.entries()].filter(([, r]) => r.status === "running").map(([id]) => id)),
  );
  let activeWorkspaces = $derived(
    [...workspaces].filter((ws) => ws.id !== stagingWsId).sort((a, b) => a.created_at - b.created_at),
  );

  // ── Kanban derived state ────────────────────────────────
  let todoItems = $derived(todos.filter((t) => t.repo_id === activeRepo?.id));
  let inProgressWs = $derived(
    activeWorkspaces.filter((ws) => {
      const pr = prStatusMap.get(ws.id);
      return !pr || pr.state === "none";
    }),
  );
  let reviewWs = $derived(
    activeWorkspaces.filter((ws) => prStatusMap.get(ws.id)?.state === "open"),
  );
  let doneWs = $derived(
    activeWorkspaces.filter((ws) => prStatusMap.get(ws.id)?.state === "merged"),
  );
  // Review alert: show completed reviews (not running ones)
  let completedReviewWs = $derived(
    reviewWs.filter((ws) => reviewByWorkspace.get(ws.id)?.status === "complete"),
  );
  let reviewAlertWs = $derived(completedReviewWs.length > 0 ? completedReviewWs[0] : null);
  let reviewAlertMore = $derived(Math.max(0, completedReviewWs.length - 1));

  // ── Lifecycle ──────────────────────────────────────────

  onMount(() => {
    let unlistenStatus: (() => void) | undefined;
    let unlistenWsUpdate: (() => void) | undefined;
    let unlistenTodos: (() => void) | undefined;

    (async () => {
      listModels(undefined).catch(() => {}); // pre-populate model cache
      listRepos().then((r) => {
        repos = r;
        if (r.length > 0) selectRepo(r[0]);
      }).catch((e) => { addToast(String(e)); });

      // Check GitHub CLI status for onboarding
      checkGhStatus();

      unlistenStatus = await onAgentStatus((event) => {
        const isReviewing = reviewByWorkspace.has(event.workspace_id);
        const ws = workspaces.find((w) => w.id === event.workspace_id);
        // Suppress "running" for reviewing workspaces; let "waiting" through
        if (ws && !(isReviewing && event.status === "running")) {
          ws.status = event.status as WorkspaceInfo["status"];
        }
        if (event.status === "waiting" && !isReviewing) {
          setSending(event.workspace_id, false);
          if (pendingDrain.get(event.workspace_id)) {
            pendingDrain.delete(event.workspace_id);
            drainQueue(event.workspace_id);
          }
          // Agent finished — check if base branch moved ahead while it was working
          refreshBaseUpdates(event.workspace_id);
        }
      });

      unlistenWsUpdate = await onWorkspaceUpdated((updated) => {
        const idx = workspaces.findIndex((w) => w.id === updated.id);
        if (idx >= 0) {
          workspaces[idx] = updated;
        }
      });

      unlistenTodos = await onTodosChanged(({ repo_id }) => {
        if (activeRepo && repo_id === activeRepo.id) {
          loadTodos(repo_id).then((raw) => {
            todos = (raw as TodoItem[]) ?? [];
          }).catch(() => {});
        }
      });

      // LSP server lifecycle events → status bar + toast notifications
      const lspStartToasts = new Map<string, string>();
      const unlistenLsp = await listen<{ repo_id?: string; server_id: string; status: string; message: string }>("lsp-status", (e) => {
        const { repo_id, server_id, status, message } = e.payload;
        // Update app-wide status bar
        const next = new Map(lspStatusMap);
        next.set(server_id, { status, message, repo_id: repo_id ?? "" });
        lspStatusMap = next;

        // Toast for transitions
        if (status === "starting") {
          const tid = addToast(message, "info", 60_000);
          lspStartToasts.set(server_id, tid);
        } else if (status === "ready") {
          const prev = lspStartToasts.get(server_id);
          if (prev) { removeToast(prev); lspStartToasts.delete(server_id); }
          addToast(message, "success");
        } else if (status === "error") {
          const prev = lspStartToasts.get(server_id);
          if (prev) { removeToast(prev); lspStartToasts.delete(server_id); }
          addToast(message, "error");
        } else if (status === "stopped") {
          const prev = lspStartToasts.get(server_id);
          if (prev) { removeToast(prev); lspStartToasts.delete(server_id); }
          addToast(message, "info");
        }
      });

      // Populate status bar with already-running servers (events may have fired before mount)
      lspGetStatus().then((servers) => {
        if (servers.length > 0) {
          const next = new Map(lspStatusMap);
          for (const s of servers) {
            next.set(s.server_id, { status: s.status, message: `${s.server_id} ${s.status}`, repo_id: s.repo_id ?? "" });
          }
          lspStatusMap = next;
        }
      }).catch(() => {});
    })();

    function handleKeydown(e: KeyboardEvent) {
      const mod = e.metaKey || e.ctrlKey;

      // ── Repo dropdown open: arrows, enter, number keys, escape ──
      if (titleBarRef?.isRepoDropdownOpen()) {
        if (e.key === "Escape") {
          e.preventDefault();
          titleBarRef.closeRepoDropdown();
          repoDropdownIndex = -1;
          return;
        }
        if (e.key === "ArrowDown") {
          e.preventDefault();
          repoDropdownIndex = Math.min(repoDropdownIndex + 1, repos.length);
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          repoDropdownIndex = Math.max(repoDropdownIndex - 1, 0);
          return;
        }
        if (e.key === "Enter" && repoDropdownIndex >= 0) {
          e.preventDefault();
          if (repoDropdownIndex < repos.length) {
            selectRepo(repos[repoDropdownIndex]);
          } else {
            activeRepo = null;
          }
          titleBarRef.closeRepoDropdown();
          repoDropdownIndex = -1;
          return;
        }
        if (!mod && e.key >= "1" && e.key <= "9") {
          e.preventDefault();
          const idx = parseInt(e.key) - 1;
          if (idx < repos.length) {
            selectRepo(repos[idx]);
          }
          titleBarRef.closeRepoDropdown();
          repoDropdownIndex = -1;
          return;
        }
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          return;
        }
      }

      if (!mod) return;

      const target = e.target as HTMLElement;
      const tag = target?.tagName;
      const inInput = tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable;

      switch (e.key) {
        case ",":
          e.preventDefault();
          if (activeRepo) showSettings = !showSettings;
          break;
        case "e":
          e.preventDefault();
          repoDropdownIndex = -1;
          titleBarRef?.toggleRepoDropdown();
          break;
        case "n":
          e.preventDefault();
          if (appMode === "plan" && planView === "kanban") {
            kanbanRef?.openNewTask();
          } else {
            handleNewWorkspace();
          }
          break;
        case "w":
          e.preventDefault();
          if (selectedWsId) handleRemove(selectedWsId);
          break;
        case "f":
          if (e.shiftKey && selectedWsId && appMode === "work") {
            e.preventDefault();
            showSearchModal = true;
          }
          break;
        case "1":
          e.preventDefault();
          if (appMode === "plan") { planView = "kanban"; }
          else { appMode = "plan"; planView = "kanban"; }
          break;
        case "2":
          e.preventDefault();
          appMode = "plan";
          planView = "files";
          break;
        case "3":
          e.preventDefault();
          appMode = "plan";
          planView = "terminal";
          tick().then(() => {
            const textarea = document.querySelector(".plan-terminal-layer.visible .xterm-helper-textarea") as HTMLTextAreaElement;
            textarea?.focus();
          });
          break;
        case "4":
          e.preventDefault();
          autopilotEnabled = !autopilotEnabled;
          if (autopilotEnabled) onAutopilotActivated();
          break;
        case "r":
          if (e.shiftKey && !inInput) {
            e.preventDefault();
            if (appMode === "work") {
              wsPanelRef?.triggerRun();
            } else {
              titleBarRef?.triggerRun();
            }
          } else if (!inInput && selectedWsId && appMode === "work") {
            e.preventDefault();
            handleReview();
          }
          break;
        case "m":
          if (!inInput && selectedWsId && appMode === "work") {
            e.preventDefault();
            handlePrAction();
          }
          break;
        case "u":
          if (!inInput && selectedWsId && appMode === "work") {
            e.preventDefault();
            handleUpdateBranch();
          }
          break;
        case "`":
          if (appMode === "work") {
            e.preventDefault();
            terminalPaneVisible = !terminalPaneVisible;
          }
          break;
      }
    }

    window.addEventListener("keydown", handleKeydown);

    // Poll PR status every 5s for workspaces that have a PR open
    const prPollInterval = setInterval(async () => {
      let anyChanged = false;
      for (const [wsId, pr] of prStatusMap) {
        if (pr.state === "open") {
          const changed = await refreshPrStatus(wsId);
          if (changed) anyChanged = true;
        }
      }
      if (anyChanged && autopilotEnabled) evaluateAutopilot();
    }, 5_000);

    // Check base branch updates every 60s for the selected workspace.
    // Lighter touch than PR polling since it involves a `git fetch`.
    const basePollInterval = setInterval(() => {
      if (selectedWsId) refreshBaseUpdates(selectedWsId);
    }, 60_000);

    return () => {
      unlistenStatus?.();
      unlistenWsUpdate?.();
      unlistenTodos?.();
      clearInterval(prPollInterval);
      clearInterval(basePollInterval);
      window.removeEventListener("keydown", handleKeydown);
    };
  });

  // ── Handlers ───────────────────────────────────────────

  function selectRepo(repo: RepoDetail) {
    if (activeRepo?.id === repo.id) return;

    // Clean up plan-mode terminals for the repo we're leaving
    cleanupPlanTerminals();
    planView = "kanban";

    // Save autopilot state for the repo we're leaving
    if (activeRepo) {
      saveAutopilotForRepo(activeRepo.id);
    }

    activeRepo = repo;
    selectedWsId = null;
    appMode = "plan";
    showSettings = false;

    // Restore autopilot state for the repo we're entering
    restoreAutopilotForRepo(repo.id);

    listWorkspaces(repo.id).then((ws) => {
      workspaces = ws;
      // Hydrate PR statuses from cache immediately so cards render in correct columns
      hydratePrStatusFromCache(ws.map((w) => w.id), prStatusMap);
      ws.forEach((w) => loadPersistedMessages(w.id));
      ws.forEach((w) => {
        refreshChangeCounts(w.id);
        refreshPrStatus(w.id);
        // Load provider info per workspace (non-blocking)
        getWorkspaceProviderInfo(w.id).then((info) => {
          providerInfoByWorkspace.set(w.id, info);
        }).catch(() => {});
      });
    }).catch((e) => { addToast(String(e)); });

    getRepoSettings(repo.id).then((s) => { repoSettings = s; }).catch(() => {});

    // Regenerate hot context (non-blocking, non-fatal)
    regenerateHot(repo.id).catch(() => {});

    // Load knowledge base status
    getContextMeta(repo.id).then((m) => { contextBuildStatus = m.build_status; }).catch(() => {});

    loadTodos(repo.id).then((raw) => {
      todos = (raw as TodoItem[]) ?? [];
    }).catch(() => { todos = []; });
  }

  async function handleRemoveRepo() {
    if (!activeRepo) return;
    const repoId = activeRepo.id;


    try {
      await removeRepo(repoId);
      showSettings = false;
      const removedWsIds = workspaces.map((w) => w.id);
      repos = repos.filter((r) => r.id !== repoId);
      workspaces = [];
      selectedWsId = null;
      sendingByWorkspace.clear();
      prStatusMap.clear();
      clearPrStatusCacheForRepo(removedWsIds);
      changeCounts.clear();
      baseBehindMap.clear();
      updatingBranchMap.clear();
      planModeByWorkspace.clear();
      thinkingModeByWorkspace.clear();
      modelByWorkspace.clear();
      agentTaskByWorkspace.clear();
      autopilotByRepo.delete(repoId);
      todos = [];
      activeRepo = repos.length > 0 ? repos[0] : null;
      if (activeRepo) selectRepo(activeRepo);
    } catch (e) {
      addToast(String(e));
    }
  }

  function handleNewWorkspace() {
    if (!activeRepo || creatingWsId) return;


    const tempId = `creating-${crypto.randomUUID()}`;
    const repoId = activeRepo.id;
    const placeholder: WorkspaceInfo = {
      id: tempId,
      name: "Creating...",
      branch: "",
      worktree_path: "",
      repo_id: repoId,
      gh_profile: null,
      status: "waiting",
      created_at: Date.now() / 1000,
    };
    creatingWsId = tempId;
    workspaces.push(placeholder);
    selectWorkspace(tempId);
    chatExpanded = true;
    // Handler returns here. Browser paints the placeholder.

    createWorkspace(repoId).then((ws) => {
      const idx = workspaces.findIndex((w) => w.id === tempId);
      if (idx >= 0) workspaces[idx] = ws;
      selectedWsId = ws.id;
      creatingWsId = null;
    }).catch((e) => {
      const failIdx = workspaces.findIndex((w) => w.id === tempId);
      if (failIdx >= 0) workspaces.splice(failIdx, 1);
      if (selectedWsId === tempId) selectedWsId = null;
      creatingWsId = null;
      addToast(String(e));
    });
  }

  function handleManualCheckout(data: { branchName: string; description: string }) {
    if (!activeRepo || creatingWsId) return;

    const repoId = activeRepo.id;
    const tempId = `creating-${crypto.randomUUID()}`;
    const placeholder: WorkspaceInfo = {
      id: tempId,
      name: data.branchName,
      branch: data.branchName,
      worktree_path: "",
      repo_id: repoId,
      gh_profile: null,
      status: "waiting",
      created_at: Date.now() / 1000,
      task_description: data.description || null,
    };
    creatingWsId = tempId;
    workspaces.push(placeholder);
    selectWorkspace(tempId);
    appMode = "work";
    chatExpanded = true;

    createWorkspace(repoId, undefined, data.description || undefined, undefined, data.branchName)
      .then((ws) => {
        const idx = workspaces.findIndex((w) => w.id === tempId);
        if (idx >= 0) workspaces[idx] = ws;
        selectedWsId = ws.id;
        creatingWsId = null;
      })
      .catch((e) => {
        const failIdx = workspaces.findIndex((w) => w.id === tempId);
        if (failIdx >= 0) workspaces.splice(failIdx, 1);
        if (selectedWsId === tempId) selectedWsId = null;
        creatingWsId = null;
        addToast(String(e));
      });
  }

  // ── Todo handlers ──────────────────────────────────────

  function persistTodos() {
    if (!activeRepo) return;
    saveTodos(activeRepo.id, todos).catch((e) => addToast(String(e)));
  }

  async function saveTodoImages(newImages: PastedImage[]): Promise<string[]> {
    if (!activeRepo || newImages.length === 0) return [];
    const namespace = `todo-${activeRepo.id}`;
    return Promise.all(newImages.map((img) => saveImage(namespace, img.base64, img.extension)));
  }

  async function handleNewTodo(data: { title: string; description: string; newImages: PastedImage[]; existingPaths: string[]; mentions?: Mention[]; planMode?: boolean; thinkingMode?: boolean; model?: string; provider?: AgentProvider }): Promise<string | null> {
    if (!activeRepo) return null;
    if (!data.title.trim() && data.newImages.length === 0 && data.existingPaths.length === 0) return null;
    try {
      const savedPaths = await saveTodoImages(data.newImages);
      const allPaths = [...data.existingPaths, ...savedPaths];
      const mentionPaths = data.mentions?.map((m) => m.path) ?? [];
      const todoId = crypto.randomUUID();
      todos.push({
        id: todoId,
        repo_id: activeRepo.id,
        title: data.title.trim(),
        description: data.description.trim(),
        imagePaths: allPaths.length > 0 ? allPaths : undefined,
        mentionPaths: mentionPaths.length > 0 ? mentionPaths : undefined,
        planMode: data.planMode || undefined,
        thinkingMode: data.thinkingMode || undefined,
        model: data.model || undefined,
        provider: data.provider || undefined,
        created_at: Date.now() / 1000,
      });
      persistTodos();
      if (autopilotEnabled) { await updateDependencies(); reprioritizeTodos(); }
      return todoId;
    } catch (e) {
      addToast(`Failed to save images: ${e}`);
      return null;
    }
  }

  async function handleNewTodoAndStart(data: { title: string; description: string; newImages: PastedImage[]; existingPaths: string[]; mentions?: Mention[]; planMode?: boolean; thinkingMode?: boolean; model?: string; provider?: AgentProvider }) {
    const todoId = await handleNewTodo(data);
    if (todoId) await handleSpawnFromTodo(todoId);
  }

  async function handleEditTodo(todoId: string, data: { title: string; description: string; newImages: PastedImage[]; existingPaths: string[]; mentions?: Mention[]; planMode?: boolean; thinkingMode?: boolean; model?: string; provider?: AgentProvider }) {
    const todo = todos.find((t) => t.id === todoId);
    if (!todo) return;
    try {
      const savedPaths = await saveTodoImages(data.newImages);
      const allPaths = [...data.existingPaths, ...savedPaths];
      const mentionPaths = data.mentions?.map((m) => m.path) ?? [];
      todo.title = data.title.trim();
      todo.description = data.description.trim();
      todo.imagePaths = allPaths.length > 0 ? allPaths : undefined;
      todo.mentionPaths = mentionPaths.length > 0 ? mentionPaths : undefined;
      todo.planMode = data.planMode || undefined;
      todo.thinkingMode = data.thinkingMode || undefined;
      todo.model = data.model || undefined;
      persistTodos();
      if (autopilotEnabled) updateDependencies();
    } catch (e) {
      addToast(`Failed to save images: ${e}`);
    }
  }

  function handleRemoveTodo(todoId: string) {
    const idx = todos.findIndex((t) => t.id === todoId);
    if (idx >= 0) {
      todos.splice(idx, 1);
      persistTodos();
    }
  }

  function handleToggleReady(todoId: string) {
    const todo = todos.find((t) => t.id === todoId);
    if (todo) {
      todo.ready = !todo.ready;
      persistTodos();
      if (todo.ready && autopilotEnabled) {
        reprioritizeTodos().then(() => evaluateAutopilot());
      }
    }
  }

  // ── Autopilot engine ──────────────────────────────────────
  function resultKind(text: string): "clean" | "failed" | "issues" {
    const t = text.trim();
    const tl = t.toLowerCase();
    if (tl.startsWith("**review failed:**")) return "failed";
    if (t.startsWith("[CLEAN]") || tl.includes("no issues found")) return "clean";
    return "issues";
  }

  let activeAgentCount = $derived(
    workspaces.filter(ws =>
      ws.repo_id === activeRepo?.id &&
      (sendingByWorkspace.get(ws.id) || reviewByWorkspace.has(ws.id) || ws.id === creatingWsId)
    ).length
  );

  async function reprioritizeTodos() {
    if (autopilotPrioritizing || !activeRepo) return;
    const readyTodos = todos.filter(t => t.ready);
    if (readyTodos.length <= 1) {
      autopilotPrioritizedIds = readyTodos.map(t => t.id);
      return;
    }
    autopilotPrioritizing = true;
    try {
      const todoData = readyTodos.map(t => ({ id: t.id, title: t.title, description: t.description, depends_on: t.depends_on ?? [] }));
      const ordered = await prioritizeTodos(JSON.stringify(todoData));
      autopilotPrioritizedIds = ordered.filter(id => readyTodos.some(t => t.id === id));
      addAutopilotEvent("prioritized", `Prioritized ${autopilotPrioritizedIds.length} tasks`);
    } catch {
      // Fallback to list order
      autopilotPrioritizedIds = readyTodos.map(t => t.id);
    } finally {
      autopilotPrioritizing = false;
    }
  }

  // ── Dependency tracking ─────────────────────────────────
  function isDependencyResolved(depId: string): boolean {
    // Still in todo list → not even started
    if (todos.some(t => t.id === depId)) return false;
    // Find workspace spawned from this todo
    const ws = workspaces.find(w => w.source_todo_id === depId);
    if (!ws) return true; // todo deleted without spawning → treat as resolved
    // Resolved only when PR is merged
    const pr = prStatusMap.get(ws.id);
    return pr?.state === "merged";
  }

  function isTodoBlocked(todo: TodoItem): boolean {
    return todo.depends_on?.some(depId => !isDependencyResolved(depId)) ?? false;
  }

  async function updateDependencies() {
    if (!activeRepo) return;
    const allTodos = todos.filter(t => t.repo_id === activeRepo!.id);
    if (allTodos.length <= 1) return;
    try {
      const todoData = allTodos.map(t => ({ id: t.id, title: t.title, description: t.description }));
      const deps = await determineDependencies(JSON.stringify(todoData));
      for (const todo of allTodos) {
        const depList = deps[todo.id];
        todo.depends_on = depList && depList.length > 0 ? depList : undefined;
      }
      persistTodos();
    } catch {
      // Non-fatal: tasks just won't have dep info
    }
  }

  /** Get the last assistant text from a workspace's messages. */
  function getLastAssistantText(wsId: string): string {
    const msgs = getMessages(wsId);
    for (let i = msgs.length - 1; i >= 0; i--) {
      const msg = msgs[i];
      if (msg.role === "user") break;
      if (msg.role === "assistant") {
        for (let j = msg.chunks.length - 1; j >= 0; j--) {
          if (msg.chunks[j].type === "text") {
            const content = (msg.chunks[j] as { type: "text"; content: string }).content.trim();
            if (content) return content;
          }
        }
      }
    }
    return "";
  }

  async function evaluateAutopilot() {
    if (!autopilotEnabled || !activeRepo || autopilotEvaluating) return;
    autopilotEvaluating = true;
    try {
      // Cleanup: clear PR-pending flag for workspaces that now have a PR
      for (const wsId of autopilotPrPending) {
        const pr = prStatusMap.get(wsId);
        if (pr && pr.state !== "none") autopilotPrPending.delete(wsId);
      }

      // 0. Auto-answer: if an agent is waiting and asking a question, respond automatically
      for (const ws of workspaces) {
        if (ws.repo_id !== activeRepo.id) continue;
        if (AUTOPILOT_BLACKLISTED_BRANCHES.has(ws.branch)) continue;
        if (ws.status !== "waiting" || sendingByWorkspace.get(ws.id) || reviewByWorkspace.has(ws.id)) continue;
        if (ws.id === creatingWsId) continue;

        // Plan-mode workspace finished planning — execute the plan
        if (planModeByWorkspace.get(ws.id) === true) {
          addAutopilotEvent("auto_answer", `Executing plan for ${ws.name}`, ws.id, ws.name);
          planModeByWorkspace.set(ws.id, false);
          sendPrompt(ws.id, "Execute the plan above. Do not ask for confirmation — just do it.", "Executing plan");
          return;
        }

        const lastText = getLastAssistantText(ws.id);
        if (!lastText) continue;

        // Check if last message ends with a question
        const hasQuestion = lastText.split("\n").some(line => line.trimEnd().endsWith("?"));
        if (!hasQuestion) continue;

        // Use AI to decide what to respond
        try {
          const replies = await suggestReplies(lastText);
          if (replies.length > 0) {
            addAutopilotEvent("auto_answer", `Auto-answering "${replies[0]}" for ${ws.name}`, ws.id, ws.name);
            sendPrompt(ws.id, replies[0]);
            return;
          }
        } catch {
          // If AI fails, just say "Yes, proceed"
          addAutopilotEvent("auto_answer", `Auto-answering "Yes, proceed" for ${ws.name}`, ws.id, ws.name);
          sendPrompt(ws.id, "Yes, proceed.");
          return;
        }
      }

      // 1. Handle completed reviews first — finalize in-flight work before starting new
      for (const ws of workspaces) {
        if (ws.repo_id !== activeRepo.id) continue;
        if (AUTOPILOT_BLACKLISTED_BRANCHES.has(ws.branch)) continue;
        const review = reviewByWorkspace.get(ws.id);
        if (review?.status !== "complete") continue;

        const kind = resultKind(review.resultMarkdown);
        if (kind === "clean" || kind === "failed") {
          // No issues or review failed → create PR, done with this workspace
          const pr = prStatusMap.get(ws.id);
          if (!pr || pr.state === "none") {
            addAutopilotEvent("pr_created", `Creating PR for ${ws.name} (review: ${kind})`, ws.id, ws.name);
            reviewByWorkspace.delete(ws.id);
            autoReviewCount.delete(ws.id);
            autopilotPrPending.add(ws.id);
            triggerPrAction(ws.id, { skipMerge: true });
            return;
          }
          reviewByWorkspace.delete(ws.id);
          autoReviewCount.delete(ws.id);
        } else if (kind === "issues") {
          // Send issues back to agent to fix, then it'll come back for another review cycle
          addAutopilotEvent("review_done", `Review found issues on ${ws.name}, sending fixes`, ws.id, ws.name);
          sendPrompt(ws.id, `The code review found these issues. Fix them all:\n\n${review.resultMarkdown}`, "Fixing review issues");
          reviewByWorkspace.delete(ws.id);
          return;
        }
      }

      // 2. Auto-pickup: spawn from next ready TODO if under limit (skip blocked)
      const readyTodos = todos.filter(t => t.ready && !isTodoBlocked(t));
      if (activeAgentCount < maxConcurrentAgents && !creatingWsId && readyTodos.length > 0) {
        // Pick from prioritized list, or first ready todo
        const nextId = autopilotPrioritizedIds.find(id => readyTodos.some(t => t.id === id))
          ?? readyTodos[0].id;
        const todo = readyTodos.find(t => t.id === nextId);
        if (todo) {
          addAutopilotEvent("spawn", `Spawning agent for "${todo.title}"`, undefined, todo.title);
          handleSpawnFromTodo(todo.id);
          return;
        }
      }

      // 3. Auto-review: for in-progress workspaces that are idle with diffs
      //    Skip if PR creation already in flight, or reviewed MAX_AUTO_REVIEW_CYCLES times.
      for (const ws of inProgressWs) {
        if (AUTOPILOT_BLACKLISTED_BRANCHES.has(ws.branch)) continue;
        if (autopilotPrPending.has(ws.id)) continue;
        if (ws.status !== "waiting" || sendingByWorkspace.get(ws.id) || reviewByWorkspace.has(ws.id)) continue;
        if (ws.id === creatingWsId) continue;
        if (autopilotErrorWs.has(ws.id)) { autopilotErrorWs.delete(ws.id); continue; }
        const cc = changeCounts.get(ws.id);
        if (cc && (cc.additions > 0 || cc.deletions > 0)) {
          const cycles = autoReviewCount.get(ws.id) ?? 0;
          if (cycles >= MAX_AUTO_REVIEW_CYCLES) {
            // Hit review cap — skip review, go straight to PR
            addAutopilotEvent("pr_created", `Review cap reached for ${ws.name}, creating PR`, ws.id, ws.name);
            autoReviewCount.delete(ws.id);
            autopilotPrPending.add(ws.id);
            triggerPrAction(ws.id, { skipMerge: true });
            return;
          }
          autoReviewCount.set(ws.id, cycles + 1);
          addAutopilotEvent("review_start", `Auto-reviewing ${ws.name} (${cycles + 1}/${MAX_AUTO_REVIEW_CYCLES})`, ws.id, ws.name);
          triggerReview(ws.id);
          return;
        }
      }

      // 4. Auto-conflict-resolution and failing checks for open PRs
      for (const ws of reviewWs) {
        if (AUTOPILOT_BLACKLISTED_BRANCHES.has(ws.branch)) continue;
        if (sendingByWorkspace.get(ws.id)) continue;
        const pr = prStatusMap.get(ws.id);
        if (pr?.mergeable === "conflicting") {
          addAutopilotEvent("conflict_resolve", `Resolving conflicts on ${ws.name}`, ws.id, ws.name);
          triggerPrAction(ws.id, { skipMerge: true });
          return;
        }
        if (pr?.checks === "failing") {
          addAutopilotEvent("auto_answer", `Fixing failing checks on ${ws.name}`, ws.id, ws.name);
          triggerPrAction(ws.id, { skipMerge: true });
          return;
        }
      }
    } finally {
      autopilotEvaluating = false;
    }
  }

  async function rebuildStaging() {
    if (!activeRepo || rebuildingStaging) return;
    if (reviewWs.length === 0) {
      if (stagingWsId) {
        const removedId = stagingWsId;
        try {
          await removeStagingWorkspace(activeRepo.id);
          workspaces = workspaces.filter(w => w.id !== removedId);
        } catch { /* ignore */ }
        if (selectedWsId === removedId) selectedWsId = null;
        stagingWsId = null;
        stagingError = null;
        stagingMergedBranches = [];
        stagingConflictingBranches = [];
      }
      return;
    }

    rebuildingStaging = true;
    const branchNames = reviewWs.map(ws => ws.branch);
    try {
      const result = await createStagingWorkspace(activeRepo.id, branchNames);
      // Remove old staging from workspaces if different id
      const oldStagingId = stagingWsId;
      if (oldStagingId && oldStagingId !== result.workspace.id) {
        workspaces = workspaces.filter(w => w.id !== oldStagingId);
        if (selectedWsId === oldStagingId) selectedWsId = result.workspace.id;
      }
      stagingWsId = result.workspace.id;
      stagingMergedBranches = result.merged_branches;
      stagingConflictingBranches = result.conflicting_branches;
      // Add/update in workspaces array
      const idx = workspaces.findIndex(w => w.id === result.workspace.id);
      if (idx >= 0) {
        workspaces[idx] = result.workspace;
      } else {
        workspaces.push(result.workspace);
      }
      stagingError = result.conflicting_branches.length > 0
        ? `Could not merge: ${result.conflicting_branches.join(", ")}`
        : null;
      addAutopilotEvent("staging_rebuild", `Staging rebuilt: ${result.merged_branches.length} merged, ${result.conflicting_branches.length} conflicting`);
    } catch (e) {
      stagingError = String(e);
      addAutopilotEvent("error", `Staging rebuild failed: ${e}`);
    } finally {
      rebuildingStaging = false;
    }
  }

  // Called when autopilot is toggled on — fetch resources, prioritize, evaluate.
  // NOT a $effect — avoids reactive loops that re-trigger on state mutations.
  async function onAutopilotActivated() {
    getSystemResources().then(res => {
      // Claude agents are I/O-bound, not memory-hungry. Use total RAM (not available — macOS
      // reports free-minus-cache which is always tiny) and be generous with the ratio.
      maxConcurrentAgents = Math.max(2, Math.min(Math.floor(res.cpu_cores / 2), Math.floor(res.memory_gb / 4)));
    }).catch(() => { maxConcurrentAgents = 3; });
    // Dependencies + prioritization must complete before evaluation — otherwise
    // all tasks spawn unconditionally because depends_on is still empty.
    await updateDependencies();
    await reprioritizeTodos();
    rebuildStaging();
    evaluateAutopilot();
  }

  async function handleAutopilotCommand(command: string) {
    if (!activeRepo) return;
    addAutopilotEvent("user_command", command);
    try {
      const context = JSON.stringify({
        todos: todos.map(t => ({ id: t.id, title: t.title, ready: t.ready })),
        workspaces: workspaces.filter(w => w.repo_id === activeRepo!.id).map(w => ({
          id: w.id, name: w.name, branch: w.branch, status: w.status, task_title: w.task_title,
          isSending: sendingByWorkspace.get(w.id) ?? false,
          isReviewing: reviewByWorkspace.has(w.id),
          reviewStatus: reviewByWorkspace.get(w.id)?.status ?? null,
          reviewCycles: autoReviewCount.get(w.id) ?? 0,
          changes: changeCounts.get(w.id) ?? null,
          planMode: planModeByWorkspace.get(w.id) ?? false,
        })),
        prStatuses: [...prStatusMap.entries()].map(([id, pr]) => ({
          wsId: id, state: pr.state, mergeable: pr.mergeable, checks: pr.checks,
          additions: pr.additions, deletions: pr.deletions,
        })),
        autopilotEnabled,
        activeAgentCount,
        maxConcurrentAgents,
      });
      const action = await interpretAutopilotCommand(command, context);
      addAutopilotEvent("orchestrator_response", action.response);

      switch (action.action_type) {
        case "pause":
          autopilotEnabled = false;
          break;
        case "resume":
          autopilotEnabled = true;
          break;
        case "skip_todo":
          for (const id of action.todo_ids) {
            const todo = todos.find(t => t.id === id);
            if (todo) { todo.ready = false; persistTodos(); }
          }
          break;
        case "prioritize":
          if (action.reorder.length > 0) autopilotPrioritizedIds = action.reorder;
          break;
      }
    } catch (e) {
      addAutopilotEvent("error", `Command failed: ${e}`);
    }
    // Re-evaluate after command — tasks may have finished during the AI call
    if (autopilotEnabled) evaluateAutopilot();
  }

  async function handleSpawnFromTodo(todoId: string) {
    if (!activeRepo || creatingWsId) return;
    const todo = todos.find((t) => t.id === todoId);
    if (!todo) return;

    const repoId = activeRepo.id;
    const tempId = `creating-${crypto.randomUUID()}`;
    const placeholder: WorkspaceInfo = {
      id: tempId,
      name: "Creating...",
      branch: "",
      worktree_path: "",
      repo_id: repoId,
      gh_profile: null,
      status: "waiting",
      created_at: Date.now() / 1000,
      task_title: todo.title,
      task_description: todo.description || null,
      source_todo_id: todoId,
    };
    creatingWsId = tempId;
    workspaces.push(placeholder);
    selectWorkspace(tempId);

    // Optimistically remove the todo card immediately on start
    handleRemoveTodo(todoId);

    try {
      const ws = await createWorkspace(repoId, todo.title, todo.description || undefined, todoId);
      const idx = workspaces.findIndex((w) => w.id === tempId);
      if (idx >= 0) workspaces[idx] = ws;
      selectedWsId = ws.id;
      creatingWsId = null;
      if (todo.planMode) planModeByWorkspace.set(ws.id, true);
      if (todo.model) modelByWorkspace.set(ws.id, todo.model);

      // Set provider override if task specifies a non-default provider
      if (todo.provider) {
        switchWorkspaceProvider(ws.id, todo.provider).then(() => {
          getWorkspaceProviderInfo(ws.id).then((info) => {
            providerInfoByWorkspace.set(ws.id, info);
          }).catch(() => {});
        }).catch(() => {});
      } else {
        // Load default provider info for the new workspace
        getWorkspaceProviderInfo(ws.id).then((info) => {
          providerInfoByWorkspace.set(ws.id, info);
        }).catch(() => {});
      }

      // Build prompt from title + description
      const promptText = todo.description
        ? `${todo.title}\n\n${todo.description}`
        : todo.title;

      // Images are already saved to disk — reference paths directly
      const todoPaths = todo.imagePaths ?? [];
      const todoMentionPaths = todo.mentionPaths ?? [];
      let fullPrompt = promptText;

      // Add mentioned file references
      if (todoMentionPaths.length > 0) {
        const mentionRefs = todoMentionPaths.map((p) => `@${p}`).join("\n");
        fullPrompt = fullPrompt
          ? `${fullPrompt}\n\nReferenced files:\n${mentionRefs}`
          : `Referenced files:\n${mentionRefs}`;
      }

      if (todoPaths.length > 0) {
        const refs = todoPaths.join("\n");
        const imageInstructions =
          todoPaths.length === 1
            ? `I've attached an image. Read it using the Read tool:\n${refs}`
            : `I've attached ${todoPaths.length} images. Read each using the Read tool:\n${refs}`;
        fullPrompt = fullPrompt
          ? `${imageInstructions}\n\n${fullPrompt}`
          : imageInstructions;
      }

      // Send the todo task as the initial prompt (hidden — user already saw it on the card)
      routeMessage(ws.id, {
        id: crypto.randomUUID(),
        prompt: promptText,
        fullPrompt,
        images: [],
        mentions: [],
        planMode: todo.planMode ?? false,
        thinkingMode: todo.thinkingMode ?? repoSettings?.default_thinking ?? false,
        model: todo.model ?? "",
        hidden: true,
      });

      // Re-evaluate so autopilot can spawn the next task if slots remain
      if (autopilotEnabled) evaluateAutopilot();
    } catch (e) {
      const failIdx = workspaces.findIndex((w) => w.id === tempId);
      if (failIdx >= 0) workspaces.splice(failIdx, 1);
      if (selectedWsId === tempId) selectedWsId = null;
      creatingWsId = null;
      // Restore the optimistically removed todo card
      todos.push(todo);
      persistTodos();
      addToast(String(e));
    }
  }

  function handleKanbanCardClick(wsId: string) {
    selectedWsId = wsId;
    appMode = "work";
    chatExpanded = true;
    refreshPrStatus(wsId);
    refreshBaseUpdates(wsId);
  }

  // ── Workspace handlers ────────────────────────────────

  async function handleRemove(wsId: string) {
    const ws = workspaces.find((w) => w.id === wsId);
    const name = ws?.name ?? wsId;

    const confirmed = await confirm(
      `This will permanently remove "${name}" — its worktree, messages, and session data will be deleted.`,
      { title: "Remove workspace?", kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" },
    );
    if (!confirmed) return;



    // Optimistic: remove from UI immediately
    const idx = workspaces.findIndex((w) => w.id === wsId);
    const removed = idx >= 0 ? workspaces[idx] : null;
    if (idx >= 0) workspaces.splice(idx, 1);
    if (selectedWsId === wsId) selectedWsId = null;
    if (creatingWsId === wsId) creatingWsId = null;
    clearWorkspaceData(wsId);
    sendingByWorkspace.delete(wsId);
    queueByWorkspace.delete(wsId);
    pendingDrain.delete(wsId);
    prStatusMap.delete(wsId);
    removePrStatusCacheEntry(wsId);
    changeCounts.delete(wsId);
    baseBehindMap.delete(wsId);
    updatingBranchMap.delete(wsId);
    planModeByWorkspace.delete(wsId);
    thinkingModeByWorkspace.delete(wsId);
    modelByWorkspace.delete(wsId);

    removeWorkspace(wsId).catch((e) => {
      // Restore on failure
      if (removed) workspaces.push(removed);
      addToast(String(e));
    });
  }

  async function handleRemoveAllDone() {
    const ids = doneWs.map((w) => w.id);
    if (ids.length === 0) return;

    const confirmed = await confirm(
      `This will permanently remove ${ids.length} completed workspace${ids.length > 1 ? "s" : ""} and all their data.`,
      { title: "Remove all done?", kind: "warning", okLabel: "Remove all", cancelLabel: "Cancel" },
    );
    if (!confirmed) return;

    for (const wsId of ids) {
      const idx = workspaces.findIndex((w) => w.id === wsId);
      if (idx >= 0) workspaces.splice(idx, 1);
      if (selectedWsId === wsId) selectedWsId = null;
      if (creatingWsId === wsId) creatingWsId = null;
      clearWorkspaceData(wsId);
      sendingByWorkspace.delete(wsId);
      queueByWorkspace.delete(wsId);
      pendingDrain.delete(wsId);
      prStatusMap.delete(wsId);
      removePrStatusCacheEntry(wsId);
      changeCounts.delete(wsId);
      planModeByWorkspace.delete(wsId);
      thinkingModeByWorkspace.delete(wsId);

      removeWorkspace(wsId).catch((e) => {
        addToast(String(e));
      });
    }
  }

  // ── Send pipeline ───────────────────────────────────────

  /** Core send — assumes caller has verified it's safe to send. */
  async function sendDirect(wsId: string, msg: QueuedMessage) {

    setSending(wsId, true);

    if (msg.actionLabel) {
      addActionMessage(wsId, crypto.randomUUID(), msg.actionLabel);
    } else {
      addUserMessage(wsId, crypto.randomUUID(), msg.prompt || "(images attached)", msg.imageDataUrls, msg.msgMentions, msg.planMode || undefined, msg.hidden);
    }

    try {
      await sendMessage(wsId, msg.fullPrompt, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          const toolUses = event.tool_uses.map((t) => ({
            name: t.name,
            input: t.input_preview ?? "",
            filePath: t.file_path,
            oldString: t.old_string,
            newString: t.new_string,
          }));
          addAssistantMessage(
            wsId,
            crypto.randomUUID(),
            event.text.trim(),
            toolUses,
            event.thinking,
          );
          if (event.tool_uses.length > 0) {
            diffRefreshTrigger++;
            const last = event.tool_uses[event.tool_uses.length - 1];
            agentTaskByWorkspace.set(wsId, formatToolTask(last.name, last.input_preview));
          } else if (event.text.trim()) {
            agentTaskByWorkspace.set(wsId, "Thinking...");
          }
        } else if (event.type === "usage") {
          if (event.cumulative) {
            finalizeTurnTokens(wsId, event.input_tokens, event.output_tokens);
          } else {
            addTurnTokens(wsId, event.input_tokens, event.output_tokens);
          }
        } else if (event.type === "done") {
          setSending(wsId, false);
          agentTaskByWorkspace.delete(wsId);
          diffRefreshTrigger++;
          const doneRefresh = Promise.all([
            refreshChangeCounts(wsId),
            refreshPrStatus(wsId),
          ]);
          chatPanelApis.get(wsId)?.refreshSuggestions();
          if (activeRepo) {
            listWorkspaces(activeRepo.id)
              .then((fresh) => {
                const freshIds = new Set(fresh.map((w) => w.id));
                for (const fw of fresh) {
                  const idx = workspaces.findIndex((w) => w.id === fw.id);
                  if (idx >= 0) {
                    workspaces[idx] = fw;
                  } else {
                    workspaces.push(fw);
                  }
                }
                for (let i = workspaces.length - 1; i >= 0; i--) {
                  if (!freshIds.has(workspaces[i].id) && workspaces[i].id !== creatingWsId) {
                    workspaces.splice(i, 1);
                  }
                }
              })
              .catch(() => {});
          }
          pendingDrain.set(wsId, true);
          if (autopilotEnabled) {
            doneRefresh.then(() => evaluateAutopilot());
          }
        } else if (event.type === "error") {
          addToast(event.message);
          setSending(wsId, false);
          agentTaskByWorkspace.delete(wsId);
          pendingDrain.delete(wsId);
          if (autopilotEnabled) {
            autopilotErrorWs.add(wsId);
            const ws = workspaces.find(w => w.id === wsId);
            addAutopilotEvent("error", `Agent error on ${ws?.name ?? wsId}: ${event.message}`, wsId, ws?.name);
            evaluateAutopilot();
          }
        }
      }, msg.planMode, msg.thinkingMode, msg.model);
    } catch (e) {
      addToast(String(e));
      setSending(wsId, false);
      pendingDrain.delete(wsId);
    }
  }

  /** Shift next queued message and send it. */
  function drainQueue(wsId: string) {
    const queue = queueByWorkspace.get(wsId);
    if (!queue || queue.length === 0) return;
    const next = queue.shift()!;
    queueByWorkspace.set(wsId, [...queue]);
    sendDirect(wsId, next);
  }

  /** Remove a specific message from the queue. */
  function removeFromQueue(wsId: string, messageId: string) {
    const queue = queueByWorkspace.get(wsId);
    if (!queue) return;
    queueByWorkspace.set(wsId, queue.filter(q => q.id !== messageId));
  }

  /** Stop the current agent and immediately send a queued message. */
  async function handleSendQueuedNow(wsId: string, messageId: string) {
    const queue = queueByWorkspace.get(wsId);
    if (!queue) return;
    const idx = queue.findIndex(q => q.id === messageId);
    if (idx < 0) return;
    const msg = queue[idx];
    // Remove from queue
    queue.splice(idx, 1);
    queueByWorkspace.set(wsId, [...queue]);
    // Stop current agent
    try {
      await stopAgent(wsId);
      setSending(wsId, false);
    } catch (e) {
      addToast(String(e));
      return;
    }
    // Send the queued message directly
    sendDirect(wsId, msg);
  }

  /** Route a QueuedMessage: send directly, or enqueue if busy. */
  function routeMessage(wsId: string, msg: QueuedMessage) {
    if (sendingByWorkspace.get(wsId)) {
      // Agent busy → enqueue
      const queue = queueByWorkspace.get(wsId) ?? [];
      queue.push(msg);
      queueByWorkspace.set(wsId, [...queue]);
      return;
    }
    if ((queueByWorkspace.get(wsId)?.length ?? 0) > 0) {
      // Idle but queue exists (e.g. after stop) → enqueue + drain
      const queue = queueByWorkspace.get(wsId)!;
      queue.push(msg);
      queueByWorkspace.set(wsId, [...queue]);
      drainQueue(wsId);
      return;
    }
    // Idle, no queue → send directly
    sendDirect(wsId, msg);
  }

  async function sendPrompt(wsId: string, prompt: string, actionLabel?: string) {
    const thinkingMode = thinkingModeByWorkspace.get(wsId) ?? repoSettings?.default_thinking ?? false;
    const model = modelByWorkspace.get(wsId) ?? "";
    routeMessage(wsId, {
      id: crypto.randomUUID(),
      prompt,
      fullPrompt: prompt,
      images: [],
      mentions: [],
      planMode: false,
      thinkingMode,
      model,
      actionLabel,
    });
  }

  async function handleSend(prompt: string, images: PastedImage[] = [], mentions: Mention[] = [], planMode: boolean = false) {
    if (!selectedWsId || reviewByWorkspace.has(selectedWsId)) return;
    const wsId = selectedWsId;
    const thinkingMode = thinkingModeByWorkspace.get(wsId) ?? repoSettings?.default_thinking ?? false;
    const model = modelByWorkspace.get(wsId) ?? "";

    // Save images to workspace dir, collect file paths
    let imagePaths: string[] = [];
    if (images.length > 0) {
      try {
        imagePaths = await Promise.all(
          images.map((img) => saveImage(wsId, img.base64, img.extension)),
        );
      } catch (e) {
        addToast(`Failed to save images: ${e}`);
        return;
      }
    }

    // Resolve mentioned files — read contents and prepend as context blocks
    let contextBlocks: string[] = [];
    for (const mention of mentions) {
      if (mention.type === "file") {
        try {
          const content = await readWorkspaceFile(wsId, mention.path);
          const lines = content.split("\n").length;
          const focusAttr = mention.lineNumber ? ` focus_line="${mention.lineNumber}"` : "";
          contextBlocks.push(`<file path="${mention.path}" lines="${lines}"${focusAttr} source="mention">\n${content}\n</file>`);
        } catch {
          contextBlocks.push(`<file path="${mention.path}" source="mention">(could not read file — use Read tool to access)</file>`);
        }
      } else if (mention.type === "folder") {
        contextBlocks.push(`<folder path="${mention.path}" />`);
      }
    }

    // Build prompt with context + image references
    let fullPrompt = prompt;
    if (contextBlocks.length > 0) {
      const contextSection = contextBlocks.join("\n\n");
      fullPrompt = `${contextSection}\n\n${fullPrompt}`;
    }
    if (imagePaths.length > 0) {
      const refs = imagePaths.map((p) => p).join("\n");
      const imageInstructions =
        imagePaths.length === 1
          ? `I've attached an image. Read it using the Read tool:\n${refs}`
          : `I've attached ${imagePaths.length} images. Read each using the Read tool:\n${refs}`;
      fullPrompt = fullPrompt
        ? `${imageInstructions}\n\n${fullPrompt}`
        : imageInstructions;
    }

    const dataUrls = images.length > 0 ? images.map((img) => img.dataUrl) : undefined;
    const msgMentions = mentions.length > 0 ? mentions.map((m) => ({ type: m.type, path: m.path, displayName: m.displayName })) : undefined;

    routeMessage(wsId, {
      id: crypto.randomUUID(),
      prompt,
      fullPrompt,
      images,
      imageDataUrls: dataUrls,
      mentions,
      msgMentions,
      planMode,
      thinkingMode,
      model,
    });
  }

  /** Send immediately, bypassing the queue. Used for AskUserQuestion answers. */
  async function handleSendImmediate(prompt: string) {
    if (!selectedWsId) return;
    const wsId = selectedWsId;
    const thinkingMode = thinkingModeByWorkspace.get(wsId) ?? repoSettings?.default_thinking ?? false;
    const model = modelByWorkspace.get(wsId) ?? "";
    sendDirect(wsId, {
      id: crypto.randomUUID(),
      prompt,
      fullPrompt: prompt,
      images: [],
      mentions: [],
      planMode: false,
      thinkingMode,
      model,
    });
  }

  async function handleRename(wsId: string, newName: string) {
    try {
      const updated = await renameBranch(wsId, newName);
      const idx = workspaces.findIndex((w) => w.id === wsId);
      if (idx >= 0) {
        workspaces[idx] = updated;
      }
    } catch (e) {
      addToast(String(e));
    }
  }

  async function triggerPrAction(wsId: string, opts?: { skipMerge?: boolean }) {
    const ws = workspaces.find(w => w.id === wsId);
    if (!ws || !activeRepo) return;
    if (gitOpInProgress.get(wsId)) return;

    const pr = prStatusMap.get(wsId);

    if (pr && pr.state === "open") {
      const cc = changeCounts.get(wsId);
      const hasLocalChanges = cc && (cc.additions !== pr.additions || cc.deletions !== pr.deletions);

      // ── Agent-delegated: conflicts & failing checks need reasoning ──
      if (pr.mergeable === "conflicting") {
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `PR #${pr.number} has merge conflicts with ${baseBranch}.\n\nResolve them:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n5. Push\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving conflicts on PR #${pr.number}`);
        return;
      }
      if (pr.checks === "failing") {
        sendPrompt(wsId, `PR #${pr.number} has failing checks. Investigate the failures using \`gh pr checks ${pr.number}\`, fix the issues, commit, and push.`, `Fixing checks on PR #${pr.number}`);
        return;
      }

      // ── Direct CLI: commit & push ──
      if (hasLocalChanges) {
        gitOpInProgress.set(wsId, true);
        addActionMessage(wsId, crypto.randomUUID(), `Committing & pushing to PR #${pr.number}`);
        try {
          const msg = await generateCommitMessage(wsId);
          try {
            await gitCommit(wsId, msg);
          } catch (commitErr) {
            // Agent may have already committed — nothing to commit is not fatal
            if (!String(commitErr).includes("Nothing to commit")) throw commitErr;
          }
          await gitPush(wsId);
          addToast("Pushed successfully", "success");
          refreshChangeCounts(wsId);
          refreshPrStatus(wsId);
        } catch (e) {
          addToast(String(e));
          sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
        } finally {
          gitOpInProgress.delete(wsId);
        }
        return;
      }

      // ── Direct CLI: push only ──
      if (pr.ahead_by > 0) {
        gitOpInProgress.set(wsId, true);
        addActionMessage(wsId, crypto.randomUUID(), `Pushing to PR #${pr.number}`);
        try {
          await gitPush(wsId);
          addToast("Pushed successfully", "success");
          refreshPrStatus(wsId);
        } catch (e) {
          addToast(String(e));
          sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
        } finally {
          gitOpInProgress.delete(wsId);
        }
        return;
      }

      // ── Direct CLI: merge (skipped in autopilot) ──
      if (opts?.skipMerge) return;
      gitOpInProgress.set(wsId, true);
      addActionMessage(wsId, crypto.randomUUID(), `Merging PR #${pr.number}`);
      try {
        await ghPrMerge(wsId, pr.number);
        addToast(`PR #${pr.number} merged`, "success");
        refreshPrStatus(wsId);

        // Post-merge: refresh hot context + trigger knowledge base update
        if (activeRepo) {
          regenerateHot(activeRepo.id).catch(() => {});
          if (contextBuildStatus === "built") {
            updateContextAfterMerge(activeRepo.id, wsId, (event) => {
              if (event.type === "done") {
                addToast("Knowledge base updated after merge", "info");
                getContextMeta(activeRepo!.id).then((m) => { contextBuildStatus = m.build_status; }).catch(() => {});
              }
            }).catch(() => {});
          }
        }
      } catch (e) {
        addToast(String(e));
        sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
      } finally {
        gitOpInProgress.delete(wsId);
      }
      return;
    }

    // ── No PR: commit & push directly, then agent creates PR ──
    const baseBranch = activeRepo.default_branch;
    const files = await getChangedFiles(wsId).catch(() => []);
    const template = await getPrTemplate(activeRepo.id).catch(() => "");

    gitOpInProgress.set(wsId, true);
    try {
      if (files.length > 0) {
        addActionMessage(wsId, crypto.randomUUID(), "Committing & pushing changes");
        const msg = await generateCommitMessage(wsId);
        try {
          await gitCommit(wsId, msg);
        } catch (commitErr) {
          if (!String(commitErr).includes("Nothing to commit")) throw commitErr;
        }
        await gitPush(wsId);
      } else {
        addActionMessage(wsId, crypto.randomUUID(), "Pushing to origin");
        await gitPush(wsId);
      }
    } catch (e) {
      addToast(String(e));
      sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
      gitOpInProgress.delete(wsId);
      return;
    }
    gitOpInProgress.delete(wsId);

    let prompt: string;
    const customMsg = repoSettings?.pr_message?.trim();

    if (customMsg) {
      prompt = customMsg
        .replace(/\{\{branch\}\}/g, ws.branch)
        .replace(/\{\{base_branch\}\}/g, baseBranch)
        .replace(/\{\{file_count\}\}/g, String(files.length))
        .replace(/\{\{pr_template\}\}/g, template
          ? `\n## PR Description Template\n\nThis workspace has a PR template. Use it:\n\n\`\`\`markdown\n${template}\n\`\`\`\n`
          : "");
    } else {
      prompt = `The code is already committed and pushed to origin.\n\n`;
      prompt += `Create a pull request using \`gh pr create --base ${baseBranch}\`.\n`;
      prompt += `Keep the title under 80 characters. Keep the description under five sentences unless there's a template.\n`;
      prompt += `Base your PR title and description on what we've been working on in this conversation.\n`;

      if (template) {
        prompt += `\n## PR Description Template\n\nThis repo has a PR template. Use it:\n\n\`\`\`markdown\n${template}\n\`\`\`\n`;
      }
    }

    sendPrompt(wsId, prompt, "Creating pull request");
  }

  async function handlePrAction() {
    if (!selectedWs) return;
    chatExpanded = true;
    triggerPrAction(selectedWs.id);
  }

  async function handleStop() {
    if (!selectedWsId) return;
    try {
      await stopAgent(selectedWsId);
      setSending(selectedWsId, false);
    } catch (e) {
      addToast(String(e));
    }
  }

  async function handleProviderSwitch(wsId: string, provider: AgentProvider) {
    try {
      await switchWorkspaceProvider(wsId, provider);
      // Insert a divider message in chat
      addActionMessage(wsId, crypto.randomUUID(), `Switched to ${provider === "codex" ? "Codex" : "Claude"}`);
      // Refresh provider info for this workspace
      const info = await getWorkspaceProviderInfo(wsId);
      providerInfoByWorkspace.set(wsId, info);
      // Reset model selection (models differ between providers)
      modelByWorkspace.delete(wsId);
    } catch (e) {
      addToast(String(e));
    }
  }

  function formatToolTask(toolName: string, inputPreview?: string): string {
    const verbs: Record<string, string> = {
      Read: "Reading",
      Grep: "Searching",
      Glob: "Finding files",
      Bash: "Running command",
      Edit: "Editing",
      Write: "Writing",
      Agent: "Delegating",
    };
    const verb = verbs[toolName] ?? `Using ${toolName}`;
    return inputPreview ? `${verb} ${inputPreview}` : `${verb}...`;
  }

  async function triggerReview(wsId: string) {
    if (!activeRepo) return;
    const ws = workspaces.find(w => w.id === wsId);
    if (!ws) return;

    if (sendingByWorkspace.get(wsId) || reviewByWorkspace.has(wsId)) return;

    // Show immediate UI feedback before any async work
    addActionMessage(wsId, crypto.randomUUID(), "Reviewing code");

    const hasContext = contextBuildStatus === "built";
    reviewByWorkspace.set(wsId, {
      status: "running",
      currentTask: hasContext ? "Checking invariants..." : "Starting review...",
      resultMarkdown: "",
    });

    // Invariant pre-check: run before review if knowledge base is built
    if (hasContext) {
      try {
        const result = await checkInvariants(wsId);
        if (!result.passed && result.violations.length > 0) {
          const violationText = result.violations
            .map(v => `- ${v.invariant_id}: ${v.description} (${v.file}:${v.line})`)
            .join("\n");
          // In autopilot: send to agent to fix
          if (autopilotEnabled) {
            reviewByWorkspace.delete(wsId);
            addActionMessage(wsId, crypto.randomUUID(), "Fixing invariant violations");
            sendPrompt(wsId, `These invariant violations were found in your changes:\n\n${violationText}\n\nFix all of them before proceeding.`, "Fixing invariant violations");
            return;
          }
          // Manual: show violations as a warning toast, proceed anyway
          addToast(`Invariant violations found:\n${violationText}`, "error");
        }
      } catch {
        // Fail-open: if check fails, proceed with review
      }
      // Update task now that invariants are done
      const review = reviewByWorkspace.get(wsId);
      if (review) {
        review.currentTask = "Starting review...";
        reviewByWorkspace.set(wsId, { ...review });
      }
    }

    const pr = prStatusMap.get(wsId);

    const baseBranch = activeRepo.default_branch;

    const customMsg = repoSettings?.review_message?.trim();
    let reviewPrompt = DEFAULT_REVIEW_PROMPT;
    if (customMsg) {
      reviewPrompt += `\n\n## Additional Instructions\n\n${customMsg}`;
    }
    reviewPrompt = reviewPrompt
      .replace(/\{\{branch\}\}/g, ws.branch)
      .replace(/\{\{base_branch\}\}/g, baseBranch)
      .replace(/\{\{pr_number\}\}/g, pr?.state === "open" ? String(pr.number) : "N/A")
      .replace(/\{\{pr_title\}\}/g, pr?.state === "open" ? (pr.title ?? "") : "N/A");

    try {
      await sendMessage(wsId, reviewPrompt, (event: AgentEvent) => {
        const review = reviewByWorkspace.get(wsId);
        if (!review) return;

        if (event.type === "assistant_message") {
          if (event.tool_uses.length > 0) {
            const last = event.tool_uses[event.tool_uses.length - 1];
            review.currentTask = formatToolTask(last.name, last.input_preview);
          }
          if (event.text.trim()) {
            review.resultMarkdown = event.text.trim();
          }
          reviewByWorkspace.set(wsId, { ...review });
        } else if (event.type === "usage") {
          if (event.cumulative) {
            finalizeTurnTokens(wsId, event.input_tokens, event.output_tokens);
          } else {
            addTurnTokens(wsId, event.input_tokens, event.output_tokens);
          }
        } else if (event.type === "done") {
          review.status = "complete";
          reviewByWorkspace.set(wsId, { ...review });
          evaluateAutopilot();
        } else if (event.type === "error") {
          review.status = "complete";
          review.resultMarkdown = `**Review failed:** ${event.message}`;
          reviewByWorkspace.set(wsId, { ...review });
          evaluateAutopilot();
        }
      });
    } catch (e) {
      const review = reviewByWorkspace.get(wsId);
      if (review) {
        review.status = "complete";
        review.resultMarkdown = `**Review failed:** ${e}`;
        reviewByWorkspace.set(wsId, { ...review });
      }
    }
  }

  async function handleReview() {
    if (!selectedWsId) return;
    triggerReview(selectedWsId);
  }

  // Thin wrappers binding the reactive maps to the extracted helpers
  const refreshChangeCounts = (wsId: string) => _refreshChangeCounts(wsId, changeCounts);
  const refreshPrStatus = async (wsId: string): Promise<boolean> => {
    const changed = await _refreshPrStatus(wsId, prStatusMap);
    if (changed) {
      rebuildStaging();
      const pr = prStatusMap.get(wsId);
      if (pr && pr.state !== "none") autopilotPrPending.delete(wsId);
    }
    return changed;
  };
  const refreshBaseUpdates = (wsId: string) => _refreshBaseUpdates(wsId, baseBehindMap);

  async function handleUpdateBranch() {
    if (!selectedWs || !activeRepo) return;
    const wsId = selectedWs.id;
    if (updatingBranchMap.get(wsId)) return;

    updatingBranchMap.set(wsId, true);
    try {
      await updateFromBase(wsId);
      addToast("Branch updated from " + activeRepo.default_branch, "success");
      baseBehindMap.set(wsId, 0);
      refreshChangeCounts(wsId);
      diffRefreshTrigger++;
    } catch (e) {
      const errMsg = String(e);
      if (errMsg.includes("conflicts")) {
        addToast("Merge conflicts — delegating to agent", "info");
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `Updating from ${baseBranch} caused merge conflicts. The automatic merge was aborted.\n\nPlease resolve this:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving merge conflicts with ${baseBranch}`);
        chatExpanded = true;
      } else {
        addToast(errMsg);
      }
    } finally {
      updatingBranchMap.delete(wsId);
      // Re-check in case the merge resolved or changed the count
      refreshBaseUpdates(wsId);
    }
  }

  function selectWorkspace(wsId: string) {
    selectedWsId = wsId;
    // Refresh PR status in background so it's current when the user lands on the workspace
    refreshPrStatus(wsId);
    refreshBaseUpdates(wsId);
    // Start LSP servers in background (non-blocking)
    lspStartServer(wsId).catch(() => {});
  }
</script>

{#if !activeRepo}
  <div class="home-screen">
    <div class="home-content">
      <!-- GitHub profiles section -->
      <div class="home-section">
        <div class="home-section-header">GitHub</div>
        {#if ghCheckLoading}
          <div class="gh-loading">
            <div class="spinner"></div>
            <span>Checking GitHub CLI...</span>
          </div>
        {:else if !ghStatus?.installed}
          <div class="gh-notice compact">
            <span>GitHub CLI not installed.</span>
            <code class="cli-cmd">brew install gh</code>
            <button class="retry-btn" onclick={checkGhStatus}>Retry</button>
          </div>
        {:else if !ghStatus?.authenticated}
          {#if ghAuthInProgress}
            <div class="gh-auth-status">
              <div class="spinner"></div>
              {#if ghAuthCode}
                <span>Enter code <code class="device-code">{ghAuthCode}</code> in your browser</span>
              {:else}
                <span>Opening browser...</span>
              {/if}
              <button class="retry-btn" onclick={async () => { await cancelGhAuthLogin(); }}>Cancel</button>
            </div>
          {:else}
            <button class="connect-gh-btn" onclick={handleGhLogin}>
              Connect GitHub Account
            </button>
          {/if}
        {:else}
          <div class="profile-pills">
            {#each ghStatus.profiles as profile}
              <button
                class="profile-pill"
                class:selected={selectedProfile === profile.login}
                onclick={() => { selectedProfile = profile.login; }}
              >
                {profile.login}
                {#if profile.active}
                  <span class="profile-active-dot"></span>
                {/if}
              </button>
            {/each}
            {#if !ghAuthInProgress}
              <button
                class="profile-pill add-profile"
                onclick={handleGhLogin}
              >+</button>
            {/if}
          </div>
          {#if ghAuthInProgress}
            <div class="gh-auth-status">
              <div class="spinner"></div>
              {#if ghAuthCode}
                <span>Enter code <code class="device-code">{ghAuthCode}</code> in your browser</span>
              {:else}
                <span>Opening browser...</span>
              {/if}
              <button class="retry-btn" onclick={async () => { await cancelGhAuthLogin(); }}>Cancel</button>
            </div>
          {/if}
        {/if}
      </div>

      <!-- Repositories section -->
      <div class="home-section repos-section">
        <div class="home-section-header">Repositories</div>

        <div class="repo-search-bar">
          <input
            type="text"
            placeholder={selectedProfile ? `Search local or ${selectedProfile}'s GitHub repos...` : "Filter repositories..."}
            value={repoSearch}
            oninput={(e) => handleRepoSearch(e.currentTarget.value)}
          />
        </div>

        <div class="repo-list">
          <!-- Local repos -->
          {#each filteredRepos as repo (repo.id)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="repo-row" onclick={() => selectRepo(repo)} onkeydown={(e) => { if (e.key === "Enter") selectRepo(repo); }} tabindex="0" role="button">
              <span class="repo-row-name">{repo.display_name}</span>
              <span class="repo-row-right">
                {#if repo.gh_profile}
                  <span class="repo-row-profile">{repo.gh_profile}</span>
                {/if}
                <button
                  class="repo-row-remove"
                  title="Remove repository"
                  onclick={(e) => { e.stopPropagation(); handleRemoveRepoFromHome(repo.id); }}
                >&times;</button>
              </span>
              <span class="repo-row-path">{repo.path}</span>
            </div>
          {/each}

          <!-- GitHub results (when searching) -->
          {#if repoSearch && selectedProfile}
            {#if loadingRepos}
              <div class="gh-loading">
                <div class="spinner"></div>
                <span>Searching GitHub...</span>
              </div>
            {:else if ghSearchTriggered && filteredGhRepos.length > 0}
              <div class="repo-divider">
                <span>from GitHub</span>
              </div>
              {#each filteredGhRepos as repo}
                <button
                  class="repo-row gh"
                  disabled={cloning}
                  onclick={() => handleCloneRepo(repo)}
                >
                  <span class="repo-row-name">{repo.full_name.split("/").pop()}</span>
                  <span class="repo-row-profile">{repo.full_name}</span>
                  {#if repo.description}
                    <span class="repo-row-desc">{repo.description}</span>
                  {/if}
                </button>
              {/each}
            {:else if ghSearchTriggered && filteredGhRepos.length === 0 && filteredRepos.length === 0}
              <div class="empty-results">No repositories found matching "{repoSearch}"</div>
            {/if}
          {/if}

          {#if !repoSearch && repos.length === 0}
            <div class="empty-results">No repositories yet. Search GitHub above or open a local folder.</div>
          {/if}
        </div>

        {#if cloning}
          <div class="gh-loading">
            <div class="spinner"></div>
            <span>Cloning repository...</span>
          </div>
        {:else if selectedProfile && repoSearch.trim() && /^[a-zA-Z0-9._-]+$/.test(repoSearch.trim())}
          {#if !showCreateForm}
            <button
              class="open-repo-btn secondary"
              onclick={() => { createName = repoSearch.trim(); showCreateForm = true; }}
            >
              Create "{repoSearch.trim()}" on GitHub
            </button>
          {:else}
            <div class="create-repo-form">
              <input
                type="text"
                class="create-input"
                placeholder="Repository name"
                bind:value={createName}
              />
              <input
                type="text"
                class="create-input"
                placeholder="Description (optional)"
                bind:value={createDescription}
              />
              <div class="create-options">
                <div class="visibility-toggle">
                  <button
                    class="vis-btn"
                    class:selected={createPrivate}
                    onclick={() => (createPrivate = true)}
                  >Private</button>
                  <button
                    class="vis-btn"
                    class:selected={!createPrivate}
                    onclick={() => (createPrivate = false)}
                  >Public</button>
                </div>
                <label class="readme-check">
                  <input type="checkbox" bind:checked={createReadme} />
                  Add README
                </label>
              </div>
              <div class="create-actions">
                <button
                  class="open-repo-btn"
                  disabled={!createName.trim() || creating}
                  onclick={handleCreateRepo}
                >Create & Open</button>
                <button
                  class="open-repo-btn secondary"
                  onclick={() => (showCreateForm = false)}
                >Cancel</button>
              </div>
            </div>
          {/if}
        {/if}

        {#if selectedProfile}
          <button class="open-repo-btn secondary" onclick={handleOpenRepoWithProfile}>
            Add local repo as {selectedProfile}
          </button>
        {/if}
      </div>
    </div>
  </div>
{:else}
  <div class="app">
    <TitleBar
      bind:this={titleBarRef}
      {repos}
      {activeRepo}
      highlightedRepoIndex={repoDropdownIndex}
      onDropdownClose={() => (repoDropdownIndex = -1)}
      inWorkspace={appMode === "work"}
      workspaceTitle={selectedWs?.task_title ?? selectedWs?.name ?? null}
      onGoToPlan={() => { appMode = "plan"; }}
      onSelectRepo={selectRepo}
      onSettings={() => (showSettings = true)}
      onGoHome={() => { cleanupPlanTerminals(); planView = "kanban"; if (activeRepo) saveAutopilotForRepo(activeRepo.id); activeRepo = null; }}
      {autopilotEnabled}
      onAutopilotToggle={() => { autopilotEnabled = !autopilotEnabled; if (autopilotEnabled) onAutopilotActivated(); }}
      autopilotStatus={autopilotEnabled ? `${activeAgentCount}/${maxConcurrentAgents} agents · ${todos.filter(t => t.ready).length} queued` : undefined}
      onShowDepGraph={() => { showDepGraph = !showDepGraph; }}
      {planView}
      onPlanViewChange={(v) => { planView = v; }}
      {repoSettings}
    />

    {#if reviewAlertWs}
      <ReviewAlertBar
        workspace={reviewAlertWs}
        moreCount={reviewAlertMore}
        onReviewNow={() => {
          selectedWsId = reviewAlertWs!.id;
          appMode = "work";
          activeTab = "diff";
        }}
      />
    {/if}

    <div class="mode-stack">
      <div class="mode-layer" class:mode-visible={appMode === "work"} inert={appMode !== "work"}>
        <div class="main-layout">
          <Sidebar
            {workspaces}
            {selectedWsId}
            {creatingWsId}
            {prStatusMap}
            {reviewingWsIds}
            onSelect={selectWorkspace}
            onRename={handleRename}
            onRemove={handleRemove}
            {stagingWsId}
            {stagingError}
            {rebuildingStaging}
            stagingMergedCount={stagingMergedBranches.length}
          />

          <WorkspacePanel
            bind:this={wsPanelRef}
            bind:activeTab
            bind:fileNavigatePath
            bind:fileNavigateLine
            bind:terminalPaneVisible
            {chatExpanded}
            onChatExpandedChange={(v) => { chatExpanded = v; }}
            defaultBranch={activeRepo?.default_branch ?? "main"}
            {selectedWs}
            {selectedWsId}
            {activeWorkspaces}
            {creatingWsId}
            {changeCounts}
            {planModeByWorkspace}
            {thinkingModeByWorkspace}
            {modelByWorkspace}
            {reviewByWorkspace}
            {agentTaskByWorkspace}
            {repoSettings}
            {diffRefreshTrigger}
            prStatus={selectedWsId ? prStatusMap.get(selectedWsId) : undefined}
            wsChanges={selectedWsId ? changeCounts.get(selectedWsId) : undefined}
            baseBehindBy={selectedWsId ? baseBehindMap.get(selectedWsId) ?? 0 : 0}
            updatingBranch={selectedWsId ? updatingBranchMap.get(selectedWsId) ?? false : false}
            isStaging={selectedWsId === stagingWsId}
            stagingMergedCount={stagingMergedBranches.length}
            stagingConflictingCount={stagingConflictingBranches.length}
            contextWarning={contextBuildStatus !== "built"}
            {providerInfoByWorkspace}
            onProviderSwitch={handleProviderSwitch}
            onPrAction={handlePrAction}
            onUpdateBranch={handleUpdateBranch}
            onReview={handleReview}
            reviewRunning={selectedWsId ? reviewByWorkspace.get(selectedWsId)?.status === "running" : false}
            operationInProgress={selectedWsId ? gitOpInProgress.get(selectedWsId) ?? false : false}
            getQueueItems={(wsId) => (queueByWorkspace.get(wsId) ?? []).map(q => ({
              id: q.id,
              prompt: q.prompt,
              imageCount: q.images.length,
              mentionCount: q.mentions.length,
              planMode: q.planMode,
            }))}
            onSend={(prompt, images, mentions, planMode) => handleSend(prompt, images, mentions, planMode)}
            onSendImmediate={(prompt) => handleSendImmediate(prompt)}
            onStop={handleStop}
            onSendNow={(wsId, id) => handleSendQueuedNow(wsId, id)}
            onRemoveFromQueue={(wsId, id) => removeFromQueue(wsId, id)}
            onPlanModeChange={(wsId, enabled) => planModeByWorkspace.set(wsId, enabled)}
            onThinkingModeChange={(wsId, enabled) => thinkingModeByWorkspace.set(wsId, enabled)}
            onModelChange={(wsId, model) => modelByWorkspace.set(wsId, model)}
            onExecutePlan={(wsId) => {
              planModeByWorkspace.set(wsId, false);
              sendPrompt(wsId, "Execute the plan above. Do not ask for confirmation — just do it.", "Executing plan");
            }}
            onChatReady={(wsId, api) => chatPanelApis.set(wsId, api)}
            onDiffQuote={(text) => {
              if (selectedWsId) {
                chatPanelApis.get(selectedWsId)?.insertText(text);
                chatExpanded = true;
              }
            }}
            onReviewCancel={(wsId) => {
              const wasRunning = reviewByWorkspace.get(wsId)?.status === "running";
              reviewByWorkspace.delete(wsId);
              if (wasRunning) stopAgent(wsId).catch((e) => { addToast(String(e)); });
            }}
            onReviewSendToChat={(wsId, markdown) => {
              reviewByWorkspace.delete(wsId);
              sendPrompt(wsId, `Address all issues from this code review:\n\n${markdown}`, "Addressing review").catch((e) => { addToast(String(e)); });
            }}
          />
        </div>
      </div>

      <div class="mode-layer" class:mode-visible={appMode === "plan"} inert={appMode !== "plan"}>
        <!-- Kanban sub-view -->
        <div class="plan-sub-layer" class:plan-visible={planView === "kanban"}>
          <KanbanBoard
            bind:this={kanbanRef}
            todos={todoItems}
            inProgress={inProgressWs}
            review={reviewWs}
            done={doneWs}
            {prStatusMap}
            {changeCounts}
            {reviewingWsIds}
            {creatingWsId}
            repoId={activeRepo.id}
            repoName={activeRepo.display_name}
            defaultThinkingMode={repoSettings?.default_thinking ?? false}
            active={appMode === "plan" && planView === "kanban"}
            onCardClick={handleKanbanCardClick}
            onSpawnAgent={handleSpawnFromTodo}
            onNewTodo={handleNewTodo}
            onAddAndStart={handleNewTodoAndStart}
            onEditTodo={handleEditTodo}
            onRemoveTodo={handleRemoveTodo}
            onToggleReady={handleToggleReady}
            onRemoveWorkspace={handleRemove}
            onRemoveAllDone={handleRemoveAllDone}
            onManualCheckout={handleManualCheckout}
            {autopilotEnabled}
            {autopilotEvents}
            autopilotActiveAgents={activeAgentCount}
            autopilotMaxAgents={maxConcurrentAgents}
            autopilotTodoQueue={todos.filter(t => t.ready).length}
            autopilotPrioritizing={autopilotPrioritizing}
            autopilotRebuildingStaging={rebuildingStaging}
            onAutopilotCommand={handleAutopilotCommand}
          />
        </div>

        <!-- Files sub-view -->
        <div class="plan-sub-layer" class:plan-visible={planView === "files"}>
          <FileBrowser scope={{ type: "repo", repoId: activeRepo.id }} />
        </div>

        <!-- Terminal sub-view -->
        <div class="plan-sub-layer plan-terminal-layout" class:plan-visible={planView === "terminal"}>
          <div class="plan-terminal-tabs">
            {#each planTerminalTabs as tab (tab.id)}
              <button
                class="plan-term-tab"
                class:active={tab.id === planActiveTerminalTab}
                onclick={() => { planActiveTerminalTab = tab.id; }}
              >
                {tab.label}
                {#if planTerminalTabs.length > 1}
                  <span
                    class="tab-close"
                    role="button"
                    tabindex="-1"
                    onclick={(e) => { e.stopPropagation(); removePlanTerminalTab(tab.id); }}
                  >&times;</span>
                {/if}
              </button>
            {/each}
            <button class="plan-term-add" onclick={addPlanTerminalTab} title="New terminal">
              <Plus size={12} />
            </button>
          </div>
          <div class="plan-terminal-body">
            {#each planTerminalTabs as tab (tab.id)}
              {@const isVisible = tab.id === planActiveTerminalTab && planView === "terminal" && appMode === "plan"}
              <div class="plan-terminal-layer" class:visible={isVisible} inert={!isVisible}>
                <TerminalView
                  scope={{ type: "repo", repoId: activeRepo.id }}
                  terminalId={tab.id}
                  visible={isVisible}
                />
              </div>
            {/each}
          </div>
        </div>
      </div>
    </div>

    {#if showDepGraph}
      <DependencyGraph
        todos={todoItems}
        workspaces={activeWorkspaces}
        {prStatusMap}
        onClose={() => { showDepGraph = false; }}
      />
    {/if}

    {#if showSearchModal && selectedWsId}
      <SearchModal
        workspaceId={selectedWsId}
        onClose={() => { showSearchModal = false; }}
        onAddToContext={(path, displayName, lineNumber) => {
          showSearchModal = false;
          chatPanelApis.get(selectedWsId!)?.addMention({ type: "file", path, displayName, lineNumber });
          chatExpanded = true;
        }}
        onOpenInFiles={(path, lineNumber) => {
          showSearchModal = false;
          fileNavigatePath = path;
          fileNavigateLine = lineNumber;
          activeTab = "files";
        }}
      />
    {/if}

    {#if showSettings}
      <RepoSettingsPanel
        repoId={activeRepo.id}
        repoName={activeRepo.display_name}
        repoPath={activeRepo.path}
        {lspStatusMap}
        workspaceId={selectedWsId}
        onClose={() => {
          showSettings = false;
          if (activeRepo) {
            getRepoSettings(activeRepo.id).then((s) => { repoSettings = s; }).catch(() => {});
            listRepos().then((r) => {
              repos = r;
              const updated = r.find((x) => x.id === activeRepo!.id);
              if (updated) activeRepo = updated;
            }).catch(() => {});
          }
        }}
      />
    {/if}

    <!-- App-wide status bar -->
    <div class="app-statusbar">
      <div class="statusbar-left">
        {#each [...lspStatusMap] as [serverId, info]}
          {@const busy = info.status === "starting" || info.status === "indexing"}
          <span class="lsp-pill" class:busy class:error={info.status === "error"} class:ready={info.status === "ready"} class:stopped={info.status === "stopped"}>
            {#if busy}<span class="lsp-pill-spinner"></span>{/if}
            {serverId}
          </span>
        {/each}
      </div>
      <div class="statusbar-right"></div>
    </div>
  </div>
{/if}

<Toasts />

<style>
  /* ── Home screen ────────────────────────────────── */

  .home-screen {
    height: 100vh;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    align-items: center;
    overflow: hidden;
    padding: 1.5rem 2rem;
    padding-top: 3rem;
  }

  .home-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
    width: 100%;
    max-width: 440px;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  .logo-mark {
    width: 48px;
    height: 48px;
    border-radius: 12px;
    background: var(--bg-active);
    border: 1px solid var(--border-light);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    font-weight: 700;
    color: var(--accent);
  }

  .home-content h1 {
    margin: 0;
    font-size: 1.5rem;
    color: var(--text-bright);
    font-weight: 600;
  }

  /* ── Sections ──────────────────────────────────── */

  .home-section {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    flex-shrink: 0;
  }

  .home-section.repos-section {
    flex: 1;
    min-height: 0;
    flex-shrink: 1;
  }

  .home-section-header {
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  /* ── GitHub profiles ───────────────────────────── */

  .profile-pills {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .profile-pill {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.35rem 0.65rem;
    background: var(--bg-card);
    border: 1.5px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    font-weight: 500;
    transition: border-color 0.15s ease;
  }

  .profile-pill:hover {
    border-color: var(--border-light);
  }

  .profile-pill.selected {
    border-color: var(--accent);
    background: var(--bg-active);
    color: var(--text-bright);
  }

  .profile-active-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
    flex-shrink: 0;
  }

  .gh-loading {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--text-secondary);
    font-size: 0.82rem;
    padding: 0.5rem 0;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--border-light);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .gh-notice {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.25rem 0;
  }

  .gh-notice.compact {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
  }

  .gh-notice span {
    color: var(--text-secondary);
    font-size: 0.82rem;
  }

  .cli-cmd {
    padding: 0.3rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.75rem;
    color: var(--text-primary);
    user-select: all;
  }

  .retry-btn {
    padding: 0.25rem 0.6rem;
    background: var(--bg-elevated);
    border: 1px solid var(--border-light);
    border-radius: 4px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }

  .retry-btn:hover {
    background: var(--bg-hover);
  }

  .connect-gh-btn {
    padding: 0.5rem 1.25rem;
    background: var(--accent);
    color: var(--bg-base);
    border: none;
    border-radius: 6px;
    font-weight: 600;
    font-size: 0.85rem;
    cursor: pointer;
    font-family: inherit;
  }

  .connect-gh-btn:hover {
    filter: brightness(1.1);
  }

  .gh-auth-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
  }

  .device-code {
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.9rem;
    font-weight: 700;
    color: var(--accent);
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.15rem 0.4rem;
    letter-spacing: 0.05em;
  }

  .profile-pill.add-profile {
    color: var(--text-dim);
    font-size: 1rem;
    padding: 0.35rem 0.55rem;
  }

  .profile-pill.add-profile:hover {
    color: var(--text-primary);
    border-color: var(--accent);
  }

  /* ── Repository section ────────────────────────── */

  .repo-search-bar {
    width: 100%;
  }

  .repo-search-bar input {
    width: 100%;
    padding: 0.5rem 0.75rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.85rem;
    outline: none;
    box-sizing: border-box;
  }

  .repo-search-bar input:focus {
    border-color: var(--accent);
  }

  .repo-list {
    width: 100%;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .repo-row {
    width: 100%;
    text-align: left;
    padding: 0.45rem 0.65rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    grid-template-rows: auto auto;
    gap: 0.1rem 0.5rem;
    align-items: baseline;
    transition: border-color 0.15s ease;
    box-sizing: border-box;
  }

  .repo-row:hover:not(:disabled) {
    border-color: var(--accent);
    background: var(--bg-hover);
  }

  .repo-row:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .repo-row-name {
    font-weight: 600;
    color: var(--text-bright);
    grid-column: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .repo-row-right {
    grid-column: 2;
    grid-row: 1;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    justify-content: flex-end;
  }

  .repo-row-profile {
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  .repo-row-remove {
    width: 18px;
    height: 18px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-dim);
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
    padding: 0;
    font-family: inherit;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 0.1s ease;
  }

  .repo-row:hover .repo-row-remove {
    opacity: 1;
  }

  .repo-row-remove:hover {
    background: var(--bg-card);
    color: var(--text-bright);
  }

  .repo-row-path {
    font-size: 0.7rem;
    color: var(--text-dim);
    grid-column: 1 / -1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .repo-row-desc {
    font-size: 0.75rem;
    color: var(--text-secondary);
    grid-column: 1 / -1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .repo-row.gh .repo-row-name::after {
    content: "clone";
    font-size: 0.65rem;
    font-weight: 500;
    color: var(--accent);
    margin-left: 0.4rem;
    vertical-align: middle;
  }

  .repo-divider {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin: 0.35rem 0;
  }

  .repo-divider::before,
  .repo-divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  .repo-divider span {
    font-size: 0.7rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .empty-results {
    padding: 1rem;
    text-align: center;
    color: var(--text-dim);
    font-size: 0.82rem;
  }

  .open-repo-btn {
    margin-top: 0.25rem;
    padding: 0.5rem 1.25rem;
    background: var(--accent);
    color: var(--bg-base);
    border: none;
    border-radius: 6px;
    font-weight: 600;
    font-size: 0.85rem;
    cursor: pointer;
    font-family: inherit;
    align-self: center;
  }

  .open-repo-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .open-repo-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .open-repo-btn.secondary {
    background: var(--bg-elevated);
    color: var(--text-primary);
    border: 1px solid var(--border-light);
  }

  .open-repo-btn.secondary:hover {
    background: var(--bg-hover);
  }

  .create-repo-form {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem;
    border: 1px solid var(--border-light);
    border-radius: 8px;
    background: var(--bg-elevated);
  }

  .create-input {
    width: 100%;
    padding: 0.45rem 0.6rem;
    background: var(--bg-base);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 0.85rem;
    font-family: inherit;
    outline: none;
    box-sizing: border-box;
  }

  .create-input:focus {
    border-color: var(--accent);
  }

  .create-options {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
  }

  .visibility-toggle {
    display: flex;
    gap: 0;
  }

  .vis-btn {
    padding: 0.3rem 0.7rem;
    font-size: 0.78rem;
    font-family: inherit;
    font-weight: 500;
    border: 1px solid var(--border);
    background: var(--bg-base);
    color: var(--text-dim);
    cursor: pointer;
  }

  .vis-btn:first-child {
    border-radius: 5px 0 0 5px;
  }

  .vis-btn:last-child {
    border-radius: 0 5px 5px 0;
    border-left: none;
  }

  .vis-btn.selected {
    background: var(--accent);
    color: var(--bg-base);
    border-color: var(--accent);
  }

  .vis-btn.selected + .vis-btn {
    border-left: none;
  }

  .readme-check {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.8rem;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .readme-check input[type="checkbox"] {
    accent-color: var(--accent);
  }

  .create-actions {
    display: flex;
    gap: 0.5rem;
    justify-content: flex-end;
  }

  /* ── App-wide status bar ────────────────────────── */

  .app-statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 0.6rem;
    height: 22px;
    background: var(--bg-sidebar);
    border-top: 1px solid var(--border);
    font-size: 0.64rem;
    color: var(--text-dim);
    flex-shrink: 0;
    gap: 0.5rem;
    user-select: none;
  }

  .statusbar-left, .statusbar-right {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .lsp-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    padding: 0.05rem 0.45rem;
    border-radius: 9px;
    font-size: 0.6rem;
    font-weight: 500;
    letter-spacing: 0.02em;
    background: color-mix(in srgb, var(--status-ok, #6a4) 12%, transparent);
    color: var(--status-ok, #6a4);
  }

  .lsp-pill.busy {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }

  .lsp-pill.error {
    background: color-mix(in srgb, var(--status-error) 12%, transparent);
    color: var(--status-error);
  }

  .lsp-pill.ready {
    background: color-mix(in srgb, var(--status-ok, #6a4) 12%, transparent);
    color: var(--status-ok, #6a4);
  }

  .lsp-pill.stopped {
    background: color-mix(in srgb, var(--text-dim) 12%, transparent);
    color: var(--text-dim);
  }

  .lsp-pill-spinner {
    display: inline-block;
    width: 7px;
    height: 7px;
    border: 1.5px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: lsp-pill-spin 0.8s linear infinite;
  }

  @keyframes lsp-pill-spin {
    to { transform: rotate(360deg); }
  }

  /* ── App layout ──────────────────────────────────── */

  .app {
    height: 100vh;
    display: flex;
    flex-direction: column;
  }

  .main-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  .mode-stack {
    flex: 1;
    position: relative;
    min-height: 0;
  }

  .mode-layer {
    position: absolute;
    inset: 0;
    display: flex;
    visibility: hidden;
    pointer-events: none;
    background: var(--bg-base);
  }

  .mode-layer.mode-visible {
    visibility: visible;
    pointer-events: auto;
    z-index: 1;
  }

  /* ── Plan sub-views ────────────────────────────────── */

  .plan-sub-layer {
    position: absolute;
    inset: 0;
    display: none;
    flex-direction: column;
  }

  .plan-sub-layer.plan-visible {
    display: flex;
    z-index: 1;
  }

  .plan-terminal-layout {
    flex-direction: column;
  }

  .plan-terminal-tabs {
    display: flex;
    align-items: center;
    padding: 0 0.5rem;
    height: 30px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: 0.15rem;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }

  .plan-terminal-tabs::-webkit-scrollbar {
    display: none;
  }

  .plan-term-tab {
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

  .plan-term-tab:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .plan-term-tab.active {
    color: var(--text-bright);
    background: var(--border);
  }

  .plan-term-tab .tab-close {
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

  .plan-term-tab .tab-close:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .plan-term-add {
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

  .plan-term-add:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .plan-terminal-body {
    flex: 1;
    position: relative;
    min-height: 0;
  }

  .plan-terminal-layer {
    position: absolute;
    inset: 0;
    display: none;
  }

  .plan-terminal-layer.visible {
    display: flex;
    z-index: 1;
  }

</style>

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
    saveImage,
    onAgentStatus,
    onWorkspaceUpdated,
    stopAgent,
    renameBranch,
    getRepoSettings,
    getPrStatus,
    getPrTemplate,
    getChangedFiles,
    readWorkspaceFile,
    gitCommit,
    gitPush,
    ghPrMerge,
    generateCommitMessage,
    checkBaseUpdates,
    updateFromBase,
    saveTodos,
    loadTodos,
    checkGhCli,
    ghAuthLogin,
    cancelGhAuthLogin,
    listGhRepos,
    cloneRepo,
    setRepoProfile,
    checkRepoGhAccess,
    type RepoDetail,
    type RepoSettings,
    type WorkspaceInfo,
    type AgentEvent,
    type PrStatus,
    type GhCliStatus,
    type GhRepoEntry,
  } from "$lib/ipc";
  import {
    addUserMessage,
    addAssistantMessage,
    addActionMessage,
    loadPersistedMessages,
    clearWorkspaceData,
    setSending,
    sendingByWorkspace,
  } from "$lib/stores/messages.svelte";
  import { onMount } from "svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import WorkspacePanel, { type PanelTab } from "$lib/components/WorkspacePanel.svelte";
  import KanbanBoard from "$lib/components/KanbanBoard.svelte";
  import ReviewAlertBar from "$lib/components/ReviewAlertBar.svelte";
  import { type PastedImage } from "$lib/components/ChatPanel.svelte";
  import type { Mention } from "$lib/components/MentionInput.svelte";
  import RepoSettingsPanel from "$lib/components/RepoSettings.svelte";
  import SearchModal from "$lib/components/SearchModal.svelte";
  import { type ReviewState } from "$lib/components/ReviewPill.svelte";
  import type { ChatPanelApi } from "$lib/components/ChatPanel.svelte";
  import Toasts from "$lib/components/Toasts.svelte";
  import { addToast } from "$lib/stores/toasts.svelte";

  // ── PR Status Cache (localStorage) ─────────────────────
  const PR_CACHE_KEY = "korlap:pr-status-cache";

  function loadPrStatusCache(): Record<string, PrStatus> {
    try {
      const raw = localStorage.getItem(PR_CACHE_KEY);
      return raw ? JSON.parse(raw) : {};
    } catch {
      return {};
    }
  }

  function savePrStatusCache() {
    try {
      const obj: Record<string, PrStatus> = {};
      for (const [k, v] of prStatusMap) obj[k] = v;
      localStorage.setItem(PR_CACHE_KEY, JSON.stringify(obj));
    } catch {
      // localStorage full or unavailable — non-critical
    }
  }

  function hydratePrStatusFromCache(workspaceIds: string[]) {
    const cache = loadPrStatusCache();
    for (const wsId of workspaceIds) {
      if (cache[wsId] && !prStatusMap.has(wsId)) {
        prStatusMap.set(wsId, cache[wsId]);
      }
    }
  }

  function removePrStatusCacheEntry(wsId: string) {
    try {
      const cache = loadPrStatusCache();
      delete cache[wsId];
      localStorage.setItem(PR_CACHE_KEY, JSON.stringify(cache));
    } catch {
      // non-critical
    }
  }

  function clearPrStatusCacheForRepo(workspaceIds: string[]) {
    try {
      const cache = loadPrStatusCache();
      for (const wsId of workspaceIds) delete cache[wsId];
      localStorage.setItem(PR_CACHE_KEY, JSON.stringify(cache));
    } catch {
      // non-critical
    }
  }

  const DEFAULT_REVIEW_PROMPT = `## Code Review Instructions

**CRITICAL — Output format:** Do NOT produce any text output until you reach step 8. No narration, no status updates, no "let me do X" messages. Use tool calls silently. Your ONLY text output must be the final result from step 8. If no issues survived validation, your entire text output must be exactly: "No issues found." — nothing else.

**Getting the workspace diff:** All diff commands below use the merge-base to capture every change on this branch (committed and uncommitted) relative to the target:

\`\`\`bash
MERGE_BASE=$(git merge-base origin/{{base_branch}} HEAD)
# File list
git diff $MERGE_BASE --name-only
# Full diff
git diff $MERGE_BASE
\`\`\`

Do NOT separate committed vs uncommitted changes — review them as a single unified diff.

---

1. Launch a haiku agent to return a list of file paths (not their contents) for all relevant CLAUDE.md files including:

    - The root CLAUDE.md file, if it exists
    - Any CLAUDE.md files in directories containing files modified by the workspace diff (use the \`git diff $MERGE_BASE --name-only\` command above)

2. If this workspace has an associated PR, read the title and description (but not the changes). This will be helpful context.

3. In parallel with step 2, launch a sonnet agent to view the workspace diff and return a summary of the changes.

4. Launch 4 agents in parallel to independently review the workspace diff. Each agent should return the list of issues, where each issue includes a description and the reason it was flagged (e.g. "CLAUDE.md adherence", "bug"). The agents should do the following:

    Agents 1 + 2: CLAUDE.md or AGENTS.md compliance sonnet agents
    Audit changes for CLAUDE.md or AGENTS.md compliance in parallel. Note: When evaluating CLAUDE.md or AGENTS.md compliance for a file, you should only consider CLAUDE.md or AGENTS.md files that share a file path with the file or parents.

    Agent 3: Opus bug agent
    Scan for obvious bugs. Focus only on the diff itself without reading extra context. Flag only significant bugs; ignore nitpicks and likely false positives. Do not flag issues that you cannot validate without looking at context outside of the git diff.

    Agent 4: Opus bug agent
    Look for problems that exist in the introduced code. This could be security issues, incorrect logic, etc. Only look for issues that fall within the changed code.

    **CRITICAL: We only want HIGH SIGNAL issues.** This means:

    - Objective bugs that will cause incorrect behavior at runtime
    - Clear, unambiguous CLAUDE.md violations where you can quote the exact rule being broken

    We do NOT want:

    - Subjective concerns or "suggestions"
    - Style preferences not explicitly required by CLAUDE.md
    - Potential issues that "might" be problems
    - Anything requiring interpretation or judgment calls

    If you are not certain an issue is real, do not flag it. False positives erode trust and waste reviewer time.

    In addition to the above, each subagent should be told the PR title and description. This will help provide context regarding the author's intent.

5. For each issue found in the previous step, launch parallel subagents to validate the issue. These subagents should get the PR title and description along with a description of the issue. The agent's job is to review the issue to validate that the stated issue is truly an issue with high confidence. For example, if an issue such as "variable is not defined" was flagged, the subagent's job would be to validate that is actually true in the code. Another example would be CLAUDE.md issues. The agent should validate that the CLAUDE.md rule that was violated is scoped for this file and is actually violated. Use Opus subagents for bugs and logic issues, and sonnet agents for CLAUDE.md violations.

6. Filter out any issues that were not validated in step 5. This step will give us our list of high signal issues for our review.

7. Post inline comments for each issue using \`gh api\` to comment on the PR:

    **IMPORTANT: Only post ONE comment per unique issue.**

8. Write out a list of issues found, along with the location of the comment. For example:

    ### **#1 Empty input causes crash**

    If the input field is empty when page loads, the app will crash.

    File: src/ui/Input.tsx

    ### **#2 Dead code**

    The getUserData function is now unused. It should be deleted.

    File: src/core/UserData.ts

Use this list when evaluating issues in Steps 5 and 6 (these are false positives, do NOT flag):

-   Pre-existing issues
-   Something that appears to be a bug but is actually correct
-   Pedantic nitpicks that a senior engineer would not flag
-   Issues that a linter will catch (do not run the linter to verify)
-   General code quality concerns (e.g., lack of test coverage, general security issues) unless explicitly required in CLAUDE.md or AGENTS.md
-   Issues mentioned in CLAUDE.md or AGENTS.md but explicitly silenced in the code (e.g., via a lint ignore comment)

Notes:

-   All subagents should be explicitly instructed not to post comments themselves. Only you, the main agent, should post comments.
-   Do not use the AskUserQuestion tool. Your goal should be to complete the entire review without user intervention.
-   Use gh CLI to interact with GitHub (e.g., fetch pull requests, create comments). Do not use web fetch.
-   You must cite and link each issue in inline comments (e.g., if referring to a CLAUDE.md or AGENTS.md rule, include a link to it).

## Fallback: if you don't have access to subagents

If you don't have subagents, perform all the steps above yourself sequentially instead of launching agents. Do each review axis (CLAUDE.md compliance, bug scan, introduced problems) yourself, and validate each issue yourself.

## Fallback: if you don't have access to the workspace diff tool

If you don't have access to a workspace diff tool, use the git commands from the top of this prompt to get the workspace diff.

No need to mention in your report whether or not you used one of the fallback strategies; it's usually irrelevant.`;

  // ── State ──────────────────────────────────────────────

  let repos = $state<RepoDetail[]>([]);
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeRepo = $state<RepoDetail | null>(null);
  let selectedWsId = $state<string | null>(null);
  let activeTab = $state<PanelTab>("chat");
  let diffRefreshTrigger = $state(0);
  let showSettings = $state(false);
  let creatingWsId = $state<string | null>(null);
  let repoSettings = $state<RepoSettings | null>(null);
  let prStatusMap = new SvelteMap<string, PrStatus>();
  let changeCounts = new SvelteMap<string, { additions: number; deletions: number }>();
  let planModeByWorkspace = new SvelteMap<string, boolean>();
  let thinkingModeByWorkspace = new SvelteMap<string, boolean>();
  let fileNavigatePath = $state<string | null>(null);
  let fileNavigateLine = $state<number | null>(null);
  let showSearchModal = $state(false);
  let chatPanelApis = new SvelteMap<string, ChatPanelApi>();
  let reviewByWorkspace = new SvelteMap<string, ReviewState>();
  let gitOpInProgress = new SvelteMap<string, boolean>();
  let baseBehindMap = new SvelteMap<string, number>();
  let updatingBranchMap = new SvelteMap<string, boolean>();
  let titleBarRef: TitleBar | undefined = $state();
  let repoDropdownIndex = $state(-1);

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

  interface TodoItem {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    imagePaths?: string[];
    mentionPaths?: string[];
    planMode?: boolean;
    thinkingMode?: boolean;
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
    [...workspaces].sort((a, b) => a.created_at - b.created_at),
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

    (async () => {
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
          handleNewWorkspace();
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
          appMode = "plan";
          break;
        case "2":
          e.preventDefault();
          appMode = "work";
          break;
      }
    }

    window.addEventListener("keydown", handleKeydown);

    // Poll PR status every 5s for workspaces that have a PR open
    const prPollInterval = setInterval(() => {
      for (const [wsId, pr] of prStatusMap) {
        if (pr.state === "open") {
          refreshPrStatus(wsId);
        }
      }
    }, 5_000);

    // Check base branch updates every 60s for the selected workspace.
    // Lighter touch than PR polling since it involves a `git fetch`.
    const basePollInterval = setInterval(() => {
      if (selectedWsId) refreshBaseUpdates(selectedWsId);
    }, 60_000);

    return () => {
      unlistenStatus?.();
      unlistenWsUpdate?.();
      clearInterval(prPollInterval);
      clearInterval(basePollInterval);
      window.removeEventListener("keydown", handleKeydown);
    };
  });

  // ── Handlers ───────────────────────────────────────────

  function selectRepo(repo: RepoDetail) {
    activeRepo = repo;
    selectedWsId = null;


    listWorkspaces(repo.id).then((ws) => {
      workspaces = ws;
      // Hydrate PR statuses from cache immediately so cards render in correct columns
      hydratePrStatusFromCache(ws.map((w) => w.id));
      ws.forEach((w) => loadPersistedMessages(w.id));
      ws.forEach((w) => {
        refreshChangeCounts(w.id);
        refreshPrStatus(w.id);
      });
    }).catch((e) => { addToast(String(e)); });

    getRepoSettings(repo.id).then((s) => { repoSettings = s; }).catch(() => {});

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
    activeTab = "chat";
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

  async function handleNewTodo(data: { title: string; description: string; newImages: PastedImage[]; existingPaths: string[]; mentions?: Mention[]; planMode?: boolean; thinkingMode?: boolean }) {
    if (!activeRepo) return;
    if (!data.title.trim() && data.newImages.length === 0 && data.existingPaths.length === 0) return;
    try {
      const savedPaths = await saveTodoImages(data.newImages);
      const allPaths = [...data.existingPaths, ...savedPaths];
      const mentionPaths = data.mentions?.map((m) => m.path) ?? [];
      todos.push({
        id: crypto.randomUUID(),
        repo_id: activeRepo.id,
        title: data.title.trim(),
        description: data.description.trim(),
        imagePaths: allPaths.length > 0 ? allPaths : undefined,
        mentionPaths: mentionPaths.length > 0 ? mentionPaths : undefined,
        planMode: data.planMode || undefined,
        thinkingMode: data.thinkingMode || undefined,
        created_at: Date.now() / 1000,
      });
      persistTodos();
    } catch (e) {
      addToast(`Failed to save images: ${e}`);
    }
  }

  async function handleEditTodo(todoId: string, data: { title: string; description: string; newImages: PastedImage[]; existingPaths: string[]; mentions?: Mention[]; planMode?: boolean; thinkingMode?: boolean }) {
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
      persistTodos();
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
    };
    creatingWsId = tempId;
    workspaces.push(placeholder);
    selectWorkspace(tempId);

    // Optimistically remove the todo card immediately on start
    handleRemoveTodo(todoId);

    try {
      const ws = await createWorkspace(repoId, todo.title, todo.description || undefined);
      const idx = workspaces.findIndex((w) => w.id === tempId);
      if (idx >= 0) workspaces[idx] = ws;
      selectedWsId = ws.id;
      creatingWsId = null;

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
        hidden: true,
      });
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
    activeTab = "chat";
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
          }
        } else if (event.type === "done") {
          setSending(wsId, false);
          diffRefreshTrigger++;
          refreshChangeCounts(wsId);
          refreshPrStatus(wsId);
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
        } else if (event.type === "error") {
          addToast(event.message);
          setSending(wsId, false);
          pendingDrain.delete(wsId);
        }
      }, msg.planMode, msg.thinkingMode);
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
    routeMessage(wsId, {
      id: crypto.randomUUID(),
      prompt,
      fullPrompt: prompt,
      images: [],
      mentions: [],
      planMode: false,
      thinkingMode,
      actionLabel,
    });
  }

  async function handleSend(prompt: string, images: PastedImage[] = [], mentions: Mention[] = [], planMode: boolean = false) {
    if (!selectedWsId || reviewByWorkspace.has(selectedWsId)) return;
    const wsId = selectedWsId;
    const thinkingMode = thinkingModeByWorkspace.get(wsId) ?? repoSettings?.default_thinking ?? false;

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
    });
  }

  /** Send immediately, bypassing the queue. Used for AskUserQuestion answers. */
  async function handleSendImmediate(prompt: string) {
    if (!selectedWsId) return;
    const wsId = selectedWsId;
    const thinkingMode = thinkingModeByWorkspace.get(wsId) ?? repoSettings?.default_thinking ?? false;
    sendDirect(wsId, {
      id: crypto.randomUUID(),
      prompt,
      fullPrompt: prompt,
      images: [],
      mentions: [],
      planMode: false,
      thinkingMode,
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

  async function handlePrAction() {
    if (!selectedWs || !activeRepo) return;
    const wsId = selectedWs.id;
    if (gitOpInProgress.get(wsId)) return;

    const pr = prStatusMap.get(wsId);

    if (pr && pr.state === "open") {
      const cc = changeCounts.get(wsId);
      const hasLocalChanges = cc && (cc.additions !== pr.additions || cc.deletions !== pr.deletions);

      // ── Agent-delegated: conflicts & failing checks need reasoning ──
      if (pr.mergeable === "conflicting") {
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `PR #${pr.number} has merge conflicts with ${baseBranch}.\n\nResolve them:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n5. Push\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving conflicts on PR #${pr.number}`);
        activeTab = "chat";
        return;
      }
      if (pr.checks === "failing") {
        sendPrompt(wsId, `PR #${pr.number} has failing checks. Investigate the failures using \`gh pr checks ${pr.number}\`, fix the issues, commit, and push.`, `Fixing checks on PR #${pr.number}`);
        activeTab = "chat";
        return;
      }

      // ── Direct CLI: commit & push ──
      if (hasLocalChanges) {
        gitOpInProgress.set(wsId, true);
        addActionMessage(wsId, crypto.randomUUID(), `Committing & pushing to PR #${pr.number}`);
        try {
          const msg = await generateCommitMessage(wsId);
          await gitCommit(wsId, msg);
          await gitPush(wsId);
          addToast("Pushed successfully", "success");
          refreshChangeCounts(wsId);
          refreshPrStatus(wsId);
        } catch (e) {
          addToast(String(e));
          activeTab = "chat";
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
          activeTab = "chat";
          sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
        } finally {
          gitOpInProgress.delete(wsId);
        }
        return;
      }

      // ── Direct CLI: merge ──
      gitOpInProgress.set(wsId, true);
      addActionMessage(wsId, crypto.randomUUID(), `Merging PR #${pr.number}`);
      try {
        await ghPrMerge(wsId, pr.number);
        addToast(`PR #${pr.number} merged`, "success");
        refreshPrStatus(wsId);
      } catch (e) {
        addToast(String(e));
        activeTab = "chat";
        sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
      } finally {
        gitOpInProgress.delete(wsId);
      }
      return;
    }

    // ── No PR: commit & push directly, then agent creates PR (needs conversation context) ──
    const baseBranch = activeRepo.default_branch;
    const files = await getChangedFiles(wsId).catch(() => []);
    const template = await getPrTemplate(activeRepo.id).catch(() => "");

    gitOpInProgress.set(wsId, true);
    try {
      // Commit & push if there are local changes
      if (files.length > 0) {
        addActionMessage(wsId, crypto.randomUUID(), "Committing & pushing changes");
        const msg = await generateCommitMessage(wsId);
        await gitCommit(wsId, msg);
        await gitPush(wsId);
      } else {
        // Just push any unpushed commits
        addActionMessage(wsId, crypto.randomUUID(), "Pushing to origin");
        await gitPush(wsId);
      }
    } catch (e) {
      addToast(String(e));
      activeTab = "chat";
      sendPrompt(wsId, `This git operation failed:\n\n\`\`\`\n${e}\n\`\`\`\n\nDiagnose the issue and fix it.`, "Fixing git error");
      gitOpInProgress.delete(wsId);
      return;
    }
    gitOpInProgress.delete(wsId);

    // Agent creates PR — it has conversation context for a good description
    activeTab = "chat";
    let prompt: string;
    const customMsg = repoSettings?.pr_message?.trim();

    if (customMsg) {
      prompt = customMsg
        .replace(/\{\{branch\}\}/g, selectedWs.branch)
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

  async function handleStop() {
    if (!selectedWsId) return;
    try {
      await stopAgent(selectedWsId);
      setSending(selectedWsId, false);
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

  async function handleReview() {
    if (!selectedWsId || !activeRepo) return;
    const wsId = selectedWsId;

    if (sendingByWorkspace.get(wsId) || reviewByWorkspace.has(wsId)) return;
    const pr = prStatusMap.get(wsId);

    const baseBranch = activeRepo.default_branch;

    const customMsg = repoSettings?.review_message?.trim();
    let reviewPrompt = DEFAULT_REVIEW_PROMPT;
    if (customMsg) {
      reviewPrompt += `\n\n## Additional Instructions\n\n${customMsg}`;
    }
    reviewPrompt = reviewPrompt
      .replace(/\{\{branch\}\}/g, selectedWs!.branch)
      .replace(/\{\{base_branch\}\}/g, baseBranch)
      .replace(/\{\{pr_number\}\}/g, pr?.state === "open" ? String(pr.number) : "N/A")
      .replace(/\{\{pr_title\}\}/g, pr?.state === "open" ? (pr.title ?? "") : "N/A");

    addActionMessage(wsId, crypto.randomUUID(), "Reviewing code");

    reviewByWorkspace.set(wsId, {
      status: "running",
      currentTask: "Starting review...",
      resultMarkdown: "",
    });

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
        } else if (event.type === "done") {
          review.status = "complete";
          reviewByWorkspace.set(wsId, { ...review });
        } else if (event.type === "error") {
          review.status = "complete";
          review.resultMarkdown = `**Review failed:** ${event.message}`;
          reviewByWorkspace.set(wsId, { ...review });
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

  // Refresh change counts (fast, local git only)
  async function refreshChangeCounts(wsId: string) {
    try {
      const files = await getChangedFiles(wsId);
      const adds = files.reduce((s, f) => s + f.additions, 0);
      const dels = files.reduce((s, f) => s + f.deletions, 0);
      const prev = changeCounts.get(wsId);
      if (prev && prev.additions === adds && prev.deletions === dels) {
        return; // No change — skip reactive update
      }
      changeCounts.set(wsId, { additions: adds, deletions: dels });
    } catch (e) {
      console.warn(`refreshChangeCounts(${wsId}):`, e);
    }
  }

  // Refresh PR status (slow, network call — run in background)
  // Only triggers reactivity when the status actually changed to avoid DOM thrash.
  async function refreshPrStatus(wsId: string) {
    try {
      const pr = await getPrStatus(wsId);
      const prev = prStatusMap.get(wsId);
      if (
        prev &&
        prev.state === pr.state &&
        prev.checks === pr.checks &&
        prev.mergeable === pr.mergeable &&
        prev.number === pr.number &&
        prev.additions === pr.additions &&
        prev.deletions === pr.deletions &&
        prev.title === pr.title &&
        prev.ahead_by === pr.ahead_by
      ) {
        return; // No change — skip reactive update
      }
      prStatusMap.set(wsId, pr);
      savePrStatusCache();
    } catch (e) {
      console.warn(`refreshPrStatus(${wsId}):`, e);
    }
  }

  // Check if base branch has new commits since workspace branched off.
  // Only triggers reactivity when behind_by actually changes.
  async function refreshBaseUpdates(wsId: string) {
    try {
      const status = await checkBaseUpdates(wsId);
      const prev = baseBehindMap.get(wsId);
      if (prev === status.behind_by) return;
      baseBehindMap.set(wsId, status.behind_by);
    } catch (e) {
      console.warn(`refreshBaseUpdates(${wsId}):`, e);
    }
  }

  async function handleUpdateBranch() {
    if (!selectedWs || !activeRepo) return;
    const wsId = selectedWs.id;
    if (updatingBranchMap.get(wsId)) return;

    updatingBranchMap.set(wsId, true);
    try {
      await updateFromBase(wsId);
      addToast("Branch updated from " + activeRepo.default_branch);
      baseBehindMap.set(wsId, 0);
      refreshChangeCounts(wsId);
      diffRefreshTrigger++;
    } catch (e) {
      const errMsg = String(e);
      if (errMsg.includes("conflicts")) {
        addToast("Merge conflicts — delegating to agent");
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `Updating from ${baseBranch} caused merge conflicts. The automatic merge was aborted.\n\nPlease resolve this:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving merge conflicts with ${baseBranch}`);
        activeTab = "chat";
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
      {selectedWs}
      {appMode}
      onModeChange={(m) => { appMode = m; }}
      onSelectRepo={selectRepo}
      onSettings={() => (showSettings = true)}
      onGoHome={() => { activeRepo = null; }}
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
          />

          <WorkspacePanel
            bind:activeTab
            bind:fileNavigatePath
            bind:fileNavigateLine
            {selectedWs}
            {selectedWsId}
            {activeWorkspaces}
            {creatingWsId}
            {changeCounts}
            {planModeByWorkspace}
            {thinkingModeByWorkspace}
            {reviewByWorkspace}
            {repoSettings}
            {diffRefreshTrigger}
            prStatus={selectedWsId ? prStatusMap.get(selectedWsId) : undefined}
            wsChanges={selectedWsId ? changeCounts.get(selectedWsId) : undefined}
            baseBehindBy={selectedWsId ? baseBehindMap.get(selectedWsId) ?? 0 : 0}
            updatingBranch={selectedWsId ? updatingBranchMap.get(selectedWsId) ?? false : false}
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
            onRemoveFromQueue={(wsId, id) => removeFromQueue(wsId, id)}
            onPlanModeChange={(wsId, enabled) => planModeByWorkspace.set(wsId, enabled)}
            onThinkingModeChange={(wsId, enabled) => thinkingModeByWorkspace.set(wsId, enabled)}
            onExecutePlan={(wsId) => {
              planModeByWorkspace.set(wsId, false);
              sendPrompt(wsId, "Execute the plan above. Do not ask for confirmation — just do it.", "Executing plan");
            }}
            onChatReady={(wsId, api) => chatPanelApis.set(wsId, api)}
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
        <KanbanBoard
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
          onCardClick={handleKanbanCardClick}
          onSpawnAgent={handleSpawnFromTodo}
          onNewTodo={handleNewTodo}
          onEditTodo={handleEditTodo}
          onRemoveTodo={handleRemoveTodo}
          onRemoveWorkspace={handleRemove}
          onRemoveAllDone={handleRemoveAllDone}
        />
      </div>
    </div>


    {#if showSearchModal && selectedWsId}
      <SearchModal
        workspaceId={selectedWsId}
        onClose={() => { showSearchModal = false; }}
        onAddToContext={(path, displayName, lineNumber) => {
          showSearchModal = false;
          chatPanelApis.get(selectedWsId!)?.addMention({ type: "file", path, displayName, lineNumber });
          activeTab = "chat";
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

</style>

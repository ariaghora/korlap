<script lang="ts">
  import { open, confirm } from "@tauri-apps/plugin-dialog";
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
    type RepoDetail,
    type RepoSettings,
    type WorkspaceInfo,
    type AgentEvent,
    type PrStatus,
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
  import ChatPanel, { type PastedImage } from "$lib/components/ChatPanel.svelte";
  import type { Mention } from "$lib/components/MentionInput.svelte";
  import DiffViewer from "$lib/components/DiffViewer.svelte";
  import FileBrowser from "$lib/components/FileBrowser.svelte";
  import TerminalView from "$lib/components/Terminal.svelte";
  import RepoSettingsPanel from "$lib/components/RepoSettings.svelte";
  import SearchModal from "$lib/components/SearchModal.svelte";
  import ReviewPill, { type ReviewState } from "$lib/components/ReviewPill.svelte";
  import type { ChatPanelApi } from "$lib/components/ChatPanel.svelte";

  type PanelTab = "chat" | "diff" | "files" | "terminal";

  const DEFAULT_REVIEW_PROMPT = `## Code Review Instructions

**IMPORTANT — Output format:** Do not narrate your process, do not describe what you're about to do, and do not print intermediate status updates. Your only output should be the final list of validated issues (step 8). If no issues were found, output only: "No issues found."

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
  let error = $state("");
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
  let showSearchModal = $state(false);
  let chatPanelApis = new SvelteMap<string, ChatPanelApi>();
  let reviewByWorkspace = new SvelteMap<string, ReviewState>();
  let titleBarRef: TitleBar | undefined = $state();
  let repoDropdownIndex = $state(-1);

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

  // ── Lifecycle ──────────────────────────────────────────

  onMount(() => {
    let unlistenStatus: (() => void) | undefined;
    let unlistenWsUpdate: (() => void) | undefined;

    (async () => {
      listRepos().then((r) => {
        repos = r;
        if (r.length > 0) selectRepo(r[0]);
      }).catch((e) => { error = String(e); });

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
            handleOpenRepo();
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
          if (e.shiftKey && selectedWsId) {
            e.preventDefault();
            showSearchModal = true;
          }
          break;
        default:
          if (!inInput && e.key >= "1" && e.key <= "9") {
            e.preventDefault();
            const idx = parseInt(e.key) - 1;
            if (idx < activeWorkspaces.length) {
              selectWorkspace(activeWorkspaces[idx].id);
            }
          }
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

    return () => {
      unlistenStatus?.();
      unlistenWsUpdate?.();
      clearInterval(prPollInterval);
      window.removeEventListener("keydown", handleKeydown);
    };
  });

  // ── Handlers ───────────────────────────────────────────

  async function handleOpenRepo() {
    error = "";
    try {
      const selected = await open({
        directory: true,
        title: "Open a git repository",
      });
      if (!selected) return;
      const repo = await addRepo(selected);
      if (!repos.find((r) => r.id === repo.id)) {
        repos = [...repos, repo];
      }
      await selectRepo(repo);
    } catch (e) {
      error = String(e);
    }
  }

  function selectRepo(repo: RepoDetail) {
    activeRepo = repo;
    selectedWsId = null;
    error = "";

    listWorkspaces(repo.id).then((ws) => {
      workspaces = ws;
      ws.forEach((w) => loadPersistedMessages(w.id));
      ws.forEach((w) => {
        refreshChangeCounts(w.id);
        refreshPrStatus(w.id);
      });
    }).catch((e) => { error = String(e); });

    getRepoSettings(repo.id).then((s) => { repoSettings = s; }).catch(() => {});
  }

  async function handleRemoveRepo() {
    if (!activeRepo) return;
    const repoId = activeRepo.id;
    error = "";

    try {
      await removeRepo(repoId);
      showSettings = false;
      repos = repos.filter((r) => r.id !== repoId);
      workspaces = [];
      selectedWsId = null;
      sendingByWorkspace.clear();
      prStatusMap.clear();
      changeCounts.clear();
      planModeByWorkspace.clear();
      thinkingModeByWorkspace.clear();
      activeRepo = repos.length > 0 ? repos[0] : null;
      if (activeRepo) selectRepo(activeRepo);
    } catch (e) {
      error = String(e);
    }
  }

  function handleNewWorkspace() {
    if (!activeRepo || creatingWsId) return;
    error = "";

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
      error = String(e);
    });
  }

  async function handleRemove(wsId: string) {
    const ws = workspaces.find((w) => w.id === wsId);
    const name = ws?.name ?? wsId;

    const confirmed = await confirm(
      `This will permanently remove "${name}" — its worktree, messages, and session data will be deleted.`,
      { title: "Remove workspace?", kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" },
    );
    if (!confirmed) return;

    error = "";

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
    changeCounts.delete(wsId);
    planModeByWorkspace.delete(wsId);
    thinkingModeByWorkspace.delete(wsId);

    removeWorkspace(wsId).catch((e) => {
      // Restore on failure
      if (removed) workspaces.push(removed);
      error = String(e);
    });
  }

  // ── Send pipeline ───────────────────────────────────────

  /** Core send — assumes caller has verified it's safe to send. */
  async function sendDirect(wsId: string, msg: QueuedMessage) {
    error = "";
    setSending(wsId, true);

    if (msg.actionLabel) {
      addActionMessage(wsId, crypto.randomUUID(), msg.actionLabel);
    } else {
      addUserMessage(wsId, crypto.randomUUID(), msg.prompt || "(images attached)", msg.imageDataUrls, msg.msgMentions, msg.planMode || undefined);
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
          error = event.message;
          setSending(wsId, false);
          pendingDrain.delete(wsId);
        }
      }, msg.planMode, msg.thinkingMode);
    } catch (e) {
      error = String(e);
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
        error = `Failed to save images: ${e}`;
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
      error = String(e);
    }
  }

  async function handlePrAction() {
    if (!selectedWs || !activeRepo) return;
    const wsId = selectedWs.id;
    const pr = prStatusMap.get(wsId);

    if (pr && pr.state === "open") {
      const cc = changeCounts.get(wsId);
      const hasLocalChanges = cc && (cc.additions !== pr.additions || cc.deletions !== pr.deletions);

      if (pr.mergeable === "conflicting") {
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `PR #${pr.number} has merge conflicts with ${baseBranch}.\n\nResolve them:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n5. Push\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving conflicts on PR #${pr.number}`);
      } else if (pr.checks === "failing") {
        sendPrompt(wsId, `PR #${pr.number} has failing checks. Investigate the failures using \`gh pr checks ${pr.number}\`, fix the issues, commit, and push.`, `Fixing checks on PR #${pr.number}`);
      } else if (hasLocalChanges) {
        sendPrompt(wsId, `There are uncommitted local changes. Run \`git status\` and \`git diff\` to review them, commit with a descriptive message, and push to origin. Only say "Pushed successfully" on success. If it fails, explain why.`, `Committing & pushing to PR #${pr.number}`);
      } else if (pr.ahead_by > 0) {
        sendPrompt(wsId, `Push local commits to origin. Run \`git push\`. Only say "Pushed successfully" on success. If it fails, explain why.`, `Pushing to PR #${pr.number}`);
      } else {
        sendPrompt(wsId, `Merge PR #${pr.number} using \`gh pr merge ${pr.number} --squash --delete-branch=false\`. Only say "PR #${pr.number} merged successfully" on success. If it fails, explain why.`, `Merging PR #${pr.number}`);
      }
      activeTab = "chat";
      return;
    } else {
      // Create PR
      const files = await getChangedFiles(wsId).catch(() => []);
      const baseBranch = activeRepo.default_branch;
      const template = await getPrTemplate(activeRepo.id).catch(() => "");

      let prompt: string;
      const customMsg = repoSettings?.pr_message?.trim();

      if (customMsg) {
        // Interpolate template variables in custom PR message
        prompt = customMsg
          .replace(/\{\{branch\}\}/g, selectedWs.branch)
          .replace(/\{\{base_branch\}\}/g, baseBranch)
          .replace(/\{\{file_count\}\}/g, String(files.length))
          .replace(/\{\{pr_template\}\}/g, template
            ? `\n## PR Description Template\n\nThis workspace has a PR template. Use it:\n\n\`\`\`markdown\n${template}\n\`\`\`\n`
            : "");
      } else {
        // Default prompt
        prompt = `Create a pull request.\n\n`;
        prompt += `There are ${files.length} uncommitted changes.\n`;
        prompt += `The current branch is ${selectedWs.branch}.\n`;
        prompt += `The target branch is origin/${baseBranch}.\n\n`;
        prompt += `Follow these steps:\n`;
        prompt += `1. Run \`git diff\` to review uncommitted changes\n`;
        prompt += `2. Commit them with a descriptive message\n`;
        prompt += `3. Push to origin\n`;
        prompt += `4. Use \`gh pr create --base ${baseBranch}\` to create a PR. Keep the title under 80 characters. Keep the description under five sentences unless there's a template.\n\n`;
        prompt += `If any step fails, explain the issue.\n`;

        if (template) {
          prompt += `\n## PR Description Template\n\nThis repo has a PR template. Use it:\n\n\`\`\`markdown\n${template}\n\`\`\`\n`;
        }
      }

      activeTab = "chat";
      sendPrompt(wsId, prompt, "Creating pull request");
    }
  }

  async function handleStop() {
    if (!selectedWsId) return;
    try {
      await stopAgent(selectedWsId);
      setSending(selectedWsId, false);
    } catch (e) {
      error = String(e);
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
    if (!pr || pr.state !== "open") return;

    const baseBranch = activeRepo.default_branch;

    const customMsg = repoSettings?.review_message?.trim();
    let reviewPrompt = DEFAULT_REVIEW_PROMPT;
    if (customMsg) {
      reviewPrompt += `\n\n## Additional Instructions\n\n${customMsg}`;
    }
    reviewPrompt = reviewPrompt
      .replace(/\{\{branch\}\}/g, selectedWs!.branch)
      .replace(/\{\{base_branch\}\}/g, baseBranch)
      .replace(/\{\{pr_number\}\}/g, String(pr.number))
      .replace(/\{\{pr_title\}\}/g, pr.title ?? "");

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
            review.resultMarkdown += (review.resultMarkdown ? "\n\n" : "") + event.text.trim();
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
    } catch {
      // ignore
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
    } catch {
      // gh not installed or no remote
    }
  }

  function selectWorkspace(wsId: string) {
    selectedWsId = wsId;
    // Refresh PR status in background so it's current when the user lands on the workspace
    refreshPrStatus(wsId);
  }
</script>

{#if !activeRepo}
  <div class="empty-state">
    <div class="empty-content">
      <div class="logo-mark">K</div>
      <h1>Korlap</h1>
      <p>Orchestrate parallel Claude agents across git worktrees.</p>
      <button class="open-repo-btn" onclick={handleOpenRepo}>
        Open Repository
      </button>
      {#if repos.length > 0}
        <div class="recent-repos">
          <span class="recent-label">Recent</span>
          {#each repos as repo}
            <button class="recent-item" onclick={() => selectRepo(repo)}>
              {repo.display_name}
              <span class="recent-path">{repo.path}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {#if error}
      <div class="error">{error}</div>
    {/if}
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
      prStatus={selectedWsId ? prStatusMap.get(selectedWsId) : undefined}
      wsChanges={selectedWsId ? changeCounts.get(selectedWsId) : undefined}
      onSelectRepo={selectRepo}
      onAddRepo={handleOpenRepo}
      onSettings={() => (showSettings = true)}
      onPrAction={handlePrAction}
      onReview={handleReview}
      reviewRunning={selectedWsId ? reviewByWorkspace.get(selectedWsId)?.status === "running" : false}
    />

    {#if error}
      <div class="error">
        {error}
        <button class="error-dismiss" onclick={() => (error = "")}>×</button>
      </div>
    {/if}

    <div class="main-layout">
      <Sidebar
        {workspaces}
        {selectedWsId}
        {creatingWsId}
        {prStatusMap}
        {reviewingWsIds}
        onSelect={selectWorkspace}
        onNewWorkspace={handleNewWorkspace}
        onRename={handleRename}
        onRemove={handleRemove}
      />

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
                  queue={(queueByWorkspace.get(ws.id) ?? []).map(q => ({
                    id: q.id,
                    prompt: q.prompt,
                    imageCount: q.images.length,
                    mentionCount: q.mentions.length,
                    planMode: q.planMode,
                  }))}
                  onSend={(prompt, images, mentions, planMode) => handleSend(prompt, images, mentions, planMode)}
                  onSendImmediate={(prompt) => handleSendImmediate(prompt)}
                  onStop={handleStop}
                  onRemoveFromQueue={(id) => { if (ws.id) removeFromQueue(ws.id, id); }}
                  onPlanModeChange={(enabled) => planModeByWorkspace.set(ws.id, enabled)}
                  onThinkingModeChange={(enabled) => thinkingModeByWorkspace.set(ws.id, enabled)}
                  onExecutePlan={() => {
                    planModeByWorkspace.set(ws.id, false);
                    sendPrompt(ws.id, "Execute the plan above. Do not ask for confirmation — just do it.", "Executing plan");
                  }}
                  onMentionClick={(path) => { fileNavigatePath = path; activeTab = "files"; }}
                  onReady={(api) => chatPanelApis.set(ws.id, api)}
                />
                {#if reviewByWorkspace.has(ws.id)}
                  <ReviewPill
                    state={reviewByWorkspace.get(ws.id)!}
                    onCancel={() => {
                      const wasRunning = reviewByWorkspace.get(ws.id)?.status === "running";
                      reviewByWorkspace.delete(ws.id);
                      if (wasRunning) stopAgent(ws.id).catch((e) => { error = String(e); });
                    }}
                    onSendToChat={(markdown) => {
                      reviewByWorkspace.delete(ws.id);
                      sendPrompt(ws.id, `Address all issues from this code review:\n\n${markdown}`, "Addressing review").catch((e) => { error = String(e); });
                      activeTab = "chat";
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
        onOpenInFiles={(path) => {
          showSearchModal = false;
          fileNavigatePath = path;
          activeTab = "files";
        }}
      />
    {/if}

    {#if showSettings}
      <RepoSettingsPanel
        repoId={activeRepo.id}
        repoName={activeRepo.display_name}
        repoPath={activeRepo.path}
        currentProfile={activeRepo.gh_profile ?? null}
        onRemoveRepo={handleRemoveRepo}
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

<style>
  /* ── Empty state ─────────────────────────────────── */

  .empty-state {
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }

  .empty-content {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
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

  .empty-content h1 {
    margin: 0;
    font-size: 1.5rem;
    color: var(--text-bright);
    font-weight: 600;
  }

  .empty-content p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .open-repo-btn {
    margin-top: 0.5rem;
    padding: 0.6rem 1.5rem;
    background: var(--accent);
    color: var(--bg-base);
    border: none;
    border-radius: 6px;
    font-weight: 600;
    font-size: 0.9rem;
    cursor: pointer;
    font-family: inherit;
  }

  .open-repo-btn:hover {
    filter: brightness(1.1);
  }

  .recent-repos {
    margin-top: 1.5rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    width: 100%;
    max-width: 360px;
  }

  .recent-label {
    font-size: 0.75rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.25rem;
  }

  .recent-item {
    width: 100%;
    text-align: left;
    padding: 0.5rem 0.75rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .recent-item:hover {
    border-color: var(--border-light);
    background: var(--bg-hover);
  }

  .recent-path {
    font-size: 0.7rem;
    color: var(--text-dim);
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

  .tab-placeholder {
    flex: 1;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    font-size: 0.85rem;
  }

  /* ── Error ──────────────────────────────────────── */

  .error {
    background: var(--error-bg);
    color: var(--error);
    padding: 0.4rem 0.75rem;
    font-size: 0.8rem;
    white-space: pre-wrap;
    word-break: break-word;
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .error-dismiss {
    background: none;
    border: none;
    color: var(--error);
    cursor: pointer;
    font-size: 1.1rem;
    padding: 0 0.25rem;
  }
</style>

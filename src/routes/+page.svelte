<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { SvelteMap } from "svelte/reactivity";
  import {
    addRepo,
    removeRepo,
    listRepos,
    createWorkspace,
    archiveWorkspace,
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
  import DiffViewer from "$lib/components/DiffViewer.svelte";
  import TerminalView from "$lib/components/Terminal.svelte";
  import RepoSettingsPanel from "$lib/components/RepoSettings.svelte";

  type PanelTab = "chat" | "diff" | "terminal";

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

  let selectedWs = $derived(workspaces.find((w) => w.id === selectedWsId));
  let activeWorkspaces = $derived(
    workspaces
      .filter((w) => w.status !== "archived")
      .sort((a, b) => a.created_at - b.created_at),
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
        const ws = workspaces.find((w) => w.id === event.workspace_id);
        if (ws) {
          ws.status = event.status as WorkspaceInfo["status"];
        }
        if (event.status === "waiting") {
          setSending(event.workspace_id, false);
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
      if (!mod) return;

      const tag = (e.target as HTMLElement)?.tagName;
      const inInput = tag === "INPUT" || tag === "TEXTAREA";

      switch (e.key) {
        case ",":
          e.preventDefault();
          if (activeRepo) showSettings = !showSettings;
          break;
        case "n":
          e.preventDefault();
          handleNewWorkspace();
          break;
        case "w":
          e.preventDefault();
          if (selectedWsId) handleArchive(selectedWsId);
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
      const active = ws.filter((w) => w.status !== "archived");
      active.forEach((w) => loadPersistedMessages(w.id));
      active.forEach((w) => {
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

  function handleArchive(wsId: string) {
    error = "";

    // Optimistic: remove from UI immediately
    const archIdx = workspaces.findIndex((w) => w.id === wsId);
    const removed = archIdx >= 0 ? workspaces[archIdx] : null;
    if (archIdx >= 0) workspaces.splice(archIdx, 1);
    if (selectedWsId === wsId) selectedWsId = null;
    if (creatingWsId === wsId) creatingWsId = null;
    clearWorkspaceData(wsId);
    sendingByWorkspace.delete(wsId);
    prStatusMap.delete(wsId);
    changeCounts.delete(wsId);

    archiveWorkspace(wsId).catch((e) => {
      // Restore on failure
      if (removed) workspaces.push(removed);
      error = String(e);
    });
  }

  async function sendPrompt(wsId: string, prompt: string, actionLabel?: string) {
    if (sendingByWorkspace.get(wsId)) return;
    error = "";
    setSending(wsId, true);

    if (actionLabel) {
      addActionMessage(wsId, crypto.randomUUID(), actionLabel);
    } else {
      addUserMessage(wsId, crypto.randomUUID(), prompt);
    }

    try {
      await sendMessage(wsId, prompt, (event: AgentEvent) => {
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
          // Refresh workspace list to pick up any branch renames from MCP.
          // Merge in-place instead of replacing the array to preserve granular reactivity.
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
                // Remove stale entries (archived externally), but keep the placeholder
                for (let i = workspaces.length - 1; i >= 0; i--) {
                  if (!freshIds.has(workspaces[i].id) && workspaces[i].id !== creatingWsId) {
                    workspaces.splice(i, 1);
                  }
                }
              })
              .catch(() => {});
          }
        } else if (event.type === "error") {
          error = event.message;
          setSending(wsId, false);
        }
      });
    } catch (e) {
      error = String(e);
      setSending(wsId, false);
    }
  }

  async function handleSend(prompt: string, images: PastedImage[] = []) {
    if (!selectedWsId) return;
    const wsId = selectedWsId;

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

    // Build prompt with image references
    let fullPrompt = prompt;
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

    // Add to message store with image paths for display
    if (sendingByWorkspace.get(wsId)) return;
    error = "";
    setSending(wsId, true);
    const dataUrls = images.length > 0 ? images.map((img) => img.dataUrl) : undefined;
    addUserMessage(wsId, crypto.randomUUID(), prompt || "(images attached)", dataUrls);

    try {
      await sendMessage(wsId, fullPrompt, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          const toolUses = event.tool_uses.map((t) => ({
            name: t.name,
            input: t.input_preview ?? "",
            filePath: t.file_path,
          }));
          addAssistantMessage(
            wsId,
            crypto.randomUUID(),
            event.text.trim(),
            toolUses,
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
        } else if (event.type === "error") {
          error = event.message;
          setSending(wsId, false);
        }
      });
    } catch (e) {
      error = String(e);
      setSending(wsId, false);
    }
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
      if (pr.mergeable === "conflicting") {
        const baseBranch = activeRepo.default_branch;
        sendPrompt(wsId, `PR #${pr.number} has merge conflicts with ${baseBranch}.\n\nResolve them:\n1. Run \`git fetch origin ${baseBranch}\`\n2. Run \`git merge origin/${baseBranch}\`\n3. Resolve all conflicts\n4. Commit the merge\n5. Push\n\nIf the conflicts are complex, explain what's conflicting before resolving.`, `Resolving conflicts on PR #${pr.number}`);
      } else if (pr.checks === "failing") {
        sendPrompt(wsId, `PR #${pr.number} has failing checks. Investigate the failures using \`gh pr checks ${pr.number}\`, fix the issues, commit, and push.`, `Fixing checks on PR #${pr.number}`);
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

      let prompt = `Create a pull request.\n\n`;
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
      {repos}
      {activeRepo}
      {selectedWs}
      prStatus={selectedWsId ? prStatusMap.get(selectedWsId) : undefined}
      onSelectRepo={selectRepo}
      onAddRepo={handleOpenRepo}
      onSettings={() => (showSettings = true)}
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
        onSelect={selectWorkspace}
        onNewWorkspace={handleNewWorkspace}
        onRename={handleRename}
      />

      <main class="panel">
        {#if selectedWs}
          <div class="tab-bar">
            <div class="tabs">
              {#each ["chat", "diff", "terminal"] as tab}
                <button
                  class="tab"
                  class:active={activeTab === tab}
                  onclick={() => (activeTab = tab as PanelTab)}
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
            <div class="tab-bar-right">
              {#if selectedWs.status === "running"}
                <span class="status-badge running">Running</span>
              {:else if prStatusMap.get(selectedWs.id)?.state === "open"}
                {#if prStatusMap.get(selectedWs.id)?.mergeable === "conflicting"}
                  <button class="status-badge conflicts" onclick={handlePrAction}>Conflicts</button>
                {:else if prStatusMap.get(selectedWs.id)?.checks === "failing"}
                  <button class="status-badge checks-fail" onclick={handlePrAction}>Fix issues</button>
                {:else if prStatusMap.get(selectedWs.id)?.checks === "pending"}
                  <span class="status-badge checks-pending">PR #{prStatusMap.get(selectedWs.id)?.number} · Checks</span>
                {:else if (prStatusMap.get(selectedWs.id)?.ahead_by ?? 0) > 0}
                  <button class="status-badge push-needed" onclick={handlePrAction}>Push</button>
                {:else}
                  <button class="status-badge mergeable" onclick={handlePrAction}>Merge #{prStatusMap.get(selectedWs.id)?.number}</button>
                {/if}
              {:else if prStatusMap.get(selectedWs.id)?.state === "merged"}
                <span class="status-badge merged">Done</span>
              {:else if selectedWs.status === "waiting"}
                <span class="status-badge waiting">Ready</span>
              {/if}
              <button
                class="archive-btn"
                onclick={() => handleArchive(selectedWs!.id)}
              >
                Archive
              </button>
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
                  disabled={ws.status === "archived"}
                  onSend={handleSend}
                  onStop={handleStop}
                />
              </div>
            {/each}

            <!-- Diff/Terminal: mount on demand, positioned absolute to fill tab-content -->
            {#if activeTab === "diff" && selectedWs}
              <div class="ws-tab-container active-layer">
                <DiffViewer
                  workspaceId={selectedWs.id}
                  refreshTrigger={diffRefreshTrigger}
                  prState={prStatusMap.get(selectedWs.id)?.state}
                  onCreatePr={handlePrAction}
                />
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

  .tab-bar-right {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .status-badge {
    font-size: 0.68rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0.2rem 0.55rem;
    border-radius: 4px;
    border: 1px solid;
  }

  .status-badge.running {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .status-badge.waiting {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 40%, transparent);
  }

  .status-badge.merged {
    color: var(--text-dim);
    border-color: var(--border-light);
  }

  .status-badge.pr-open {
    color: #7e8ec8;
    border-color: color-mix(in srgb, #7e8ec8 40%, transparent);
    text-transform: none;
  }

  .status-badge.checks-pending {
    color: var(--text-dim);
    border-color: var(--border-light);
    text-transform: none;
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .status-badge.checks-fail {
    color: var(--diff-del);
    border-color: color-mix(in srgb, var(--diff-del) 40%, transparent);
    background: color-mix(in srgb, var(--diff-del) 7%, transparent);
    text-transform: none;
  }

  .status-badge.push-needed {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
    cursor: pointer;
    text-transform: none;
  }

  .status-badge.push-needed:hover {
    filter: brightness(1.2);
  }

  .status-badge.mergeable {
    color: var(--status-ok);
    border-color: color-mix(in srgb, var(--status-ok) 40%, transparent);
    background: color-mix(in srgb, var(--status-ok) 7%, transparent);
    cursor: pointer;
    text-transform: none;
  }

  .status-badge.mergeable:hover {
    filter: brightness(1.2);
  }

  .status-badge.create-pr {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: color-mix(in srgb, var(--accent) 7%, transparent);
    cursor: pointer;
    text-transform: none;
  }

  .status-badge.create-pr:hover {
    filter: brightness(1.2);
  }

  .status-badge.checks-fail {
    cursor: pointer;
  }

  .status-badge.conflicts {
    color: #c87e7e;
    border-color: color-mix(in srgb, #c87e7e 40%, transparent);
    background: color-mix(in srgb, #c87e7e 7%, transparent);
    cursor: pointer;
    text-transform: none;
  }

  .status-badge.conflicts:hover {
    filter: brightness(1.2);
  }

  @keyframes badge-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.6; }
  }

  .archive-btn {
    padding: 0.2rem 0.5rem;
    background: transparent;
    border: 1px solid var(--border-light);
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.72rem;
  }

  .archive-btn:hover {
    color: var(--text-primary);
    background: var(--bg-active);
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

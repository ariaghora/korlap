<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    addRepo,
    listRepos,
    createWorkspace,
    archiveWorkspace,
    listWorkspaces,
    sendMessage,
    onAgentStatus,
    type RepoDetail,
    type WorkspaceInfo,
    type AgentEvent,
  } from "$lib/ipc";
  import {
    addUserMessage,
    addAssistantMessage,
    getMessages,
    loadPersistedMessages,
  } from "$lib/stores/messages.svelte";
  import { onMount } from "svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import ChatPanel from "$lib/components/ChatPanel.svelte";

  type PanelTab = "chat" | "diff" | "terminal" | "scripts";

  // ── State ──────────────────────────────────────────────

  let repos = $state<RepoDetail[]>([]);
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeRepo = $state<RepoDetail | null>(null);
  let selectedWsId = $state<string | null>(null);
  let error = $state("");
  let sending = $state(false);
  let activeTab = $state<PanelTab>("chat");

  let selectedWs = $derived(workspaces.find((w) => w.id === selectedWsId));
  let activeWorkspaces = $derived(
    workspaces.filter((w) => w.status !== "archived"),
  );

  // ── Lifecycle ──────────────────────────────────────────

  onMount(() => {
    let unlistenFn: (() => void) | undefined;

    (async () => {
      try {
        repos = await listRepos();
        if (repos.length > 0) {
          await selectRepo(repos[0]);
        }
      } catch (e) {
        error = String(e);
      }

      unlistenFn = await onAgentStatus((event) => {
        const ws = workspaces.find((w) => w.id === event.workspace_id);
        if (ws) {
          ws.status = event.status as WorkspaceInfo["status"];
          workspaces = [...workspaces];
        }
        if (event.workspace_id === selectedWsId) {
          if (event.status === "waiting") {
            sending = false;
          }
        }
      });
    })();

    return () => {
      unlistenFn?.();
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

  async function selectRepo(repo: RepoDetail) {
    activeRepo = repo;
    selectedWsId = null;
    error = "";
    try {
      workspaces = await listWorkspaces(repo.id);
      // Load persisted messages for all non-archived workspaces
      await Promise.all(
        workspaces
          .filter((w) => w.status !== "archived")
          .map((ws) => loadPersistedMessages(ws.id)),
      );
    } catch (e) {
      error = String(e);
    }
  }

  async function handleNewWorkspace() {
    if (!activeRepo) return;
    error = "";
    try {
      const ws = await createWorkspace(activeRepo.id);
      workspaces = [...workspaces, ws];
      selectedWsId = ws.id;
      activeTab = "chat";
    } catch (e) {
      error = String(e);
    }
  }

  async function handleArchive(wsId: string) {
    error = "";
    try {
      await archiveWorkspace(wsId);
      workspaces = workspaces.filter((w) => w.id !== wsId);
      if (selectedWsId === wsId) {
        selectedWsId = null;
      }
    } catch (e) {
      error = String(e);
    }
  }

  async function handleSend(prompt: string) {
    if (!selectedWsId || sending) return;
    const wsId = selectedWsId;
    error = "";
    sending = true;

    addUserMessage(wsId, crypto.randomUUID(), prompt);

    try {
      await sendMessage(wsId, prompt, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          const toolUses = event.tool_uses.map((t) => ({
            name: t.name,
            input: t.input_preview ?? "",
          }));
          addAssistantMessage(
            wsId,
            crypto.randomUUID(),
            event.text.trim(),
            toolUses,
          );
        } else if (event.type === "done") {
          sending = false;
        } else if (event.type === "error") {
          error = event.message;
          sending = false;
        }
      });
    } catch (e) {
      error = String(e);
      sending = false;
    }
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
      onSelectRepo={selectRepo}
      onAddRepo={handleOpenRepo}
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
        onSelect={(wsId) => (selectedWsId = wsId)}
        onNewWorkspace={handleNewWorkspace}
      />

      <main class="panel">
        {#if selectedWs}
          <!-- Tab bar + status badge -->
          <div class="tab-bar">
            <div class="tabs">
              {#each ["chat", "diff", "terminal", "scripts"] as tab}
                <button
                  class="tab"
                  class:active={activeTab === tab}
                  onclick={() => (activeTab = tab as PanelTab)}
                >
                  {tab.charAt(0).toUpperCase() + tab.slice(1)}
                </button>
              {/each}
            </div>
            <div class="tab-bar-right">
              {#if selectedWs.status === "running"}
                <span class="status-badge running">Running</span>
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

          <!-- Tab content: display:none switching preserves scroll + state -->
          <div class="tab-content">
            <!-- Chat panels — one per workspace, toggled via display -->
            {#each activeWorkspaces as ws (ws.id)}
              <div
                class="ws-chat-container"
                style:display={activeTab === "chat" && ws.id === selectedWsId ? "flex" : "none"}
              >
                <ChatPanel
                  messages={getMessages(ws.id)}
                  sending={sending && ws.id === selectedWsId}
                  disabled={ws.status === "archived"}
                  onSend={handleSend}
                />
              </div>
            {/each}

            <!-- Placeholder tabs -->
            <div class="tab-placeholder" style:display={activeTab === "diff" ? "flex" : "none"}>
              <p>Diff viewer — coming in M3</p>
            </div>
            <div class="tab-placeholder" style:display={activeTab === "terminal" ? "flex" : "none"}>
              <p>Terminal — coming in M3</p>
            </div>
            <div class="tab-placeholder" style:display={activeTab === "scripts" ? "flex" : "none"}>
              <p>Scripts runner — coming in M3</p>
            </div>
          </div>
        {:else}
          <div class="panel-empty">
            <p>Create a workspace to start an agent.</p>
          </div>
        {/if}
      </main>
    </div>

  </div>
{/if}

<style>
  :global(body) {
    margin: 0;
    background: #13110e;
    color: #d4c5a9;
    font-family: "Space Grotesk", system-ui, sans-serif;
    font-size: 14px;
  }

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
    background: #2a2520;
    border: 1px solid #3a3530;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    font-weight: 700;
    color: #c8a97e;
  }

  .empty-content h1 {
    margin: 0;
    font-size: 1.5rem;
    color: #e8dcc8;
    font-weight: 600;
  }

  .empty-content p {
    margin: 0;
    color: #8a7e6a;
    font-size: 0.85rem;
  }

  .open-repo-btn {
    margin-top: 0.5rem;
    padding: 0.6rem 1.5rem;
    background: #c8a97e;
    color: #13110e;
    border: none;
    border-radius: 6px;
    font-weight: 600;
    font-size: 0.9rem;
    cursor: pointer;
    font-family: inherit;
  }

  .open-repo-btn:hover {
    background: #d4b88a;
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
    color: #6a6050;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.25rem;
  }

  .recent-item {
    width: 100%;
    text-align: left;
    padding: 0.5rem 0.75rem;
    background: #1a1714;
    border: 1px solid #2a2520;
    border-radius: 6px;
    color: #d4c5a9;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .recent-item:hover {
    border-color: #3a3530;
    background: #1e1b17;
  }

  .recent-path {
    font-size: 0.7rem;
    color: #6a6050;
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
    color: #6a6050;
    font-size: 0.85rem;
  }

  /* ── Tab bar ───────────────────────────────────── */

  .tab-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 1rem;
    height: 38px;
    border-bottom: 1px solid #2a2520;
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
    color: #6a6050;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    font-weight: 500;
  }

  .tab:hover {
    color: #d4c5a9;
    background: #1e1b17;
  }

  .tab.active {
    color: #e8dcc8;
    background: #2a2520;
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
    color: #c8a97e;
    border-color: #c8a97e66;
    background: #c8a97e11;
    animation: badge-pulse 2s ease-in-out infinite;
  }

  .status-badge.waiting {
    color: #7e9e6b;
    border: 1px solid #7e9e6b44;
  }

  @keyframes badge-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.6;
    }
  }

  .archive-btn {
    padding: 0.2rem 0.5rem;
    background: transparent;
    border: 1px solid #3a3530;
    border-radius: 4px;
    color: #6a6050;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.72rem;
  }

  .archive-btn:hover {
    color: #d4c5a9;
    background: #2a2520;
  }

  /* ── Tab content ──────────────────────────────────── */

  .tab-content {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .ws-chat-container {
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .tab-placeholder {
    flex: 1;
    align-items: center;
    justify-content: center;
    color: #4a4540;
    font-size: 0.85rem;
  }

  /* ── Error ──────────────────────────────────────── */

  .error {
    background: #3a1a1a;
    color: #e88;
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
    color: #e88;
    cursor: pointer;
    font-size: 1.1rem;
    padding: 0 0.25rem;
  }
</style>

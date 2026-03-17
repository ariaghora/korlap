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
    type ToolUseInfo,
  } from "$lib/ipc";
  import { onMount } from "svelte";

  // ── Types ──────────────────────────────────────────────

  interface ChatMessage {
    id: string;
    role: "user" | "assistant";
    text: string;
    toolUses?: ToolUseInfo[];
  }

  // ── State ──────────────────────────────────────────────

  let repos = $state<RepoDetail[]>([]);
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeRepo = $state<RepoDetail | null>(null);
  let selectedWsId = $state<string | null>(null);
  let error = $state("");
  let userInput = $state("");
  let sending = $state(false);

  // Messages per workspace
  let messagesByWs = $state(new Map<string, ChatMessage[]>());

  let selectedWs = $derived(workspaces.find((w) => w.id === selectedWsId));
  let activeWorkspaces = $derived(
    workspaces.filter((w) => w.status !== "archived"),
  );
  let currentMessages = $derived(
    selectedWsId ? messagesByWs.get(selectedWsId) ?? [] : [],
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

  function addMessage(wsId: string, msg: ChatMessage) {
    const existing = messagesByWs.get(wsId) ?? [];
    const updated = new Map(messagesByWs);
    updated.set(wsId, [...existing, msg]);
    messagesByWs = updated;
  }

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
    } catch (e) {
      error = String(e);
    }
  }

  function handleSelectWorkspace(wsId: string) {
    selectedWsId = wsId;
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

  async function handleSend() {
    if (!selectedWsId || !userInput.trim() || sending) return;
    const wsId = selectedWsId;
    const prompt = userInput.trim();
    userInput = "";
    error = "";
    sending = true;

    // Add user message
    addMessage(wsId, {
      id: crypto.randomUUID(),
      role: "user",
      text: prompt,
    });

    try {
      await sendMessage(wsId, prompt, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          addMessage(wsId, {
            id: crypto.randomUUID(),
            role: "assistant",
            text: event.text.trim(),
            toolUses: event.tool_uses.length > 0 ? event.tool_uses : undefined,
          });
        } else if (event.type === "error") {
          error = event.message;
        }
        // "done" is handled by agent-status event → sets sending = false
      });
    } catch (e) {
      error = String(e);
      sending = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function formatToolUse(tool: ToolUseInfo): string {
    if (tool.input_preview) {
      return `${tool.name}: ${tool.input_preview}`;
    }
    return tool.name;
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
    <header class="titlebar" data-tauri-drag-region>
      <div class="repo-tabs">
        {#each repos as repo}
          <button
            class="repo-tab"
            class:active={repo.id === activeRepo.id}
            onclick={() => selectRepo(repo)}
          >
            {repo.display_name}
          </button>
        {/each}
        <button class="repo-tab add-tab" onclick={handleOpenRepo}>+</button>
      </div>
      <div class="branch-info">
        {activeRepo.default_branch}
      </div>
    </header>

    {#if error}
      <div class="error">
        {error}
        <button class="error-dismiss" onclick={() => (error = "")}>×</button>
      </div>
    {/if}

    <div class="main-layout">
      <aside class="sidebar">
        <div class="sidebar-header">
          <span class="sidebar-label">Workspaces</span>
        </div>
        <div class="workspace-list">
          {#each activeWorkspaces as ws}
            <button
              class="ws-item"
              class:active={ws.id === selectedWsId}
              onclick={() => handleSelectWorkspace(ws.id)}
            >
              <span
                class="ws-dot"
                class:running={ws.status === "running"}
                class:waiting={ws.status === "waiting"}
              ></span>
              <span class="ws-name">{ws.name}</span>
              {#if ws.status === "running"}
                <span class="ws-status">running</span>
              {/if}
            </button>
          {/each}
        </div>
        <button class="new-ws-btn" onclick={handleNewWorkspace}>
          + New workspace
        </button>
      </aside>

      <main class="panel">
        {#if selectedWs}
          <div class="panel-header">
            <div class="panel-title">
              <strong>{selectedWs.name}</strong>
              <span class="panel-branch">{selectedWs.branch}</span>
            </div>
            <div class="panel-actions">
              <button
                class="action-btn archive-btn"
                onclick={() => handleArchive(selectedWs!.id)}
              >
                Archive
              </button>
            </div>
          </div>

          <div class="chat-area">
            {#if currentMessages.length === 0}
              <div class="chat-empty">
                <p>Send a message to start the agent.</p>
              </div>
            {:else}
              {#each currentMessages as msg}
                <div class="chat-msg" class:user={msg.role === "user"}>
                  {#if msg.role === "assistant"}
                    <div class="msg-label">Claude</div>
                  {/if}
                  {#if msg.text}
                    <div class="msg-text">{msg.text}</div>
                  {/if}
                  {#if msg.toolUses && msg.toolUses.length > 0}
                    <div class="tool-uses">
                      {#each msg.toolUses as tool}
                        <span class="tool-tag">{formatToolUse(tool)}</span>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/each}
              {#if sending}
                <div class="chat-msg">
                  <div class="msg-label">Claude</div>
                  <div class="msg-thinking">Thinking...</div>
                </div>
              {/if}
            {/if}
          </div>

          <form
            class="input-row"
            onsubmit={(e) => {
              e.preventDefault();
              handleSend();
            }}
          >
            <input
              bind:value={userInput}
              onkeydown={handleKeydown}
              placeholder="Ask to make changes, @mention files, run /commands"
              disabled={selectedWs.status === "archived"}
            />
            <button type="submit" class="send-btn" disabled={sending || !userInput.trim()}
              >Send</button
            >
          </form>
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

  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem 0.75rem;
    border-bottom: 1px solid #2a2520;
    background: #1a1714;
    -webkit-user-select: none;
    user-select: none;
  }

  .repo-tabs {
    display: flex;
    gap: 0.25rem;
    align-items: center;
  }

  .repo-tab {
    padding: 0.3rem 0.6rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .repo-tab:hover {
    color: #d4c5a9;
    background: #2a2520;
  }

  .repo-tab.active {
    color: #e8dcc8;
    background: #2a2520;
    border-color: #3a3530;
  }

  .add-tab {
    font-size: 1rem;
    padding: 0.2rem 0.5rem;
    color: #6a6050;
  }

  .branch-info {
    font-size: 0.75rem;
    color: #6a6050;
  }

  .main-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ── Sidebar ─────────────────────────────────────── */

  .sidebar {
    width: 220px;
    border-right: 1px solid #2a2520;
    display: flex;
    flex-direction: column;
    background: #16140f;
  }

  .sidebar-header {
    padding: 0.6rem 0.75rem 0.3rem;
  }

  .sidebar-label {
    font-size: 0.7rem;
    color: #6a6050;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .workspace-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem;
  }

  .ws-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.45rem 0.5rem;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: #d4c5a9;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    text-align: left;
  }

  .ws-item:hover {
    background: #1e1b17;
  }

  .ws-item.active {
    background: #2a2520;
    border-color: #3a3530;
  }

  .ws-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    background: #3a3530;
  }

  .ws-dot.running {
    background: #c8a97e;
    box-shadow: 0 0 6px #c8a97e88;
    animation: pulse 2s ease-in-out infinite;
  }

  .ws-dot.waiting {
    background: #7e9e6b;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  .ws-status {
    font-size: 0.65rem;
    color: #c8a97e;
    margin-left: auto;
  }

  .ws-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .new-ws-btn {
    margin: 0.5rem;
    padding: 0.4rem;
    background: transparent;
    border: 1px dashed #3a3530;
    border-radius: 4px;
    color: #6a6050;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .new-ws-btn:hover {
    color: #c8a97e;
    border-color: #c8a97e;
    background: #1e1b17;
  }

  /* ── Main panel ──────────────────────────────────── */

  .panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .panel-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #2a2520;
  }

  .panel-title strong {
    color: #e8dcc8;
    font-size: 0.9rem;
  }

  .panel-branch {
    margin-left: 0.5rem;
    font-size: 0.75rem;
    color: #6a6050;
  }

  .panel-actions {
    display: flex;
    gap: 0.35rem;
  }

  .action-btn {
    padding: 0.25rem 0.6rem;
    background: transparent;
    border: 1px solid #3a3530;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.78rem;
  }

  .action-btn:hover {
    color: #d4c5a9;
    background: #2a2520;
  }

  /* ── Chat area ──────────────────────────────────── */

  .chat-area {
    flex: 1;
    overflow-y: auto;
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .chat-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6a6050;
    font-size: 0.85rem;
  }

  .chat-msg {
    max-width: 85%;
  }

  .chat-msg.user {
    align-self: flex-end;
  }

  .chat-msg.user .msg-text {
    background: #2a2520;
    border: 1px solid #3a3530;
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    color: #e8dcc8;
  }

  .msg-label {
    font-size: 0.7rem;
    color: #6a6050;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin-bottom: 0.25rem;
  }

  .msg-text {
    font-size: 0.85rem;
    line-height: 1.5;
    color: #d4c5a9;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .msg-thinking {
    font-size: 0.85rem;
    color: #c8a97e;
    animation: pulse 2s ease-in-out infinite;
  }

  .tool-uses {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin-top: 0.4rem;
  }

  .tool-tag {
    display: inline-block;
    padding: 0.2rem 0.5rem;
    background: #1e1b17;
    border: 1px solid #2e2a24;
    border-radius: 4px;
    font-size: 0.72rem;
    color: #8a7e6a;
    font-family: "SF Mono", "Fira Code", monospace;
  }

  /* ── Input row ──────────────────────────────────── */

  .input-row {
    display: flex;
    gap: 0.5rem;
    padding: 0.5rem 0.75rem;
    border-top: 1px solid #2a2520;
  }

  .input-row input {
    flex: 1;
    background: #1e1b17;
    border: 1px solid #2e2a24;
    color: #d4c5a9;
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .input-row input:focus {
    outline: none;
    border-color: #c8a97e;
  }

  .input-row input:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .send-btn {
    padding: 0.5rem 1rem;
    background: #2a2520;
    border: 1px solid #3a3530;
    color: #d4c5a9;
    border-radius: 6px;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.85rem;
  }

  .send-btn:hover:not(:disabled) {
    background: #3a3530;
  }

  .send-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .panel-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #6a6050;
    font-size: 0.85rem;
  }

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

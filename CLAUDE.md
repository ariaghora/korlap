# Korlap — Claude Code Instructions

Tauri v2 + Svelte 5 + Bun desktop app for orchestrating parallel Claude Code agents across git worktrees. Each workspace = one git worktree on an isolated branch. Full design spec in `DESIGN.md`.

---

## Hard rules

**Rust**
- Every `#[tauri::command]` returns `Result<T, String>` — never panic, never unwrap in command handlers
- No `unwrap()` or `expect()` outside of tests
- All shared state through `Mutex<T>` in Tauri managed state — no globals, no lazy_static
- PTY reader threads handle EOF/errors gracefully, emit `agent-status` event on exit
- `portable-pty`: always close slave end in parent after spawning child
- Spawn `claude` with explicit env — inject `GH_TOKEN` per-process, never rely on ambient shell
- Never call `gh auth switch` globally — use `gh auth token --user <profile>` and inject per-process
- Git operations return descriptive errors, not generic ones

**Frontend**
- PTY output never touches Svelte state — xterm.js owns its buffer
- Messages use `SvelteMap<id, Message>`, mutated in place — never replace entire arrays
- xterm instances use `display: none/block` on workspace switch — never mount/unmount
- Tauri Channel API for binary streams — never `listen()` + JSON for high-frequency data
- All `invoke()` calls wrapped in try/catch with user-visible error handling

**Data**
- All app data under `~/Library/Application Support/net.ghora.korlap/` — zero files in managed repos
- Worktrees: `<data_dir>/workspaces/<workspace-id>/`
- Messages: `<data_dir>/messages/<workspace-id>.json`
- Metadata: `<data_dir>/workspaces.json`, `<data_dir>/sessions.json`, `<data_dir>/repos.json`
- Never operate on the main worktree's working directory

**Commands**
- Use `bun`, not `npm`, `npx`, or `yarn`
- Type check: `bun run check`
- Rust check: `cargo check` (never `cargo build` or `tauri build`)

**General**
- No `console.log` in production paths — `tracing` in Rust
- No hardcoded paths — derive from repo root or Tauri app data dir
- Async filesystem/process ops must have a timeout

---

## Architecture

```
src-tauri/src/
  main.rs / lib.rs       — Tauri setup, state init, command registration
  state.rs               — AppState, RepoInfo, WorkspaceInfo, WorkspaceStatus
  mcp_api.rs             — MCP server (random port)
  traffic.rs             — macOS traffic light positioning
  commands/
    mod.rs               — module exports
    repo.rs              — add_repo, remove_repo, list_repos
    workspace.rs         — create/remove/list/rename_branch
    agent.rs             — send_message, stop_agent, generate_commit_message, suggest_replies
    git.rs               — diff, commit, push, branch sync, base updates
    github.rs            — gh profiles, PR status/merge, auth, repo listing/cloning
    files.rs             — list/read/write/search/grep (workspace & repo level)
    terminal.rs          — open/write/resize/close terminal (portable-pty)
    scripts.rs           — run_script (zsh in worktree dir)
    persistence.rs       — save/load messages, todos, images, repo settings
    helpers.rs           — git auth, default branch detection, shell env

src/
  lib/
    stores/
      messages.svelte.ts — SvelteMap<wsId, SvelteMap<msgId, Message>>, sendingByWorkspace
      toasts.svelte.ts   — toast notifications
      theme.svelte.ts    — dark/light theme state
    ipc.ts               — typed invoke() wrappers + event listeners
    chat-utils.ts        — message rendering, mention parsing
    markdown.ts          — markdown to HTML
    actions.ts           — Svelte actions (external links, code copy)
    pr-status.ts         — PR status cache
    themes.ts            — color palette definitions
  components/
    TitleBar.svelte      — repo tabs, plan/work mode, main branch sync
    Sidebar.svelte       — workspace list with status dots
    WorkspacePanel.svelte — tab bar (Chat/Diff/Terminal/Scripts/Files)
    ChatPanel.svelte     — message list + input + agent streaming
    Terminal.svelte      — xterm.js terminal
    DiffViewer.svelte    — syntax-highlighted diffs
    CodeEditor.svelte    — CodeMirror 6 editor
    FileBrowser.svelte   — file tree navigation
    SearchModal.svelte   — file + content search
    KanbanBoard.svelte   — task board (draggable)
    MentionInput.svelte  — rich input with @-file mentions
    RepoSettings.svelte  — per-repo config (scripts, prompts, templates)
    ... and more (see src/components/)
  routes/
    +page.svelte         — main app shell, state management, event listeners
```

---

## Key patterns

- **Agent output (structured chat):** `claude -p --output-format stream-json --verbose` → NDJSON parsed in Rust → streamed to frontend via Tauri Channel as `AgentEvent`s → rendered in ChatPanel.
- **Terminal (raw PTY):** `portable-pty` → Rust reader thread → Tauri Channel (binary) → `xterm.js`. Nothing in this pipeline touches Svelte state.
- **Persistence:** Messages debounced at 500ms, fire-and-forget. Workspace/repo metadata saved on mutation.
- **GH auth:** Token read via `gh auth token --user <profile>`, injected as env var per spawned process. Never mutate global gh config.

---

## Visual identity

See `design.md` for color tokens. Colors are defined via a dynamic theme system in `lib/themes.ts` — never hardcode hex values, always use CSS custom properties / theme tokens. Typeface: Space Grotesk.

---

## What not to build (unless explicitly instructed)
- Codex support
- Checkpoint/restore of Claude conversation history
- MCP config UI
- Multi-repo open simultaneously
- Windows support

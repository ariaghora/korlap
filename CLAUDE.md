# Korlap — Claude Code Instructions

## What this is
Korlap is a Tauri v2 + Svelte 5 + Bun desktop app for orchestrating parallel Claude Code agents across git worktrees. Each workspace = one git worktree on an isolated branch. Full design spec is in `DESIGN.md`.

---

## Non-negotiable rules

### Never break these, regardless of milestone or scope

**Rust**
- Every `#[tauri::command]` returns `Result<T, String>` — never panic, never unwrap in command handlers
- All shared state goes through `Mutex<T>` inside Tauri's managed state — no globals, no lazy_static
- PTY reader threads must handle EOF and errors gracefully and emit a terminal `agent-status` event on exit
- Spawn `claude` with explicit env isolation — always inject `GH_TOKEN` from the repo's bound profile, never rely on ambient shell env
- Never call `gh auth switch` globally — it mutates shared config and breaks parallel agents. Read token via `gh auth token --user <profile>` and inject per-process
- All git operations that can fail (worktree add/remove, diff) must return descriptive errors, not generic ones
- `portable-pty` PTY pair: always close the slave end in the parent process after spawning the child, or reads on the master will never see EOF

**Frontend**
- Never store PTY output in Svelte state — xterm.js owns its buffer, period
- Never use a reactive `$state<Message[]>` array that gets replaced on each chunk — use `Map<id, Message>` and mutate individual entries
- The message list must be virtualized — no exceptions, even at low message counts
- xterm instances are never destroyed on workspace switch — use `display: none` / `block`, not mount/unmount
- Tauri Channel API for PTY binary streams — never `listen()` + JSON for high-frequency byte data
- All Tauri `invoke()` calls must be wrapped in try/catch with user-visible error handling — silent failures are not acceptable

**Git**
- Never operate on the main worktree's working directory — all writes go into the workspace's worktree path
- Worktree paths live under `.korlap/worktrees/<workspace-id>/` relative to the repo root
- Workspace metadata (id, branch, worktree path, repo id, gh profile, status, timestamps) is persisted to `.korlap/workspaces.json` and kept in sync on every state change — app restart must restore full state

**General**
- No `unwrap()` or `expect()` outside of tests
- No `console.log` left in production paths — use a proper log level system (`tracing` in Rust, a thin wrapper in TS)
- No hardcoded paths — always derive from repo root or app data dir via Tauri APIs
- Every async operation that touches the filesystem or spawns a process must have a timeout

---

## Architecture

See `DESIGN.md` for full detail. Summary:

```
src-tauri/src/
  main.rs          — Tauri setup, managed state registration
  workspace.rs     — worktree lifecycle (create, archive, restore)
  agent.rs         — claude CLI spawn, PTY management, output streaming
  git.rs           — diff, branch ops via git2
  commands.rs      — all #[tauri::command] handlers

src/
  lib/
    stores/
      repos.svelte.ts        — Map<id, Repository>, activeRepoId
      workspaces.svelte.ts   — Map<id, Workspace>, activeWorkspaceId, visibleWorkspaces derived
      messages.svelte.ts     — Map<workspaceId, Map<msgId, Message>>
    ipc.ts                   — typed invoke() wrappers + Channel setup
  components/
    TitleBar.svelte
    Sidebar.svelte
    WorkspacePanel.svelte
    Terminal.svelte          — xterm.js, never touches Svelte state
    ChatPanel.svelte         — virtualized message list
    DiffViewer.svelte
    ScriptRunner.svelte
```

---

## State conventions

```ts
// Repos and workspaces: always Maps, never arrays
const repos = $state(new Map<string, Repository>())
const workspaces = $state(new Map<string, Workspace>())

// Messages: nested Maps, mutate in place
const messagesByWorkspace = $state(new Map<string, Map<string, Message>>())

// Derived — never duplicated state
const visibleWorkspaces = $derived(
  activeRepoId
    ? [...workspaces.values()].filter(w => w.repoId === activeRepoId)
    : []
)
```

---

## PTY streaming pipeline

```
claude process (slave PTY)
  → Rust reader thread
    → accumulate bytes, flush every 16ms max (~60fps)
      → Tauri Channel (ArrayBuffer, not JSON)
        → xterm.js terminal.write(data)
```

Nothing in this pipeline touches Svelte state. If you find yourself writing `messages.update(...)` in a PTY handler, stop — you're in the wrong path.

---

## IPC surface (Tauri commands)

All commands must:
- Return `Result<T, String>` with human-readable error strings
- Be idempotent where possible (e.g. `archive_workspace` on an already-archived workspace is a no-op, not an error)
- Emit a corresponding Tauri event on state changes so the frontend stays in sync without polling

```
Repository:   add_repo, remove_repo, list_repos, set_active_repo, update_repo_profile, list_gh_profiles
Workspace:    create_workspace, archive_workspace, restore_workspace, list_workspaces
Agent:        spawn_agent, kill_agent, write_to_agent, resize_pty, open_pty_stream
Git:          get_diff, get_branches
Scripts:      run_script

Events emitted:
  agent-output   { workspace_id: string, data: ArrayBuffer }
  agent-status   { workspace_id: string, status: WorkspaceStatus }
  script-output  { workspace_id: string, data: string }
```

---

## Visual identity

- Background: `#13110e` — warm dark, amber-tinted, not cold gray
- Single accent: `#c8a97e` — used for running state, active branch, interactive focus
- Typeface: Space Grotesk (already imported via Google Fonts)
- No app name in the title bar
- Status colors: running = `#c8a97e` (pulsing), waiting = `#7e9e6b`, archived = `#2a2420`
- Agent activity bar: 4px segmented bar at bottom of main panel, one segment per workspace, click navigates to that workspace

Full token reference and component mockup in `DESIGN.md`.

---

## Milestones — build in order, do not skip ahead

### M1 — Core plumbing
- `add_repo` command: accept a local path, validate it's a git repo, persist to workspaces.json
- `create_workspace`: `git worktree add -b conductor/<name> .korlap/worktrees/<id> <base>`, persist metadata
- `spawn_agent`: open PTY, exec `claude` in worktree path with injected `GH_TOKEN`, stream output via Tauri Channel to a dumb `<pre>` tag
- `archive_workspace`: kill agent if running, `git worktree remove --force`, mark archived in state
- `list_workspaces`: restore state from workspaces.json on app start
- No real UI — bare functional skeleton only

### M2 — Real UI
- Sidebar workspace list with status dots (pulsing amber / olive / dim)
- Custom title bar: traffic lights, repo tabs with gh profile pills, breadcrumb, avatar
- xterm.js Terminal tab with full PTY input/output
- Workspace switching via `display:none` — no teardown
- Agent activity bar at bottom

### M3 — Diff + Scripts
- Diff tab: `git diff <base>..<branch>` rendered with syntax highlighting (additions green, deletions red, warm palette)
- Scripts tab: run arbitrary shell commands in worktree dir, stream output
- resize_pty wired to ResizeObserver on xterm container

### M4 — Polish
- Workspace state survives app restart (workspaces.json fully round-trips)
- Archive/restore UI
- Keyboard shortcuts: ⌘N new workspace, ⌘W archive, ⌘1–9 switch workspace
- Error states: repo not found, git op failed, agent crashed — all surface to UI

---

## What not to build (ever, unless explicitly instructed)
- PR creation via GitHub API
- Codex support
- Checkpoint/restore of Claude conversation history
- MCP config UI
- Multi-repo open simultaneously (one active repo at a time)
- Windows support

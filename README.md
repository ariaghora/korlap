# Korlap

A macOS app for running multiple [Claude Code](https://docs.anthropic.com/en/docs/claude-code) agents in parallel, each isolated in its own `git worktree`.

Built with Tauri v2, Svelte 5, and Rust.

## What it does

Korlap puts the developer in the role of orchestrator. You define tasks, spawn agents explicitly, and review their output. Nothing runs without your intent.

The interface has two modes. **Plan** is a kanban board where each card is a task for an AI agent. When a task moves to "In Progress," Korlap creates a workspace: a git worktree on its own branch with a dedicated Claude Code agent. **Work** is where you interact with individual agents, read their diffs, run scripts, and review changes before they merge.

The core assumption: the kanban board is for AI agents, and each task maps to an isolated workspace.

### Features

- **Task board.** Four-column kanban (Todo, In Progress, Review, Done) that drives agent lifecycle.
- **Workspace isolation.** Each agent gets a full worktree copy of the repo on a dedicated branch.
- **Structured chat.** Agent output parsed from `--output-format stream-json`, rendered as a rich message list.
- **Diff viewer.** See what each agent changed against the base branch, with syntax highlighting.
- **Review flow.** Opus-powered evaluation of diffs before merging.
- **Script runner.** Run shell commands inside any worktree without leaving the app.
- **`gh` profile support.** Bind a GitHub auth profile per repo so tokens stay scoped to the right org.

The name comes from Indonesian *koordinator lapangan* (field coordinator). The person who orchestrates parallel operations on the ground.

## Prerequisites

- **macOS** only
- [Bun](https://bun.sh/) (package manager + runtime)
- [Rust](https://rustup.rs/) (stable toolchain)
- [Claude Code CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [GitHub CLI](https://cli.github.com/) (`gh`) if you want profile switching

## Getting started

```bash
# Install frontend dependencies
bun install

# Run in development mode (starts both Vite dev server and Tauri)
bun tauri dev
```

That's it. The app opens, you add a repo via the folder picker, and create your first workspace.

## Project structure

```
src-tauri/src/
  main.rs            Tauri setup, managed state registration
  workspace.rs       Worktree lifecycle (create, archive, restore)
  agent.rs           Claude CLI spawn, PTY management, output streaming
  git.rs             Diff and branch ops
  commands.rs        All #[tauri::command] handlers

src/
  lib/
    stores/          Svelte 5 rune stores (repos, workspaces, messages)
    ipc.ts           Typed invoke() wrappers + Channel setup
  components/
    TitleBar.svelte
    Sidebar.svelte
    ChatPanel.svelte
    DiffViewer.svelte
    Terminal.svelte
    ScriptRunner.svelte
```

## Tech stack

| Layer | Choice | Why |
|-------|--------|-----|
| Shell | Tauri v2 | Rust backend with native process and PTY control |
| UI | Svelte 5 | Runes reactivity, minimal runtime cost |
| Runtime | Bun | Fast installs, built-in TypeScript |
| Styling | Tailwind v4 | Zero config |
| Terminal | xterm.js | Standard terminal emulator for the raw PTY tab |

## Architecture notes

**Agent output pipeline.** Claude runs as a subprocess with `--output-format stream-json`. A Rust reader thread accumulates bytes and flushes at ~60fps via Tauri's Channel API as raw `ArrayBuffer`, bypassing JSON serialization on the hot path.

**Workspace switching is O(1).** Each workspace's panel stays in the DOM with `display: none`. Switching toggles visibility. Panels persist across switches, preserving scroll position and state.

**State lives in Maps.** Messages use `Map<workspaceId, Map<msgId, Message>>` so updating one message triggers one reactive cell. Only the changed message re-renders.

**All app data** goes into `~/Library/Application Support/net.ghora.korlap/`. Managed repos stay clean.

## Design

See [`DESIGN.md`](./DESIGN.md) for the full spec: color tokens, component mockups, data model, IPC surface, and milestone breakdown.

## License

MIT

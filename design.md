# Korlap — Design Doc

## Stack: Tauri v2 + Svelte 5 + Bun

---

## Identity

**Name:** Korlap (from Indonesian "koordinator lapangan" — field coordinator, the person who orchestrates parallel operations on the ground)

**Design direction:** Warm dark. All blacks tinted amber, not cold gray. Multiple theme palettes defined in `src/lib/themes.ts` — never hardcode hex values, always use CSS custom properties (e.g. `var(--bg-base)`, `var(--accent)`).

**Token names** (used across all palettes): `bg-sidebar`, `bg-base`, `bg-titlebar`, `bg-card`, `bg-hover`, `bg-active`, `border`, `border-light`, `text-muted`, `text-dim`, `text-secondary`, `text-primary`, `text-bright`, `accent`, `status-ok`, `diff-add`, `diff-add-bg`, `diff-del`, `diff-del-bg`, `error`, `error-bg`.

**Typeface:** Space Grotesk. Geometric, purposeful, slightly idiosyncratic. Not Inter.

**No app name in the title bar.** Korlap identifies itself through aesthetic, not a label.

---

## What We're Building

A desktop app that orchestrates multiple Claude Code agents in parallel, each isolated in a `git worktree`. GUI lets you monitor agents, view diffs, run scripts, and switch between workspaces.

Repositories are first-class entities. Workspaces are children of a repository. Each repository carries a bound `gh` auth profile, so switching between orgs/companies is explicit and never bleeds across repos.

Core primitives:
- `git worktree` for workspace isolation
- `claude` CLI as agent subprocess — structured chat via `--output-format stream-json`
- `gh auth token --user <profile>` for per-process token injection (never `gh auth switch`)
- Per-workspace state, diff viewing, terminal, file browsing, and task management

---

## Stack Rationale

| Layer | Choice | Why |
|-------|--------|-----|
| Shell | Tauri v2 | Rust backend = native process/PTY control; no Electron bloat |
| UI | Svelte 5 | Runes-based reactivity is genuinely good; minimal overhead |
| Runtime | Bun | Fast installs, built-in TS, test runner |
| Styling | Tailwind v4 | Zero config, good DX |
| Terminal | xterm.js | De-facto standard; works well in Tauri WebView |
| Code editor | CodeMirror 6 | Multi-language syntax highlighting, used for file editing |

---

## Performance Principles

1. **Chat messages use `SvelteMap<id, SvelteMap<id, Message>>`.** Updating one message = one reactive cell, not the whole list. Never use arrays for collections that update frequently.
2. **Keyed `{#each}` blocks everywhere.** Svelte diffs by key — without keys it re-renders the entire list.
3. **Persistence is debounced.** Messages saved to disk every 500ms, not on every event. Fire-and-forget — never block the UI thread.
4. **Workspace switch is O(1).** Toggle `display:none` on panels. No data fetching, no re-initialization. Each workspace's xterm/chat stays alive in the DOM.
5. **All app data in Tauri's app data dir.** Zero writes to the managed repo.
6. **PTY binary streams via Tauri Channel API.** No JSON serialization on the hot path.

---

## App Layout

Two top-level modes toggled via title bar (⌘1 / ⌘2):

**Plan mode** — kanban board for task management:
```
┌──────────────────────────────────────────────────────────┐
│  repo ⌘E  ⚙    [Plan ⌘1] [Work ⌘2]                     │
├──────────────────────────────────────────────────────────┤
│  TODO        IN PROGRESS      REVIEW        DONE        │
│ ┌────────┐  ┌────────────┐  ┌──────────┐  ┌──────────┐ │
│ │        │  │ task title  │  │          │  │ done task │ │
│ │        │  │ description │  │          │  │ +N -N     │ │
│ │        │  │ branch +N-N │  │          │  │           │ │
│ └────────┘  └────────────┘  └──────────┘  └──────────┘ │
│                    [+ New task]                          │
└──────────────────────────────────────────────────────────┘
```

**Work mode** — workspace chat, diff, files, terminal:
```
┌──────────────────────────────────────────────────────────┐
│  repo ⌘E  ⚙    [Plan ⌘1] [Work ⌘2]   branch › main     │
├──────────┬───────────────────────────────────────────────┤
│          │  Chat  Diff  Files  Terminal  ▶Run   🔍 PR    │
│ ● auth   │  ┌─────────────────────────────────────────┐  │
│   fix    │  │                                         │  │
│          │  │   active tab content                     │  │
│ ○ ui-    │  │                                         │  │
│   rework │  │                                         │  │
│          │  └─────────────────────────────────────────┘  │
└──────────┴───────────────────────────────────────────────┘
```

Sidebar: workspace list with status dots (pulsing amber = running, olive = waiting).
Work mode tabs: Chat, Diff, Files, Terminal, Run. Actions: Review, Push & create PR.

---

## What not to build (unless explicitly instructed)
- Codex support
- Checkpoint/restore of Claude conversation history
- MCP config UI
- Multi-repo open simultaneously
- Windows support

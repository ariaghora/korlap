# Conductor Clone — Design Doc
## Stack: Tauri v2 + Svelte 5 + Bun

---

## Identity

**Name:** Korlap (from Indonesian "koordinator lapangan" — field coordinator, the person who orchestrates parallel operations on the ground)

**Design direction:** Warm dark. All blacks tinted amber, not cold gray. Single accent color: `#c8a97e` (muted gold). Nothing else competes with it.

**Color hierarchy** (darkest → lightest):
| Token | Hex | Usage |
|-------|-----|-------|
| `bg-sidebar` | `#0f0d0a` | Sidebar, deepest layer |
| `bg-base` | `#13110e` | Main panel background |
| `bg-titlebar` | `#1a1714` | Title bar, tab bar |
| `bg-card` | `#1a1814` | Assistant message cards, input fields |
| `bg-hover` | `#1e1b17` | Hover states |
| `bg-active` | `#2a2520` | Selected items, user bubbles, active tab |
| `border` | `#2a2520` | Primary borders |
| `border-light` | `#3a3530` | Active item borders, button borders |
| `text-muted` | `#4a4540` | Placeholders, separators |
| `text-dim` | `#6a6050` | Labels, breadcrumb base, secondary text |
| `text-secondary` | `#8a7e6a` | Tool tags, inactive tabs, button text |
| `text-primary` | `#d4c5a9` | Body text, input text |
| `text-bright` | `#e8dcc8` | Headings, active items, user message text |
| `accent` | `#c8a97e` | Running state, active branch, focus ring |
| `status-ok` | `#7e9e6b` | Waiting/ready state |
| `diff-add` | `#7e9e6b` / bg `#1a2a1a` | Diff additions |
| `diff-del` | `#c87e7e` / bg `#2a1a1a` | Diff deletions |
| `error` | `#e88` / bg `#3a1a1a` | Error states |

**Typeface:** Space Grotesk. Geometric, purposeful, slightly idiosyncratic. Not Inter.

**No app name in the title bar.** Korlap identifies itself through aesthetic, not a label.

---

## What We're Building

A desktop app that orchestrates multiple Claude Code agents in parallel, each isolated in a `git worktree`. GUI lets you monitor agents, view diffs, run scripts, and switch between workspaces.

Repositories are first-class entities. Workspaces are children of a repository. Each repository carries a bound `gh` auth profile, so switching between orgs/companies is explicit and never bleeds across repos.

Core primitives:
- `git worktree` for workspace isolation
- `claude` CLI as agent subprocess with PTY
- `gh auth switch` integrated — called transparently before any `gh` op on a repo
- Per-workspace state + diff viewing

---

## Stack Rationale

| Layer | Choice | Why |
|-------|--------|-----|
| Shell | Tauri v2 | Rust backend = native process/PTY control; no Electron bloat |
| UI | Svelte 5 | Runes-based reactivity is genuinely good; minimal overhead |
| Runtime | Bun | Fast installs, built-in TS, test runner |
| Styling | Tailwind v4 | Zero config, good DX |
| Terminal emulator | xterm.js | De-facto standard; works well in Tauri WebView |

---

## Project Structure

```
conductor/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs
│   │   ├── workspace.rs     # worktree create/archive/restore
│   │   ├── agent.rs         # claude CLI spawn + PTY management
│   │   ├── git.rs           # diff, branch, push ops via git2
│   │   └── commands.rs      # Tauri #[command] handlers (IPC bridge)
│   └── Cargo.toml
├── src/
│   ├── lib/
│   │   ├── stores/
│   │   │   ├── workspaces.svelte.ts   # $state workspace list
│   │   │   └── agent.svelte.ts        # per-workspace agent state
│   │   └── ipc.ts                     # typed wrappers around invoke()
│   ├── components/
│   │   ├── Sidebar.svelte             # workspace list
│   │   ├── WorkspacePanel.svelte      # main content area
│   │   ├── Terminal.svelte            # xterm.js PTY passthrough
│   │   ├── DiffViewer.svelte          # git diff renderer
│   │   └── ScriptRunner.svelte
│   ├── app.svelte
│   └── app.css
├── package.json
└── bunfig.toml
```

---

## Rust Backend Design

### Core Data Model

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Repository {
    pub id: String,           // uuid
    pub path: PathBuf,        // local clone path — unique key, one profile per path
    pub name: String,         // derived from git remote or dirname
    pub gh_profile: String,   // gh auth switch profile name (e.g. "personal", "work-acme")
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub id: String,
    pub repo_id: String,      // FK -> Repository
    pub name: String,
    pub branch: String,
    pub worktree_path: PathBuf,
    pub status: WorkspaceStatus,
    pub created_at: i64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum WorkspaceStatus {
    Idle,
    Running { pid: u32 },
    Waiting,    // agent paused at permission prompt
    Done,
    Archived,
}
```

### State (Tauri Managed State)

```rust
pub struct AppState {
    pub repos: Mutex<HashMap<String, Repository>>,
    pub workspaces: Mutex<HashMap<String, Workspace>>,
    pub agents: Mutex<HashMap<String, AgentHandle>>,
    pub active_repo_id: Mutex<Option<String>>,
}

pub struct AgentHandle {
    pub pty_master: Box<dyn MasterPty>,
    pub writer: Box<dyn Write + Send>,
    pub pid: u32,
}
```

### `gh` Profile Switching

`gh auth switch` is a process-level operation — it mutates `~/.config/gh/hosts.yml`. Since agents run concurrently across repos with potentially different profiles, we **never call `gh auth switch` globally**. Instead, we inject `GH_TOKEN` per-process by reading the token for the bound profile at spawn time.

```rust
fn gh_token_for_profile(profile: &str) -> Result<String> {
    // `gh auth token --hostname github.com --user <profile>`
    let out = Command::new("gh")
        .args(["auth", "token", "--user", profile])
        .output()?;
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

fn spawn_agent(ws: &Workspace, repo: &Repository, app: &AppHandle) -> Result<AgentHandle> {
    let token = gh_token_for_profile(&repo.gh_profile)?;

    let mut cmd = CommandBuilder::new("claude");
    cmd.cwd(&ws.worktree_path);
    cmd.env("GH_TOKEN", token);
    cmd.env("GITHUB_TOKEN", token); // some tools check this instead

    // ... rest of PTY spawn
}
```

This is safe for parallel agents across different repos — no global state mutation, each process gets its own env.

### Tauri Commands (IPC surface)

```rust
// Repository management
#[tauri::command] add_repo(path, gh_profile) -> Result<Repository>
#[tauri::command] remove_repo(repo_id) -> Result<()>
#[tauri::command] list_repos() -> Result<Vec<Repository>>
#[tauri::command] set_active_repo(repo_id) -> Result<()>
#[tauri::command] list_gh_profiles() -> Result<Vec<String>>   // shells out to `gh auth status`
#[tauri::command] update_repo_profile(repo_id, gh_profile) -> Result<()>

// Workspace management (all scoped to a repo)
#[tauri::command] create_workspace(repo_id, name, base_branch) -> Result<Workspace>
#[tauri::command] archive_workspace(id) -> Result<()>
#[tauri::command] restore_workspace(id) -> Result<Workspace>
#[tauri::command] list_workspaces(repo_id) -> Result<Vec<Workspace>>

// Agent
#[tauri::command] spawn_agent(workspace_id) -> Result<u32>
#[tauri::command] kill_agent(workspace_id) -> Result<()>
#[tauri::command] write_to_agent(workspace_id, data: Vec<u8>) -> Result<()>
#[tauri::command] resize_pty(workspace_id, rows: u16, cols: u16) -> Result<()>

// Git
#[tauri::command] get_diff(workspace_id) -> Result<String>
#[tauri::command] get_branches(repo_id) -> Result<Vec<String>>

// Scripts
#[tauri::command] run_script(workspace_id, script: String) -> Result<()>

// Events emitted TO frontend
// "agent-output"  { workspace_id, data: Vec<u8> }
// "agent-status"  { workspace_id, status: WorkspaceStatus }
```

### Worktree Lifecycle

```rust
// create
fn create_workspace(name: &str, base: &str, repo: &Path) -> Result<Workspace> {
    let id = Uuid::new_v4().to_string();
    let branch = format!("conductor/{name}");
    let path = repo.join(".conductor/worktrees").join(&id);

    Command::new("git")
        .args(["worktree", "add", "-b", &branch, path.to_str().unwrap(), base])
        .current_dir(repo)
        .output()?;

    Ok(Workspace { id, branch, worktree_path: path, status: Idle, .. })
}

// archive: kill agent + remove worktree from fs (branch survives)
fn archive_workspace(ws: &Workspace) -> Result<()> {
    Command::new("git")
        .args(["worktree", "remove", "--force", ws.worktree_path.to_str().unwrap()])
        .output()?;
    Ok(())
}

// restore: re-add worktree on existing branch
fn restore_workspace(ws: &Workspace, repo: &Path) -> Result<()> {
    Command::new("git")
        .args(["worktree", "add", ws.worktree_path.to_str().unwrap(), &ws.branch])
        .current_dir(repo)
        .output()?;
    Ok(())
}
```

### PTY / Agent Spawning

Use `portable-pty` crate. Key flow:

```rust
fn spawn_agent(ws: &Workspace, app_handle: &AppHandle) -> Result<AgentHandle> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, .. })?;

    let cmd = CommandBuilder::new("claude");
    let _child = pair.slave.spawn_command(cmd)?;

    let master = pair.master;
    let mut reader = master.try_clone_reader()?;
    let workspace_id = ws.id.clone();
    let handle = app_handle.clone();

    // stream PTY output -> frontend via Tauri event
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    handle.emit("agent-output", AgentOutput {
                        workspace_id: workspace_id.clone(),
                        data: buf[..n].to_vec(),
                    }).ok();
                }
            }
        }
    });

    Ok(AgentHandle { pty_master: master, writer: pair.master.take_writer()?, .. })
}
```

---

## Frontend Design

### Performance Rules (non-negotiable)

1. **Chat messages use `Map<string, Map<string, Message>>`.** Updating one message = one reactive cell, not the whole list. Never use arrays for collections that update frequently.
2. **Keyed `{#each}` blocks everywhere.** Svelte diffs by key — without keys it re-renders the entire list.
3. **Persistence is debounced.** Messages saved to disk every 500ms, not on every event. Fire-and-forget — never block the UI thread.
4. **Active workspace switch is O(1).** Just toggle `display:none`. No data fetching, no re-initialization. Each workspace's chat panel stays alive in the DOM.
5. **All app data in Tauri's app data dir.** Zero writes to the managed repo. No `.korlap/` folder, no gitignore entries.

---

### State Shape

```ts
// stores/repos.svelte.ts
export const repos = $state(new Map<string, Repository>());
export const activeRepoId = $state<string | null>(null);
export const activeRepo = $derived(
    activeRepoId ? repos.get(activeRepoId) ?? null : null
);

// stores/workspaces.svelte.ts
// Flat map of all workspaces across all repos.
// UI filters by activeRepoId — no separate per-repo lists to keep in sync.
export const workspaces = $state(new Map<string, Workspace>());
export const activeWorkspaceId = $state<string | null>(null);

export const visibleWorkspaces = $derived(
    activeRepoId
        ? [...workspaces.values()].filter(w => w.repoId === activeRepoId)
        : []
);

// The single biggest performance decision: Map, not array.
// Updating one message = one reactive cell changes, not the whole list.
type MessageChunk = { type: "text"; content: string } | { type: "tool"; name: string; input: string };

interface Message {
    id: string;
    role: "user" | "assistant";
    chunks: MessageChunk[];
    done: boolean;
}

// stores/messages.svelte.ts

// stores/messages.svelte.ts
// Keyed by workspaceId -> message list (also a Map for O(1) tail updates)
export const messagesByWorkspace = $state(new Map<string, Map<string, Message>>());

// Append a chunk to the last message — only that message's reactive cell fires
export function appendChunk(workspaceId: string, messageId: string, chunk: MessageChunk) {
    const msgs = messagesByWorkspace.get(workspaceId)!;
    const msg = msgs.get(messageId)!;
    msg.chunks.push(chunk);  // fine: Svelte 5 tracks array mutations at the property level
}
```

### Rust: PTY Batching

The Rust reader thread accumulates bytes and flushes on a timer, not per read. This caps IPC event rate regardless of how fast the agent writes.

```rust
fn stream_pty_output(
    mut reader: Box<dyn Read + Send>,
    workspace_id: String,
    app: AppHandle,
) {
    let mut buf = [0u8; 8192];
    let mut accumulator: Vec<u8> = Vec::with_capacity(16384);
    let mut last_flush = Instant::now();
    const FLUSH_INTERVAL: Duration = Duration::from_millis(16); // ~60fps

    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                accumulator.extend_from_slice(&buf[..n]);
                if last_flush.elapsed() >= FLUSH_INTERVAL {
                    app.emit("agent-output", AgentOutput {
                        workspace_id: workspace_id.clone(),
                        data: std::mem::take(&mut accumulator),
                    }).ok();
                    last_flush = Instant::now();
                }
            }
        }
    }
    // flush remainder
    if !accumulator.is_empty() {
        app.emit("agent-output", AgentOutput { workspace_id, data: accumulator }).ok();
    }
}
```

### IPC: Binary Transfer

`Vec<u8>` serialized as a JSON number array is wasteful. Use Tauri's `ArrayBuffer` channel instead:

```ts
// Frontend receives PTY data as ArrayBuffer, not number[]
// In tauri.conf.json, use invoke channels for high-frequency binary data

import { Channel } from "@tauri-apps/api/core";

export function openPtyChannel(workspaceId: string, onData: (data: Uint8Array) => void) {
    const ch = new Channel<ArrayBuffer>();
    ch.onmessage = buf => onData(new Uint8Array(buf));
    invoke("open_pty_stream", { workspaceId, channel: ch });
    return ch;
}
```

This bypasses JSON serialization entirely for the hot path.

### Terminal Component

xterm.js is a black box. We never read from it, only write to it. Its internal canvas renderer handles everything.

```svelte
<!-- Terminal.svelte -->
<script lang="ts">
  import { onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { openPtyChannel } from "$lib/ipc";

  let { workspaceId }: { workspaceId: string } = $props();
  let el: HTMLDivElement;

  onMount(() => {
    const term = new Terminal({
        scrollback: 5000,  // cap scrollback, don't let it grow unbounded
        theme: { background: "#0d0d0d" },
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    fit.fit();

    // resize: propagate to PTY so apps (vim, etc.) behave correctly
    const ro = new ResizeObserver(() => {
        fit.fit();
        invoke("resize_pty", { workspaceId, rows: term.rows, cols: term.cols });
    });
    ro.observe(el);

    term.onData(data =>
        invoke("write_to_agent", { workspaceId, data: new TextEncoder().encode(data) })
    );

    const ch = openPtyChannel(workspaceId, data => term.write(data));

    return () => { ch.close(); ro.disconnect(); term.dispose(); };
  });
</script>

<div bind:this={el} class="h-full w-full" />
```

### Message List

**No fixed-height virtualization for chat.** Chat messages are variable-height (short replies, code blocks, tool use lists). A fixed `itemHeight` VirtualList breaks layout — `position: relative` + `transform: translateY` kills flexbox alignment needed for user/assistant message positioning.

The real performance wins for the message list are:

1. **`Map<string, Message>` not arrays** — updating one message doesn't trigger a full list re-render. Already implemented in `stores/messages.svelte.ts`.
2. **Keyed `{#each}` blocks** — Svelte only re-renders changed messages via `(msg.id)` keys.
3. **Debounced persistence** — messages saved to disk at most every 500ms, not per-message.

If message count becomes a bottleneck (hundreds per workspace), implement **measured-height virtualization** — render items, measure with ResizeObserver, cache heights in a Map, then calculate visible window. Not the naive fixed-height approach.

### Workspace Switching

xterm instances are **never destroyed** when switching workspaces — only detached from the DOM. This preserves scrollback and avoids re-initialization cost.

```svelte
<!-- WorkspacePanel.svelte -->
<script lang="ts">
  import { activeId } from "$lib/stores/workspaces.svelte";

  let { workspaceIds }: { workspaceIds: string[] } = $props();
</script>

<!-- All terminals rendered, only active one visible -->
{#each workspaceIds as id (id)}
  <div class={id === activeId ? "block" : "hidden"}>
    <Terminal workspaceId={id} />
  </div>
{/each}
```

`hidden` = `display: none`. The xterm canvas stays alive, no teardown/reinit on switch. Cost of switching is one class toggle.

### App Layout

```
┌──────────────────────────────────────────────────────┐
│  ◆ Conductor   my-app   [+ New Workspace]    ⚙       │
├──────────┬───────────────────────────────────────────┤
│          │  feat/auth-refactor          [⚡Running]  │
│ ● auth   │  ┌─────────────────────────────────────┐  │
│   fix    │  │  [Chat] [Diff] [Terminal] [Scripts] │  │
│          │  ├─────────────────────────────────────┤  │
│ ○ ui-    │  │                                     │  │
│   rework │  │   xterm.js PTY panel here           │  │
│          │  │                                     │  │
│ ⊘ old    │  │                                     │  │
│   feat   │  │                                     │  │
│          │  └─────────────────────────────────────┘  │
└──────────┴───────────────────────────────────────────┘
```

Sidebar: workspace list with status indicator (running / waiting / idle / archived).
Main panel: tab strip per workspace (Chat, Diff, Terminal, Scripts).

---

## Key Dependencies

### Rust (Cargo.toml)
```toml
tauri = { version = "2", features = ["shell-open"] }
portable-pty = "0.8"
git2 = "0.19"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

### Frontend (package.json)
```json
{
  "@tauri-apps/api": "^2",
  "@tauri-apps/plugin-shell": "^2",
  "@xterm/xterm": "^5",
  "@xterm/addon-fit": "^0.10",
  "svelte": "^5",
  "tailwindcss": "^4",
  "typescript": "^5"
}
```

---

## MVP Milestones

### M1 — Plumbing (no real UI)
- Open a repo, persist path
- Create / archive / restore worktrees
- Spawn `claude` in a worktree via PTY
- Stream PTY output to a dumb `<pre>` tag

### M2 — Real UI
- Sidebar workspace list with status
- xterm.js terminal per workspace (input + output)
- Tab strip (Chat + Terminal to start)

### M3 — Diff + Scripts
- Diff tab: `git diff main..<branch>` rendered with syntax highlighting
- Scripts tab: run arbitrary shell commands, stream output

### M4 — Polish
- Workspace state persistence across restarts (workspaces.json)
- Archive/restore UI
- Keyboard shortcuts

---

## Explicitly Out of Scope (v0)

- PR creation
- MCP config UI
- Checkpoints / conversation restore
- Multi-repo support
- Windows support (PTY story is painful)

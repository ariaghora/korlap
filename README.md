> [!WARNING]  
> This is an alpha-quality software, where major parts of the code was written by Claude. It is usable at least for the maintainer's use cases and workflows.
> By using this you acknowledge that this tool will undergo tons of changes anytime as the maintainer wishes and deems appropriate.

# Korlap

A macOS kanban app for running multiple [Claude Code](https://docs.anthropic.com/en/docs/claude-code) agents in parallel, each isolated in its own `git worktree`.

<img width="1380" height="889" alt="image" src="https://github.com/user-attachments/assets/a94561fe-9831-46cf-b31f-5c2e12678f4c" />

<img width="1380" height="889" alt="image" src="https://github.com/user-attachments/assets/b19b4714-e2db-418e-8a82-dacd943a31e3" />



Built with Tauri v2, Svelte 5, and Rust. Primary support for macOS, may or may not run well on the other platforms.

## What it does

Korlap puts the developer in the role of orchestrator. You define tasks, spawn agents explicitly, and review their output. Nothing runs without your intent.

The interface has two modes. **Plan** is a kanban board where each card is a task for an AI agent. When a task moves to "In Progress," Korlap creates a workspace: a git worktree on its own branch with a dedicated Claude Code agent. **Work** is where you interact with individual agents, read their diffs, run scripts, and review changes before they merge.

The core assumption: the kanban board is for AI agents, and each task maps to an isolated workspace.

### Features

- **Task board.** Four-column kanban (Todo, In Progress, Review, Done) that drives agent lifecycle.
- **Workspace isolation.** Each agent gets a full worktree copy of the repo on a dedicated branch.
- **Structured chat.** Agent output parsed from `--output-format stream-json`, rendered as a rich message list.
- **Language Server Protocol (LSP).** LSP integration for a more efficient editing and navigation for agent and human.
- **Diff viewer.** See what each agent changed against the base branch, with syntax highlighting.
- **Review flow.** Opus-powered evaluation of diffs before merging.
- **Script runner.** Run shell commands inside any worktree without leaving the app.
- **`gh` profile support.** Bind a GitHub auth profile per repo so tokens stay scoped to the right org.

The name comes from Indonesian *koordinator lapangan* (field coordinator). The person who orchestrates parallel operations on the ground.

## Prerequisites

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

# Build the app bundle
bun tauri build
```

That's it. The app opens, you add a repo via the folder picker, and create your first workspace.

## Tech stack

| Layer | Choice | Why |
|-------|--------|-----|
| Shell | Tauri v2 | Rust backend with native process and PTY control |
| UI | Svelte 5 | Runes reactivity, minimal runtime cost |
| Runtime | Bun | Fast installs, built-in TypeScript |
| Styling | Tailwind v4 | Zero config |
| Terminal | xterm.js | Standard terminal emulator for the raw PTY tab |

## Contributing

Do your best not to get rejected.

## License

MIT

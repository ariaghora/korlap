<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { openTerminal, writeTerminal, resizeTerminal } from "$lib/ipc";
  import "@xterm/xterm/css/xterm.css";

  interface Props {
    workspaceId: string;
  }

  let { workspaceId }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let term: Terminal | undefined;
  let fitAddon: FitAddon | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let colorSchemeQuery: MediaQueryList | undefined;
  let opened = false;
  let fitRafId: number | undefined;

  const darkTheme = {
    background: "#12110e",
    foreground: "#d4c5a9",
    cursor: "#c8a97e",
    cursorAccent: "#12110e",
    selectionBackground: "#c8a97e44",
    black: "#1e1b17",
    red: "#c87e7e",
    green: "#7e9e6b",
    yellow: "#c8a97e",
    blue: "#7e8e9e",
    magenta: "#9e7e8e",
    cyan: "#7e9e9e",
    white: "#d4c5a9",
    brightBlack: "#6a6050",
    brightRed: "#e8a0a0",
    brightGreen: "#a0c890",
    brightYellow: "#e8c8a0",
    brightBlue: "#a0b0c8",
    brightMagenta: "#c8a0b8",
    brightCyan: "#a0c8c0",
    brightWhite: "#e8dcc8",
  };

  const lightTheme = {
    background: "#f7f4ef",
    foreground: "#33302a",
    cursor: "#9a7a48",
    cursorAccent: "#f7f4ef",
    selectionBackground: "#9a7a4844",
    black: "#dbd4c7",
    red: "#b54545",
    green: "#4e7a3a",
    yellow: "#9a7a48",
    blue: "#4e6a8e",
    magenta: "#7e4e6e",
    cyan: "#3a7a6e",
    white: "#33302a",
    brightBlack: "#907f6d",
    brightRed: "#c85050",
    brightGreen: "#5a8e45",
    brightYellow: "#b08a50",
    brightBlue: "#5a7aa0",
    brightMagenta: "#905a80",
    brightCyan: "#4a8e80",
    brightWhite: "#1a1714",
  };

  function getTermTheme() {
    return window.matchMedia("(prefers-color-scheme: dark)").matches
      ? darkTheme
      : lightTheme;
  }

  function onColorSchemeChange() {
    if (term) {
      term.options.theme = getTermTheme();
    }
  }

  function initTerminal() {
    if (!containerEl || opened) return;
    // Don't open if container is hidden (zero dimensions)
    if (containerEl.offsetHeight === 0) return;

    term = new Terminal({
      scrollback: 10000,
      fontFamily: "'SF Mono', 'Fira Code', 'Menlo', monospace",
      fontSize: 13,
      theme: getTermTheme(),
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerEl);
    fitAddon.fit();
    opened = true;

    // Send keystrokes to PTY
    term.onData((data) => {
      const bytes = Array.from(new TextEncoder().encode(data));
      writeTerminal(workspaceId, bytes).catch(() => {});
    });

    // Open PTY and stream output
    openTerminal(workspaceId, (data: number[]) => {
      if (term) {
        term.write(new Uint8Array(data));
      }
    })
      .then(() => {
        // Sync PTY size with actual xterm dimensions (PTY defaults to 24x80)
        if (term) {
          resizeTerminal(workspaceId, term.rows, term.cols).catch(() => {});
        }
      })
      .catch((e) => {
        if (term) {
          term.writeln(`\r\n\x1b[31mFailed to open terminal: ${e}\x1b[0m`);
        }
      });
  }

  onMount(() => {
    if (!containerEl) return;

    // Listen for system color scheme changes to update xterm theme
    colorSchemeQuery = window.matchMedia("(prefers-color-scheme: dark)");
    colorSchemeQuery.addEventListener("change", onColorSchemeChange);

    // Use ResizeObserver to detect when container becomes visible.
    // Guard fit() against zero dimensions (display:none when tab not active).
    resizeObserver = new ResizeObserver(() => {
      if (!containerEl || containerEl.offsetHeight === 0) return;
      if (!opened) {
        initTerminal();
      } else if (fitAddon && term) {
        // Debounce fit() to next frame to avoid ResizeObserver loop warnings.
        // fit() mutates the DOM which can trigger another resize notification
        // in the same observation cycle — deferring breaks the loop.
        if (fitRafId !== undefined) cancelAnimationFrame(fitRafId);
        fitRafId = requestAnimationFrame(() => {
          fitRafId = undefined;
          if (fitAddon && term) {
            fitAddon.fit();
            resizeTerminal(workspaceId, term.rows, term.cols).catch(() => {});
          }
        });
      }
    });
    resizeObserver.observe(containerEl);
  });

  onDestroy(() => {
    if (fitRafId !== undefined) cancelAnimationFrame(fitRafId);
    colorSchemeQuery?.removeEventListener("change", onColorSchemeChange);
    resizeObserver?.disconnect();
    term?.dispose();
  });
</script>

<div class="terminal-container" bind:this={containerEl}></div>

<style>
  .terminal-container {
    flex: 1;
    min-height: 0;
    background: var(--bg-base);
    overflow: hidden;
  }

  .terminal-container :global(.xterm) {
    height: 100%;
    padding: 8px 12px;
  }

  .terminal-container :global(.xterm-viewport) {
    background-color: var(--bg-base) !important;
  }

  .terminal-container :global(.xterm-screen) {
    width: 100% !important;
  }
</style>

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { openTerminal, writeTerminal, resizeTerminal } from "$lib/ipc";
  import { getTerminalTheme } from "$lib/stores/theme.svelte";
  import "@xterm/xterm/css/xterm.css";

  interface Props {
    workspaceId: string;
  }

  let { workspaceId }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let term: Terminal | undefined;
  let fitAddon: FitAddon | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let opened = false;
  let fitRafId: number | undefined;

  function onThemeChange() {
    if (term) {
      term.options.theme = getTerminalTheme();
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
      theme: getTerminalTheme(),
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

    // Listen for theme changes (color scheme + theme picker)
    window.addEventListener("korlap-theme-change", onThemeChange);

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
    window.removeEventListener("korlap-theme-change", onThemeChange);
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

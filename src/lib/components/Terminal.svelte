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

  onMount(() => {
    if (!containerEl) return;

    term = new Terminal({
      scrollback: 10000,
      fontFamily: "var(--font-mono)",
      fontSize: 13,
      theme: {
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
      },
    });

    fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerEl);
    fitAddon.fit();

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
    }).catch((e) => {
      if (term) {
        term.writeln(`\r\n\x1b[31mFailed to open terminal: ${e}\x1b[0m`);
      }
    });

    // Resize PTY when container resizes
    resizeObserver = new ResizeObserver(() => {
      if (fitAddon && term) {
        fitAddon.fit();
        resizeTerminal(workspaceId, term.rows, term.cols).catch(() => {});
      }
    });
    resizeObserver.observe(containerEl);
  });

  onDestroy(() => {
    resizeObserver?.disconnect();
    term?.dispose();
  });
</script>

<div class="terminal-container" bind:this={containerEl}></div>

<style>
  .terminal-container {
    flex: 1;
    min-height: 0;
    padding: 4px;
  }

  .terminal-container :global(.xterm) {
    height: 100%;
  }

  .terminal-container :global(.xterm-viewport) {
    overflow-y: auto;
  }
</style>

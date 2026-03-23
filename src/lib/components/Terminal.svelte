<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { openTerminal, writeTerminal, resizeTerminal } from "$lib/ipc";
  import { getTerminalTheme } from "$lib/stores/theme.svelte";
  import "@xterm/xterm/css/xterm.css";

  interface Props {
    workspaceId: string;
    visible?: boolean;
  }

  let { workspaceId, visible = true }: Props = $props();

  let containerEl: HTMLDivElement | undefined = $state();
  let term: Terminal | undefined;
  let fitAddon: FitAddon | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let opened = false;
  let fitDebounceId: ReturnType<typeof setTimeout> | undefined;

  function onThemeChange() {
    if (term) {
      term.options.theme = getTerminalTheme();
    }
  }

  function initTerminal() {
    if (!containerEl || opened) return;
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

    term.onData((data) => {
      const bytes = Array.from(new TextEncoder().encode(data));
      writeTerminal(workspaceId, bytes).catch(() => {});
    });

    openTerminal(workspaceId, (data: number[]) => {
      if (term) {
        term.write(new Uint8Array(data));
      }
    })
      .then(() => {
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

  // Init when visible prop transitions to true (parent controls display)
  $effect(() => {
    if (visible && !opened && containerEl) {
      // rAF ensures layout has settled after display:none → display:flex
      const id = requestAnimationFrame(() => {
        if (!opened && containerEl && containerEl.offsetHeight > 0) {
          initTerminal();
        }
      });
      return () => cancelAnimationFrame(id);
    }
  });

  onMount(() => {
    if (!containerEl) return;

    window.addEventListener("korlap-theme-change", onThemeChange);

    // ResizeObserver handles fit() on resize. Also inits if visible at mount.
    resizeObserver = new ResizeObserver(() => {
      if (!containerEl || containerEl.offsetHeight === 0) return;
      if (!opened) {
        initTerminal();
      } else if (fitAddon && term) {
        if (fitDebounceId !== undefined) clearTimeout(fitDebounceId);
        fitDebounceId = setTimeout(() => {
          fitDebounceId = undefined;
          if (fitAddon && term) {
            fitAddon.fit();
            resizeTerminal(workspaceId, term.rows, term.cols).catch(() => {});
          }
        }, 100);
      }
    });
    resizeObserver.observe(containerEl);
  });

  onDestroy(() => {
    if (fitDebounceId !== undefined) clearTimeout(fitDebounceId);
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

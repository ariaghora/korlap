<script lang="ts">
  import "../app.css";
  import { onMount } from "svelte";
  import { initTheme } from "$lib/stores/theme.svelte";

  let { children } = $props();

  // Apply saved theme on startup + listen for color scheme changes
  initTheme();

  // Suppress the benign "ResizeObserver loop" error globally.
  // This fires when a ResizeObserver callback causes layout changes that produce
  // new resize notifications that can't be delivered in the same frame. The browser
  // re-queues them automatically — the warning is harmless by spec (ResizeObserver §3.3).
  // Our VirtualScroller's measure→relayout cycle and xterm's fit() both trigger this.
  onMount(() => {
    function suppress(e: ErrorEvent) {
      if (e.message?.startsWith("ResizeObserver loop")) {
        e.stopImmediatePropagation();
      }
    }
    window.addEventListener("error", suppress);
    return () => window.removeEventListener("error", suppress);
  });
</script>

{@render children()}

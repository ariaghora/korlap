<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Message } from "$lib/stores/messages.svelte";

  interface Props {
    items: Message[];
    itemHeight?: number;
    renderItem: Snippet<[Message]>;
    autoScroll?: boolean;
  }

  let {
    items,
    itemHeight = 80,
    renderItem,
    autoScroll = true,
  }: Props = $props();

  let containerHeight = $state(0);
  let scrollTop = $state(0);
  let container: HTMLDivElement | undefined = $state();
  let userScrolledUp = $state(false);

  const overscan = 5;
  const start = $derived(Math.max(0, Math.floor(scrollTop / itemHeight) - overscan));
  const end = $derived(
    Math.min(items.length, Math.ceil((scrollTop + containerHeight) / itemHeight) + overscan),
  );
  const visible = $derived(items.slice(start, end));
  const totalHeight = $derived(items.length * itemHeight);
  const offsetY = $derived(start * itemHeight);

  function handleScroll(e: Event) {
    const el = e.target as HTMLElement;
    scrollTop = el.scrollTop;
    // Detect if user scrolled away from bottom
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    userScrolledUp = !atBottom;
  }

  // Auto-scroll to bottom when new items arrive (if user hasn't scrolled up)
  $effect(() => {
    items.length; // track
    if (autoScroll && !userScrolledUp && container) {
      requestAnimationFrame(() => {
        container!.scrollTop = container!.scrollHeight;
      });
    }
  });
</script>

<div
  class="virtual-list"
  bind:this={container}
  bind:clientHeight={containerHeight}
  onscroll={handleScroll}
>
  <div class="virtual-list-inner" style="height: {totalHeight}px; position: relative;">
    <div style="transform: translateY({offsetY}px)">
      {#each visible as msg (msg.id)}
        {@render renderItem(msg)}
      {/each}
    </div>
  </div>
</div>

<style>
  .virtual-list {
    overflow-y: auto;
    height: 100%;
  }
</style>

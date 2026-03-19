<script lang="ts">
  import { ChevronDown } from "lucide-svelte";
  import type { Snippet } from "svelte";

  interface Props {
    trigger: Snippet;
    children: Snippet;
    onclose?: () => void;
  }

  let { trigger, children, onclose }: Props = $props();

  let open = $state(false);
  let rootEl: HTMLDivElement | undefined = $state();
  let prevOpen = false;

  $effect(() => {
    if (prevOpen && !open) onclose?.();
    prevOpen = open;
  });

  export function close() {
    open = false;
  }

  export function toggle() {
    open = !open;
  }

  export function isOpen() {
    return open;
  }

  function handleClickOutside(e: MouseEvent) {
    if (open && rootEl && !rootEl.contains(e.target as Node)) {
      open = false;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window onmousedown={handleClickOutside} />

<div class="dropdown" bind:this={rootEl}>
  <button class="dropdown-trigger" onclick={toggle}>
    {@render trigger()}
    <ChevronDown size={12} class="dropdown-chevron" />
  </button>

  {#if open}
    <div class="dropdown-menu">
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .dropdown {
    position: relative;
  }

  .dropdown-trigger {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.5rem;
    background: transparent;
    border: 1px solid var(--border-light);
    border-radius: 5px;
    color: var(--text-bright);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .dropdown-trigger:hover {
    background: var(--border);
    border-color: var(--border-light);
  }

  .dropdown-trigger :global(.dropdown-chevron) {
    color: var(--text-dim);
  }

  .dropdown-menu {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    min-width: 200px;
    max-width: 280px;
    background: var(--bg-titlebar);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    padding: 0.25rem;
    z-index: 100;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }
</style>

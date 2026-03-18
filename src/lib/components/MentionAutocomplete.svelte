<script lang="ts">
  export interface FileSearchResult {
    path: string;
    name: string;
    kind: "file" | "folder";
    score: number;
  }

  export interface MentionAutocompleteApi {
    moveUp: () => void;
    moveDown: () => void;
    selectCurrent: () => void;
  }

  interface Props {
    results: FileSearchResult[];
    visible: boolean;
    loading: boolean;
    anchorEl: HTMLElement | null;
    onSelect: (result: FileSearchResult) => void;
    ref?: MentionAutocompleteApi | undefined;
  }

  let {
    results,
    visible,
    loading,
    anchorEl,
    onSelect,
    ref = $bindable(undefined),
  }: Props = $props();

  let activeIndex = $state(0);
  let popupEl: HTMLDivElement | undefined = $state();

  // Position state
  let popupStyle = $state("");

  let shouldShow = $derived(visible && (results.length > 0 || loading));

  // Reset active index when results change
  $effect(() => {
    results; // track
    activeIndex = 0;
  });

  // Compute position relative to anchor
  $effect(() => {
    if (!shouldShow || !anchorEl) {
      popupStyle = "display: none;";
      return;
    }

    const rect = anchorEl.getBoundingClientRect();
    // Position above the input, anchored to bottom-left
    popupStyle = `left: ${rect.left}px; bottom: ${window.innerHeight - rect.top + 4}px;`;
  });

  // Scroll active item into view
  $effect(() => {
    if (!popupEl || !shouldShow) return;
    const activeItem = popupEl.querySelector(".autocomplete-item.active");
    if (activeItem) {
      activeItem.scrollIntoView({ block: "nearest" });
    }
  });

  function moveUp() {
    if (results.length === 0) return;
    activeIndex = activeIndex <= 0 ? results.length - 1 : activeIndex - 1;
  }

  function moveDown() {
    if (results.length === 0) return;
    activeIndex = activeIndex >= results.length - 1 ? 0 : activeIndex + 1;
  }

  function selectCurrent() {
    if (results.length === 0 || activeIndex < 0 || activeIndex >= results.length) return;
    onSelect(results[activeIndex]);
  }

  // Expose API via bindable ref
  $effect(() => {
    ref = { moveUp, moveDown, selectCurrent };
  });

  function parentDir(path: string): string {
    const parts = path.split("/");
    if (parts.length <= 1) return "";
    parts.pop();
    return parts.join("/") + "/";
  }
</script>

{#if shouldShow}
  <div
    bind:this={popupEl}
    class="autocomplete-popup"
    style={popupStyle}
  >
    {#if loading && results.length === 0}
      <div class="autocomplete-loading">Searching...</div>
    {/if}
    {#each results as result, i (result.path)}
      <button
        class="autocomplete-item"
        class:active={i === activeIndex}
        onmouseenter={() => { activeIndex = i; }}
        onmousedown={(e) => { e.preventDefault(); onSelect(result); }}
      >
        <span class="autocomplete-icon">
          {result.kind === "folder" ? "\uD83D\uDCC1" : "\uD83D\uDCC4"}
        </span>
        <span class="autocomplete-name">{result.name}</span>
        {#if parentDir(result.path)}
          <span class="autocomplete-path">{parentDir(result.path)}</span>
        {/if}
      </button>
    {/each}
    {#if loading && results.length > 0}
      <div class="autocomplete-loading">...</div>
    {/if}
  </div>
{/if}

<style>
  .autocomplete-popup {
    position: fixed;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    padding: 0.3rem;
    max-height: 240px;
    overflow-y: auto;
    min-width: 200px;
    max-width: 400px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
    z-index: 100;
  }

  .autocomplete-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    border-radius: 5px;
    cursor: pointer;
    font-size: 0.8rem;
    color: var(--text-secondary);
    width: 100%;
    background: none;
    border: none;
    font-family: inherit;
    text-align: left;
  }

  .autocomplete-item.active {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--text-bright);
  }

  .autocomplete-item:hover {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .autocomplete-icon {
    font-size: 0.85rem;
    opacity: 0.6;
    flex-shrink: 0;
  }

  .autocomplete-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .autocomplete-path {
    font-family: var(--font-mono);
    font-size: 0.75rem;
    color: var(--text-dim);
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 180px;
  }

  .autocomplete-loading {
    padding: 0.4rem 0.6rem;
    font-size: 0.75rem;
    color: var(--text-dim);
    font-style: italic;
  }
</style>

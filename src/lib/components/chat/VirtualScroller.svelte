<script lang="ts">
  import { untrack } from "svelte";

  interface Props {
    /** Total number of items */
    count: number;
    /** Estimated height for items not yet measured (px) */
    estimatedHeight?: number;
    /** Gap between items in px */
    gap?: number;
    /** Extra items to render above/below the visible window */
    overscan?: number;
    /** Snippet that renders each item by index */
    children: import("svelte").Snippet<[number]>;
    /** If true, keep scroll pinned to bottom when new items arrive */
    stickToBottom?: boolean;
    /** Called when user scrolls away from bottom */
    onscrolledUp?: (scrolledUp: boolean) => void;
  }

  let {
    count,
    estimatedHeight = 60,
    gap = 0,
    overscan = 5,
    children,
    stickToBottom = false,
    onscrolledUp,
  }: Props = $props();

  let container: HTMLDivElement | undefined = $state();
  let viewportHeight = $state(0);
  let scrollTop = $state(0);

  // Measured heights per index. Plain Map — not reactive, we batch updates.
  const measuredHeights = new Map<number, number>();
  // Track which indices have active ResizeObservers
  const observedElements = new Map<number, Element>();

  let resizeObserver: ResizeObserver | undefined;

  // Trigger re-render when measurements change
  let measureVersion = $state(0);

  function getHeight(index: number): number {
    return measuredHeights.get(index) ?? estimatedHeight;
  }

  // Compute cumulative offsets and total height
  function getLayout(_version: number) {
    const offsets: number[] = new Array(count);
    let y = 0;
    for (let i = 0; i < count; i++) {
      offsets[i] = y;
      y += getHeight(i) + gap;
    }
    const totalHeight = count > 0 ? y - gap : 0;
    return { offsets, totalHeight };
  }

  let layout = $derived(getLayout(measureVersion));

  // Find first visible index via binary search
  function findStartIndex(top: number, offsets: number[]): number {
    let lo = 0;
    let hi = offsets.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >>> 1;
      if (offsets[mid] <= top) {
        lo = mid + 1;
      } else {
        hi = mid - 1;
      }
    }
    return Math.max(0, hi);
  }

  let visibleRange = $derived.by(() => {
    if (count === 0) return { start: 0, end: 0 };
    const { offsets } = layout;
    const startRaw = findStartIndex(scrollTop, offsets);
    const start = Math.max(0, startRaw - overscan);

    const endY = scrollTop + viewportHeight;
    let end = startRaw;
    while (end < count && offsets[end] < endY) {
      end++;
    }
    end = Math.min(count, end + overscan);

    return { start, end };
  });

  // ── Scroll position management ──────────────────────────────────

  let wasAtBottom = true;

  function handleScroll() {
    if (!container) return;
    scrollTop = container.scrollTop;
    const atBottom = container.scrollHeight - container.scrollTop - container.clientHeight < 50;
    wasAtBottom = atBottom;
    onscrolledUp?.(!atBottom);
  }

  // Scroll to bottom ONLY when new items are added (count increases).
  // NOT on measureVersion changes — those should preserve scroll position.
  let prevCountForScroll = 0;
  $effect(() => {
    const c = count;
    if (c > prevCountForScroll && wasAtBottom && stickToBottom && container) {
      untrack(() => {
        requestAnimationFrame(() => {
          if (container) {
            container.scrollTop = container.scrollHeight;
            scrollTop = container.scrollTop;
          }
        });
      });
    }
    prevCountForScroll = c;
  });

  // ── Scroll anchoring on measurement changes ─────────────────────
  // When ResizeObserver updates heights, items above the viewport may shift.
  // Adjust scrollTop so the first visible item stays in place.

  let anchorIndex = $state(-1);
  let anchorOffset = $state(0);

  // Before layout recalculation, capture what the user is looking at.
  $effect.pre(() => {
    // Read measureVersion to run before layout recomputes
    const _v = measureVersion;
    if (!container || count === 0) return;
    const { offsets } = untrack(() => layout);
    const idx = findStartIndex(container.scrollTop, offsets);
    anchorIndex = idx;
    anchorOffset = container.scrollTop - (offsets[idx] ?? 0);
  });

  // After layout recalculation, restore the anchor position.
  $effect(() => {
    const _v = measureVersion;
    if (!container || anchorIndex < 0 || wasAtBottom) return;
    const { offsets } = layout;
    const newTop = (offsets[anchorIndex] ?? 0) + anchorOffset;
    if (Math.abs(container.scrollTop - newTop) > 1) {
      container.scrollTop = newTop;
      scrollTop = newTop;
    }
  });

  // ── ResizeObserver setup ────────────────────────────────────────

  $effect(() => {
    resizeObserver = new ResizeObserver((entries) => {
      let changed = false;
      for (const entry of entries) {
        const el = entry.target as HTMLElement;
        const idx = parseInt(el.dataset.vindex!, 10);
        if (isNaN(idx)) continue;
        const h = entry.borderBoxSize?.[0]?.blockSize ?? el.getBoundingClientRect().height;
        const prev = measuredHeights.get(idx);
        if (prev !== h) {
          measuredHeights.set(idx, h);
          changed = true;
        }
      }
      if (changed) {
        measureVersion++;
      }
    });

    return () => {
      resizeObserver?.disconnect();
      resizeObserver = undefined;
      observedElements.clear();
    };
  });

  // Svelte action: observe element height via ResizeObserver
  function observeItem(el: HTMLElement, index: number) {
    let currentIndex = index;

    function startObserving() {
      if (!resizeObserver) return;
      const prev = observedElements.get(currentIndex);
      if (prev && prev !== el) resizeObserver.unobserve(prev);
      observedElements.set(currentIndex, el);
      el.dataset.vindex = String(currentIndex);
      resizeObserver.observe(el);
    }

    startObserving();

    return {
      update(newIndex: number) {
        if (newIndex !== currentIndex) {
          if (observedElements.get(currentIndex) === el) {
            resizeObserver?.unobserve(el);
            observedElements.delete(currentIndex);
          }
          currentIndex = newIndex;
          startObserving();
        }
      },
      destroy() {
        if (observedElements.get(currentIndex) === el) {
          resizeObserver?.unobserve(el);
          observedElements.delete(currentIndex);
        }
      },
    };
  }

  // Measure viewport height
  $effect(() => {
    if (!container) return;
    const ro = new ResizeObserver(([entry]) => {
      viewportHeight = entry.contentRect.height;
    });
    ro.observe(container);
    viewportHeight = container.clientHeight;
    return () => ro.disconnect();
  });

  // Only clear measured heights for indices that no longer exist.
  // Avoids nuking all cached heights when footer toggles (count changes by 1).
  let prevCount = 0;
  $effect.pre(() => {
    if (count < prevCount) {
      for (let i = count; i < prevCount; i++) {
        measuredHeights.delete(i);
      }
    }
    prevCount = count;
  });
</script>

<div
  class="virtual-scroller"
  bind:this={container}
  onscroll={handleScroll}
>
  <div class="virtual-content" style="height: {layout.totalHeight}px; position: relative;">
    {#each { length: visibleRange.end - visibleRange.start } as _, i}
      {@const index = visibleRange.start + i}
      {@const top = layout.offsets[index]}
      <div
        class="virtual-item"
        style="position: absolute; top: {top}px; left: 0; right: 0;"
        use:observeItem={index}
      >
        {@render children(index)}
      </div>
    {/each}
  </div>
</div>

<style>
  .virtual-scroller {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }
</style>

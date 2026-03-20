<script lang="ts">
  interface Props {
    /** Called with the incremental pixel delta since the last pointer move */
    onResize: (delta: number) => void;
    /** Called when the drag ends */
    onResizeEnd?: () => void;
    /** Orientation of the handle */
    direction?: "horizontal" | "vertical";
  }

  let { onResize, onResizeEnd, direction = "horizontal" }: Props = $props();

  let dragging = $state(false);
  let startPos = 0;

  function handlePointerDown(e: PointerEvent) {
    e.preventDefault();
    dragging = true;
    startPos = direction === "horizontal" ? e.clientX : e.clientY;

    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
  }

  function handlePointerMove(e: PointerEvent) {
    if (!dragging) return;
    const current = direction === "horizontal" ? e.clientX : e.clientY;
    const delta = current - startPos;
    onResize(delta);
    startPos = current;
  }

  function handlePointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    const target = e.currentTarget as HTMLElement;
    try { target.releasePointerCapture(e.pointerId); } catch {}
    onResizeEnd?.();
  }
</script>

<div
  class="resize-handle"
  class:horizontal={direction === "horizontal"}
  class:vertical={direction === "vertical"}
  class:dragging
  role="separator"
  aria-orientation={direction}
  onpointerdown={handlePointerDown}
  onpointermove={handlePointerMove}
  onpointerup={handlePointerUp}
  onpointercancel={handlePointerUp}
></div>

<style>
  .resize-handle {
    flex-shrink: 0;
    position: relative;
    z-index: 10;
    touch-action: none;
  }

  .resize-handle.horizontal {
    width: 1px;
    cursor: col-resize;
    background: var(--border);
  }

  .resize-handle.vertical {
    height: 1px;
    cursor: row-resize;
    background: var(--border);
  }

  /* Invisible wider hit area */
  .resize-handle::after {
    content: "";
    position: absolute;
  }

  .resize-handle.horizontal::after {
    top: 0;
    bottom: 0;
    left: -3px;
    right: -3px;
  }

  .resize-handle.vertical::after {
    left: 0;
    right: 0;
    top: -3px;
    bottom: -3px;
  }

  .resize-handle:hover,
  .resize-handle.dragging {
    background: var(--accent);
  }
</style>

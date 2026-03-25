import { openUrl } from "@tauri-apps/plugin-opener";

// ── Tooltip action ──────────────────────────────────────────────────

export interface TooltipOptions {
  /** Primary label text */
  text: string;
  /** Optional keyboard shortcut displayed as a kbd badge */
  shortcut?: string;
  /** Preferred placement — flips automatically if clipped */
  placement?: "top" | "bottom";
}

/**
 * Svelte action: shows a tooltip near the element on hover.
 * Appears after a very short delay (100 ms) so it feels nearly instant.
 * Tooltip is appended to `document.body` so it's never clipped by overflow.
 */
export function tooltip(node: HTMLElement, options: TooltipOptions) {
  let opts = options;
  let el: HTMLDivElement | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  // Remove any native title that would conflict
  node.removeAttribute("title");

  function show() {
    if (el || !opts.text) return;
    el = document.createElement("div");
    el.className = "korlap-tooltip";

    const label = document.createElement("span");
    label.className = "korlap-tooltip-text";
    label.textContent = opts.text;
    el.appendChild(label);

    if (opts.shortcut) {
      const kbd = document.createElement("kbd");
      kbd.className = "korlap-tooltip-kbd";
      kbd.textContent = opts.shortcut;
      el.appendChild(kbd);
    }

    document.body.appendChild(el);
    position();
  }

  function position() {
    if (!el) return;
    const GAP = 6;
    const EDGE_PAD = 8;
    const rect = node.getBoundingClientRect();
    const tt = el.getBoundingClientRect();
    const placement = opts.placement ?? "top";

    // Horizontal: center on node, clamp to viewport
    let left = rect.left + rect.width / 2 - tt.width / 2;
    left = Math.max(EDGE_PAD, Math.min(left, window.innerWidth - tt.width - EDGE_PAD));

    // Vertical
    let top: number;
    let actualPlacement = placement;
    if (placement === "top") {
      top = rect.top - tt.height - GAP;
      if (top < EDGE_PAD) { top = rect.bottom + GAP; actualPlacement = "bottom"; }
    } else {
      top = rect.bottom + GAP;
      if (top + tt.height > window.innerHeight - EDGE_PAD) { top = rect.top - tt.height - GAP; actualPlacement = "top"; }
    }

    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
    el.dataset.placement = actualPlacement;
  }

  function hide() {
    if (timer) { clearTimeout(timer); timer = null; }
    if (el) { el.remove(); el = null; }
  }

  function onEnter() {
    timer = setTimeout(show, 100);
  }

  function onLeave() {
    hide();
  }

  function onPointerDown() {
    // Dismiss immediately on click so tooltip doesn't linger
    hide();
  }

  node.addEventListener("pointerenter", onEnter);
  node.addEventListener("pointerleave", onLeave);
  node.addEventListener("pointerdown", onPointerDown);

  return {
    update(newOpts: TooltipOptions) {
      opts = newOpts;
      node.removeAttribute("title");
      if (el) {
        // Update live tooltip
        const label = el.querySelector(".korlap-tooltip-text");
        if (label) label.textContent = opts.text;
        const existingKbd = el.querySelector(".korlap-tooltip-kbd");
        if (opts.shortcut) {
          if (existingKbd) {
            existingKbd.textContent = opts.shortcut;
          } else {
            const kbd = document.createElement("kbd");
            kbd.className = "korlap-tooltip-kbd";
            kbd.textContent = opts.shortcut;
            el.appendChild(kbd);
          }
        } else if (existingKbd) {
          existingKbd.remove();
        }
        position();
      }
    },
    destroy() {
      hide();
      node.removeEventListener("pointerenter", onEnter);
      node.removeEventListener("pointerleave", onLeave);
      node.removeEventListener("pointerdown", onPointerDown);
    },
  };
}

// ── Draggable action ─────────────────────────────────────────────────

export interface DragOffset {
  x: number;
  y: number;
}

export interface DraggableOptions {
  /** CSS selector for the drag handle within the node. If omitted, the whole node is the handle. */
  handle?: string;
  /** Current offset — pass stored state so position survives re-mounts. */
  offset?: DragOffset;
  /** Called on drag end with the new offset. Store it in parent state. */
  onDrag?: (offset: DragOffset) => void;
}

/**
 * Svelte action: makes an absolutely-positioned element draggable via pointer events.
 * Uses CSS `transform: translate()` on top of existing positioning.
 * Hard-clamps the entire element within its offset parent (with 8px edge padding).
 * Re-clamps on parent resize so panels can never end up outside the visible area.
 */
export function draggable(node: HTMLElement, options: DraggableOptions = {}) {
  let offsetX = options.offset?.x ?? 0;
  let offsetY = options.offset?.y ?? 0;
  let startX = 0;
  let startY = 0;
  let dragging = false;
  let didMove = false;

  // Cached measurements from drag start (avoid per-frame reflow)
  let baseLeft = 0;
  let baseTop = 0;
  let nodeW = 0;
  let nodeH = 0;
  let parentW = 0;
  let parentH = 0;

  const EDGE_PAD = 8; // min distance from parent edges

  // Apply initial offset
  applyTransform();

  let handle = resolveHandle();
  if (handle) handle.style.cursor = "grab";

  function resolveHandle(): HTMLElement {
    if (options.handle) {
      return (node.querySelector(options.handle) as HTMLElement) ?? node;
    }
    return node;
  }

  function applyTransform() {
    node.style.transform =
      offsetX || offsetY ? `translate(${offsetX}px, ${offsetY}px)` : "";
  }

  /** Clamp offset so the full node stays within its offset parent (with padding). */
  function clamp(newX: number, newY: number): [number, number] {
    const left = baseLeft + newX;
    const top = baseTop + newY;

    // Left/right: keep entire width inside parent
    if (left < EDGE_PAD) newX = EDGE_PAD - baseLeft;
    else if (left + nodeW > parentW - EDGE_PAD)
      newX = parentW - EDGE_PAD - nodeW - baseLeft;

    // Top/bottom: keep entire height inside parent
    if (top < EDGE_PAD) newY = EDGE_PAD - baseTop;
    else if (top + nodeH > parentH - EDGE_PAD)
      newY = parentH - EDGE_PAD - nodeH - baseTop;

    return [newX, newY];
  }

  /** Snapshot the node's natural position and parent size. */
  function measureLayout() {
    const parentEl = node.offsetParent as HTMLElement;
    if (!parentEl) return;
    const nr = node.getBoundingClientRect();
    const pr = parentEl.getBoundingClientRect();
    baseLeft = nr.left - pr.left - offsetX;
    baseTop = nr.top - pr.top - offsetY;
    nodeW = nr.width;
    nodeH = nr.height;
    parentW = pr.width;
    parentH = pr.height;
  }

  /** Re-clamp current offset to parent bounds (e.g. after resize). */
  function reclamp() {
    measureLayout();
    const [cx, cy] = clamp(offsetX, offsetY);
    if (cx !== offsetX || cy !== offsetY) {
      offsetX = cx;
      offsetY = cy;
      applyTransform();
      options.onDrag?.({ x: offsetX, y: offsetY });
    }
  }

  // Watch parent resize so panels can't end up outside after window shrinks
  let resizeObserver: ResizeObserver | undefined;
  const parentEl = node.offsetParent as HTMLElement;
  if (parentEl) {
    resizeObserver = new ResizeObserver(() => {
      if (!dragging && (offsetX || offsetY)) reclamp();
    });
    resizeObserver.observe(parentEl);
  }

  function onPointerDown(e: PointerEvent) {
    // Don't hijack interactive children
    if (
      (e.target as HTMLElement).closest(
        "button, input, textarea, a, select, [contenteditable]",
      )
    )
      return;

    // Resolve handle lazily (may not exist at mount time in conditional blocks)
    handle = resolveHandle();
    if (!handle.contains(e.target as Node)) return;

    e.preventDefault();
    dragging = true;
    didMove = false;
    startX = e.clientX - offsetX;
    startY = e.clientY - offsetY;

    measureLayout();

    handle.setPointerCapture(e.pointerId);
    handle.style.cursor = "grabbing";
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;

    let newX = e.clientX - startX;
    let newY = e.clientY - startY;

    [newX, newY] = clamp(newX, newY);

    offsetX = newX;
    offsetY = newY;
    didMove = true;
    applyTransform();
  }

  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    handle.style.cursor = "grab";
    try {
      handle.releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    if (didMove) {
      options.onDrag?.({ x: offsetX, y: offsetY });
    }
  }

  // Attach to the node (not handle) so we capture even if handle re-renders.
  // The pointerdown handler checks handle.contains() to gate.
  node.addEventListener("pointerdown", onPointerDown);
  node.addEventListener("pointermove", onPointerMove);
  node.addEventListener("pointerup", onPointerUp);

  return {
    update(newOptions: DraggableOptions) {
      options = newOptions;
      const newX = newOptions.offset?.x ?? 0;
      const newY = newOptions.offset?.y ?? 0;
      if (!dragging && (newX !== offsetX || newY !== offsetY)) {
        offsetX = newX;
        offsetY = newY;
        applyTransform();
      }
      const newHandle = resolveHandle();
      if (newHandle !== handle) {
        if (handle) handle.style.cursor = "";
        handle = newHandle;
        handle.style.cursor = "grab";
      }
    },
    destroy() {
      node.removeEventListener("pointerdown", onPointerDown);
      node.removeEventListener("pointermove", onPointerMove);
      node.removeEventListener("pointerup", onPointerUp);
      resizeObserver?.disconnect();
      if (handle) handle.style.cursor = "";
    },
  };
}

// ── Resizable action ─────────────────────────────────────────────────

export interface ResizableOptions {
  minWidth: number;
  minHeight: number;
  /** Called once when a resize gesture starts, with the current computed size. */
  onResizeStart?: (currentSize: { w: number; h: number }) => void;
  /** Called on every pointer move during resize. posDelta is the total transform
   *  compensation needed for right/bottom edge resizing (keeps opposite edge fixed). */
  onResize?: (
    size: { w: number; h: number },
    posDelta: { dx: number; dy: number },
  ) => void;
  onResizeEnd?: () => void;
}

type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

const RESIZE_CURSORS: Record<ResizeEdge, string> = {
  n: "ns-resize",
  s: "ns-resize",
  e: "ew-resize",
  w: "ew-resize",
  ne: "nesw-resize",
  nw: "nwse-resize",
  se: "nwse-resize",
  sw: "nesw-resize",
};

/**
 * Svelte action: adds invisible edge/corner resize handles to an element.
 * Handles all 8 directions. For a bottom-right anchored element, resizing
 * from the right or bottom edge reports a position delta so the parent can
 * compensate via transform to keep the opposite edge stationary.
 * Caps size to parent bounds on window resize.
 */
export function resizable(node: HTMLElement, options: ResizableOptions) {
  const EDGE = 5; // edge handle thickness
  const CORNER = 10; // corner handle size
  const PAD = 8; // min gap from parent edges
  const handleEls: HTMLElement[] = [];

  let active: ResizeEdge | null = null;
  let startMX = 0;
  let startMY = 0;
  let startW = 0;
  let startH = 0;
  let maxW = Infinity;
  let maxH = Infinity;

  function mkHandle(edge: ResizeEdge) {
    const el = document.createElement("div");
    const s = el.style;
    s.position = "absolute";
    s.zIndex = "2";
    s.cursor = RESIZE_CURSORS[edge];
    s.touchAction = "none";

    const isCorner = edge.length === 2;
    if (isCorner) {
      s.width = s.height = `${CORNER}px`;
      if (edge.includes("n")) s.top = "0";
      if (edge.includes("s")) s.bottom = "0";
      if (edge.includes("w")) s.left = "0";
      if (edge.includes("e")) s.right = "0";
    } else if (edge === "n" || edge === "s") {
      s[edge === "n" ? "top" : "bottom"] = "0";
      s.left = `${CORNER}px`;
      s.right = `${CORNER}px`;
      s.height = `${EDGE}px`;
    } else {
      s[edge === "w" ? "left" : "right"] = "0";
      s.top = `${CORNER}px`;
      s.bottom = `${CORNER}px`;
      s.width = `${EDGE}px`;
    }

    el.addEventListener("pointerdown", (e) => onDown(e, edge));
    node.appendChild(el);
    handleEls.push(el);
  }

  const ALL_EDGES: ResizeEdge[] = [
    "n",
    "s",
    "e",
    "w",
    "ne",
    "nw",
    "se",
    "sw",
  ];
  for (const edge of ALL_EDGES) mkHandle(edge);

  function onDown(e: PointerEvent, edge: ResizeEdge) {
    e.preventDefault();
    e.stopPropagation();
    active = edge;
    startMX = e.clientX;
    startMY = e.clientY;

    const r = node.getBoundingClientRect();
    startW = r.width;
    startH = r.height;

    const p = node.offsetParent as HTMLElement;
    if (p) {
      const pr = p.getBoundingClientRect();
      maxW = pr.width - PAD * 2;
      maxH = pr.height - PAD * 2;
    }

    options.onResizeStart?.({ w: Math.round(startW), h: Math.round(startH) });
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!active) return;
    const dX = e.clientX - startMX;
    const dY = e.clientY - startMY;

    let w = startW;
    let h = startH;
    if (active.includes("w")) w = startW - dX;
    if (active.includes("e")) w = startW + dX;
    if (active.includes("n")) h = startH - dY;
    if (active.includes("s")) h = startH + dY;

    w = Math.round(Math.max(options.minWidth, Math.min(maxW, w)));
    h = Math.round(Math.max(options.minHeight, Math.min(maxH, h)));

    // Position compensation for right/bottom edges (keeps opposite edge fixed)
    const posDx = active.includes("e") ? w - Math.round(startW) : 0;
    const posDy = active.includes("s") ? h - Math.round(startH) : 0;

    node.style.width = `${w}px`;
    node.style.height = `${h}px`;
    options.onResize?.({ w, h }, { dx: posDx, dy: posDy });
  }

  function onUp(e: PointerEvent) {
    if (!active) return;
    active = null;
    try {
      (e.target as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
    options.onResizeEnd?.();
  }

  node.addEventListener("pointermove", onMove);
  node.addEventListener("pointerup", onUp);

  // Cap size when parent shrinks
  let ro: ResizeObserver | undefined;
  const parentEl = node.offsetParent as HTMLElement;
  if (parentEl) {
    ro = new ResizeObserver(() => {
      if (active) return;
      // Only cap if size was explicitly set (user has resized)
      if (!node.style.width) return;
      const pr = parentEl.getBoundingClientRect();
      const mw = pr.width - PAD * 2;
      const mh = pr.height - PAD * 2;
      const nr = node.getBoundingClientRect();
      const cw = Math.round(
        Math.max(options.minWidth, Math.min(mw, nr.width)),
      );
      const ch = Math.round(
        Math.max(options.minHeight, Math.min(mh, nr.height)),
      );
      if (cw < Math.round(nr.width) || ch < Math.round(nr.height)) {
        node.style.width = `${cw}px`;
        node.style.height = `${ch}px`;
        options.onResize?.({ w: cw, h: ch }, { dx: 0, dy: 0 });
      }
    });
    ro.observe(parentEl);
  }

  return {
    update(newOpts: ResizableOptions) {
      options = newOpts;
    },
    destroy() {
      for (const el of handleEls) el.remove();
      node.removeEventListener("pointermove", onMove);
      node.removeEventListener("pointerup", onUp);
      ro?.disconnect();
    },
  };
}

// ── External links action ────────────────────────────────────────────

/** Svelte action: intercepts <a> clicks inside the node and opens them in the system browser. */
export function externalLinks(node: HTMLElement) {
  function handleClick(e: MouseEvent) {
    const anchor = (e.target as HTMLElement).closest("a");
    if (!anchor || !anchor.href) return;
    e.preventDefault();
    openUrl(anchor.href);
  }
  node.addEventListener("click", handleClick);
  return {
    destroy() {
      node.removeEventListener("click", handleClick);
    },
  };
}

const COPY_ICON = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>`;
const CHECK_ICON = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="20 6 9 17 4 12"/></svg>`;

/** Svelte action: adds a copy button to each <pre> code block inside the node. */
export function copyCodeBlocks(node: HTMLElement) {
  const timers: number[] = [];

  function addButtons() {
    node.querySelectorAll("pre").forEach((pre) => {
      if (pre.querySelector(".copy-code-btn")) return;
      pre.style.position = "relative";
      const btn = document.createElement("button");
      btn.className = "copy-code-btn";
      btn.innerHTML = COPY_ICON;
      btn.title = "Copy";
      btn.addEventListener("click", () => {
        const code = pre.querySelector("code")?.textContent ?? pre.textContent ?? "";
        navigator.clipboard.writeText(code);
        btn.innerHTML = CHECK_ICON;
        const t = window.setTimeout(() => { btn.innerHTML = COPY_ICON; }, 1500);
        timers.push(t);
      });
      pre.appendChild(btn);
    });
  }

  addButtons();

  return {
    update() { addButtons(); },
    destroy() {
      timers.forEach(clearTimeout);
      node.querySelectorAll(".copy-code-btn").forEach((b) => b.remove());
    },
  };
}

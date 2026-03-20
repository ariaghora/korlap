import { openUrl } from "@tauri-apps/plugin-opener";

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

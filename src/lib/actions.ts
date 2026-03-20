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

import type { SvelteMap } from "svelte/reactivity";
import { getPrStatus, getChangedFiles, checkBaseUpdates, type PrStatus } from "$lib/ipc";

// ── PR Status Cache (localStorage) ─────────────────────

const PR_CACHE_KEY = "korlap:pr-status-cache";

export function loadPrStatusCache(): Record<string, PrStatus> {
  try {
    const raw = localStorage.getItem(PR_CACHE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

export function savePrStatusCache(prStatusMap: SvelteMap<string, PrStatus>) {
  try {
    const obj: Record<string, PrStatus> = {};
    for (const [k, v] of prStatusMap) obj[k] = v;
    localStorage.setItem(PR_CACHE_KEY, JSON.stringify(obj));
  } catch {
    // localStorage full or unavailable — non-critical
  }
}

export function hydratePrStatusFromCache(workspaceIds: string[], prStatusMap: SvelteMap<string, PrStatus>) {
  const cache = loadPrStatusCache();
  for (const wsId of workspaceIds) {
    if (cache[wsId] && !prStatusMap.has(wsId)) {
      prStatusMap.set(wsId, cache[wsId]);
    }
  }
}

export function removePrStatusCacheEntry(wsId: string) {
  try {
    const cache = loadPrStatusCache();
    delete cache[wsId];
    localStorage.setItem(PR_CACHE_KEY, JSON.stringify(cache));
  } catch {
    // non-critical
  }
}

export function clearPrStatusCacheForRepo(workspaceIds: string[]) {
  try {
    const cache = loadPrStatusCache();
    for (const wsId of workspaceIds) delete cache[wsId];
    localStorage.setItem(PR_CACHE_KEY, JSON.stringify(cache));
  } catch {
    // non-critical
  }
}

// ── PR Status Refresh ────────────────────────────────────

// Only triggers reactivity when the status actually changed to avoid DOM thrash.
export async function refreshPrStatus(wsId: string, prStatusMap: SvelteMap<string, PrStatus>) {
  try {
    const pr = await getPrStatus(wsId);
    const prev = prStatusMap.get(wsId);
    if (
      prev &&
      prev.state === pr.state &&
      prev.checks === pr.checks &&
      prev.mergeable === pr.mergeable &&
      prev.number === pr.number &&
      prev.additions === pr.additions &&
      prev.deletions === pr.deletions &&
      prev.title === pr.title &&
      prev.ahead_by === pr.ahead_by
    ) {
      return; // No change — skip reactive update
    }
    prStatusMap.set(wsId, pr);
    savePrStatusCache(prStatusMap);
  } catch (e) {
    console.warn(`refreshPrStatus(${wsId}):`, e);
  }
}

export async function refreshChangeCounts(
  wsId: string,
  changeCounts: SvelteMap<string, { additions: number; deletions: number }>,
) {
  try {
    const files = await getChangedFiles(wsId);
    const adds = files.reduce((s, f) => s + f.additions, 0);
    const dels = files.reduce((s, f) => s + f.deletions, 0);
    const prev = changeCounts.get(wsId);
    if (prev && prev.additions === adds && prev.deletions === dels) {
      return; // No change — skip reactive update
    }
    changeCounts.set(wsId, { additions: adds, deletions: dels });
  } catch (e) {
    console.warn(`refreshChangeCounts(${wsId}):`, e);
  }
}

export async function refreshBaseUpdates(
  wsId: string,
  baseBehindByMap: SvelteMap<string, number>,
) {
  try {
    const status = await checkBaseUpdates(wsId);
    const prev = baseBehindByMap.get(wsId);
    if (prev === status.behind_by) return;
    baseBehindByMap.set(wsId, status.behind_by);
  } catch (e) {
    console.warn(`refreshBaseUpdates(${wsId}):`, e);
  }
}

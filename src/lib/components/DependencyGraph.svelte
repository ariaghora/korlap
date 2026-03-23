<script lang="ts">
  import type { WorkspaceInfo, PrStatus } from "$lib/ipc";
  import { X } from "lucide-svelte";

  interface TodoItem {
    id: string;
    repo_id: string;
    title: string;
    description: string;
    depends_on?: string[];
    ready?: boolean;
  }

  interface Props {
    todos: TodoItem[];
    workspaces: WorkspaceInfo[];
    prStatusMap: Map<string, PrStatus>;
    onClose: () => void;
  }

  let { todos, workspaces, prStatusMap, onClose }: Props = $props();

  // ── Graph node types ──────────────────────────────────
  type NodeStatus = "ready" | "blocked" | "in-progress" | "review" | "done";

  interface GraphNode {
    id: string; // todo id or source_todo_id
    label: string;
    status: NodeStatus;
    x: number;
    y: number;
  }

  interface GraphEdge {
    from: string;
    to: string;
  }

  // ── Build graph from todos + workspaces ───────────────
  const NODE_W = 180;
  const NODE_H = 44;
  const H_GAP = 80;
  const V_GAP = 24;

  function getWsStatus(ws: WorkspaceInfo): NodeStatus {
    const pr = prStatusMap.get(ws.id);
    if (pr?.state === "merged") return "done";
    if (pr?.state === "open") return "review";
    return "in-progress";
  }

  function isTodoBlocked(todo: TodoItem): boolean {
    if (!todo.depends_on || todo.depends_on.length === 0) return false;
    return todo.depends_on.some(depId => {
      if (todos.some(t => t.id === depId)) return true;
      const ws = workspaces.find(w => w.source_todo_id === depId);
      if (!ws) return false;
      const pr = prStatusMap.get(ws.id);
      return pr?.state !== "merged";
    });
  }

  // Collect all node IDs that participate in dependencies
  function buildGraph(): { nodes: GraphNode[]; edges: GraphEdge[] } {
    const nodeMap = new Map<string, { label: string; status: NodeStatus; deps: string[] }>();
    const edges: GraphEdge[] = [];

    // Add all todos
    for (const todo of todos) {
      nodeMap.set(todo.id, {
        label: todo.title,
        status: isTodoBlocked(todo) ? "blocked" : "ready",
        deps: todo.depends_on ?? [],
      });
    }

    // Add all workspaces that originated from tasks (have task_title)
    for (const ws of workspaces) {
      if (!ws.task_title) continue;
      const nodeId = ws.source_todo_id ?? `ws-${ws.id}`;
      // Don't overwrite if a todo with this ID still exists
      if (nodeMap.has(nodeId)) continue;
      nodeMap.set(nodeId, {
        label: ws.task_title,
        status: getWsStatus(ws),
        deps: [],
      });
    }

    // Build edges from depends_on
    for (const todo of todos) {
      if (!todo.depends_on) continue;
      for (const depId of todo.depends_on) {
        if (nodeMap.has(depId)) {
          edges.push({ from: depId, to: todo.id });
        }
      }
    }

    // Show all nodes — todos and task-originated workspaces
    const filteredIds = new Set<string>(nodeMap.keys());

    // Topological rank assignment (Kahn's algorithm)
    const inDegree = new Map<string, number>();
    const adj = new Map<string, string[]>();
    for (const id of filteredIds) {
      inDegree.set(id, 0);
      adj.set(id, []);
    }
    for (const e of edges) {
      if (!filteredIds.has(e.from) || !filteredIds.has(e.to)) continue;
      adj.get(e.from)!.push(e.to);
      inDegree.set(e.to, (inDegree.get(e.to) ?? 0) + 1);
    }

    const rank = new Map<string, number>();
    const queue: string[] = [];
    for (const [id, deg] of inDegree) {
      if (deg === 0) { queue.push(id); rank.set(id, 0); }
    }

    while (queue.length > 0) {
      const curr = queue.shift()!;
      const currRank = rank.get(curr) ?? 0;
      for (const next of adj.get(curr) ?? []) {
        const nextRank = Math.max(rank.get(next) ?? 0, currRank + 1);
        rank.set(next, nextRank);
        const newDeg = (inDegree.get(next) ?? 1) - 1;
        inDegree.set(next, newDeg);
        if (newDeg === 0) queue.push(next);
      }
    }

    // Handle nodes not reached (cycles) — assign max rank + 1
    const maxRank = Math.max(0, ...rank.values());
    for (const id of filteredIds) {
      if (!rank.has(id)) rank.set(id, maxRank + 1);
    }

    // Group by rank for layout
    const rankGroups = new Map<number, string[]>();
    for (const [id, r] of rank) {
      if (!rankGroups.has(r)) rankGroups.set(r, []);
      rankGroups.get(r)!.push(id);
    }

    // Position nodes
    const nodes: GraphNode[] = [];
    const sortedRanks = [...rankGroups.keys()].sort((a, b) => a - b);
    for (const r of sortedRanks) {
      const group = rankGroups.get(r)!;
      const colX = r * (NODE_W + H_GAP) + 40;
      const totalHeight = group.length * NODE_H + (group.length - 1) * V_GAP;
      const startY = Math.max(40, (400 - totalHeight) / 2);
      for (let i = 0; i < group.length; i++) {
        const id = group[i];
        const n = nodeMap.get(id);
        if (!n) continue;
        nodes.push({
          id,
          label: n.label,
          status: n.status,
          x: colX,
          y: startY + i * (NODE_H + V_GAP),
        });
      }
    }

    return { nodes, edges: edges.filter(e => filteredIds.has(e.from) && filteredIds.has(e.to)) };
  }

  let graph = $derived(buildGraph());

  // SVG dimensions
  let svgWidth = $derived(
    Math.max(600, graph.nodes.reduce((m, n) => Math.max(m, n.x + NODE_W + 40), 0))
  );
  let svgHeight = $derived(
    Math.max(400, graph.nodes.reduce((m, n) => Math.max(m, n.y + NODE_H + 40), 0))
  );

  // Edge path generation
  function edgePath(e: GraphEdge): string {
    const fromNode = graph.nodes.find(n => n.id === e.from);
    const toNode = graph.nodes.find(n => n.id === e.to);
    if (!fromNode || !toNode) return "";

    const x1 = fromNode.x + NODE_W;
    const y1 = fromNode.y + NODE_H / 2;
    const x2 = toNode.x;
    const y2 = toNode.y + NODE_H / 2;
    const cx = (x1 + x2) / 2;

    return `M ${x1} ${y1} C ${cx} ${y1}, ${cx} ${y2}, ${x2} ${y2}`;
  }

  function statusColor(status: NodeStatus): string {
    switch (status) {
      case "ready": return "var(--accent)";
      case "blocked": return "var(--text-dim)";
      case "in-progress": return "var(--accent)";
      case "review": return "var(--status-pr-open, var(--accent))";
      case "done": return "var(--diff-add)";
    }
  }

  function handleBackdrop(e: MouseEvent) {
    if ((e.target as HTMLElement).classList.contains("dep-graph-overlay")) onClose();
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="dep-graph-overlay" onclick={handleBackdrop}>
  <div class="dep-graph-panel">
    <div class="dep-graph-header">
      <h3>Task Dependencies</h3>
      <button class="close-btn" onclick={onClose}><X size={14} /></button>
    </div>

    {#if graph.nodes.length === 0}
      <div class="empty-state">No tasks to display</div>
    {:else}
      <div class="dep-graph-scroll">
        <svg width={svgWidth} height={svgHeight}>
          <defs>
            <marker
              id="arrowhead"
              markerWidth="8"
              markerHeight="6"
              refX="8"
              refY="3"
              orient="auto"
            >
              <polygon points="0 0, 8 3, 0 6" fill="var(--text-dim)" />
            </marker>
          </defs>

          <!-- Edges -->
          {#each graph.edges as edge}
            <path
              d={edgePath(edge)}
              fill="none"
              stroke="var(--text-dim)"
              stroke-width="1.5"
              stroke-opacity="0.5"
              marker-end="url(#arrowhead)"
            />
          {/each}

          <!-- Nodes -->
          {#each graph.nodes as node}
            <g transform="translate({node.x}, {node.y})">
              <rect
                width={NODE_W}
                height={NODE_H}
                rx="8"
                fill="var(--bg-card)"
                stroke={statusColor(node.status)}
                stroke-width={node.status === "blocked" ? "1" : "1.5"}
                stroke-dasharray={node.status === "blocked" ? "4 3" : "none"}
                opacity={node.status === "done" ? 0.6 : 1}
              />
              {#if node.status === "in-progress"}
                <rect
                  width={NODE_W}
                  height={NODE_H}
                  rx="8"
                  fill="none"
                  stroke={statusColor(node.status)}
                  stroke-width="1.5"
                  class="pulse-ring"
                />
              {/if}
              <text
                x={NODE_W / 2}
                y={NODE_H / 2 + 1}
                text-anchor="middle"
                dominant-baseline="middle"
                fill={node.status === "blocked" ? "var(--text-dim)" : "var(--text-primary)"}
                font-size="11"
                font-weight="600"
                font-family="var(--font-family, 'Space Grotesk', sans-serif)"
              >
                {node.label.length > 22 ? node.label.slice(0, 20) + "..." : node.label}
              </text>
              <text
                x={NODE_W / 2}
                y={NODE_H - 6}
                text-anchor="middle"
                fill={statusColor(node.status)}
                font-size="8"
                font-weight="700"
                font-family="var(--font-family, 'Space Grotesk', sans-serif)"
                letter-spacing="0.5"
              >
                {node.status.replace("-", " ").toUpperCase()}
              </text>
            </g>
          {/each}
        </svg>
      </div>
    {/if}
  </div>
</div>

<style>
  .dep-graph-overlay {
    position: fixed;
    inset: 0;
    z-index: 1050;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dep-graph-panel {
    background: var(--bg-base);
    border: 1px solid var(--border-light);
    border-radius: 12px;
    width: min(90vw, 900px);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
  }

  .dep-graph-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1rem;
    border-bottom: 1px solid var(--border-light);
  }

  .dep-graph-header h3 {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 700;
    color: var(--text-primary);
  }

  .close-btn {
    width: 24px;
    height: 24px;
    border-radius: 6px;
    background: transparent;
    border: 1px solid var(--border-light);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    cursor: pointer;
    padding: 0;
    transition: background 0.15s, color 0.15s;
  }

  .close-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .dep-graph-scroll {
    overflow: auto;
    padding: 1rem;
    flex: 1;
  }

  .dep-graph-scroll svg {
    display: block;
  }

  .empty-state {
    padding: 3rem;
    text-align: center;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  :global(.pulse-ring) {
    animation: dep-pulse 2s ease-in-out infinite;
  }

  @keyframes dep-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }
</style>

<script lang="ts">
  import { Zap, ChevronDown, ChevronUp, Loader2, GitPullRequestCreate, Eye, AlertTriangle, ListOrdered, GitMerge, MessageSquare, CircleX } from "lucide-svelte";

  export interface AutopilotEvent {
    id: string;
    time: number;
    type: "spawn" | "auto_answer" | "review_start" | "review_done" | "pr_created" | "conflict_resolve" | "prioritized" | "staging_rebuild" | "user_command" | "orchestrator_response" | "error";
    message: string;
    wsId?: string;
    wsName?: string;
  }

  interface Props {
    enabled: boolean;
    events: AutopilotEvent[];
    activeAgentCount: number;
    maxAgents: number;
    todoQueueLength: number;
    prioritizing: boolean;
    rebuildingStaging: boolean;
    onSendCommand: (command: string) => void;
    onCardClick: (wsId: string) => void;
  }

  let {
    enabled,
    events,
    activeAgentCount,
    maxAgents,
    todoQueueLength,
    prioritizing,
    rebuildingStaging,
    onSendCommand,
    onCardClick,
  }: Props = $props();

  let expanded = $state(false);
  let inputValue = $state("");
  let inputEl: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();

  function handleSubmit() {
    const cmd = inputValue.trim();
    if (!cmd) return;
    onSendCommand(cmd);
    inputValue = "";
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
    if (e.key === "Escape") {
      expanded = false;
    }
  }

  let statusText = $derived.by(() => {
    const parts: string[] = [];
    if (activeAgentCount > 0) parts.push(`${activeAgentCount} agent${activeAgentCount > 1 ? "s" : ""}`);
    if (todoQueueLength > 0) parts.push(`${todoQueueLength} queued`);
    if (prioritizing) parts.push("prioritizing…");
    if (rebuildingStaging) parts.push("rebuilding staging…");
    return parts.length > 0 ? parts.join(" · ") : "idle";
  });

  let lastEvent = $derived(events.length > 0 ? events[events.length - 1] : null);

  function eventIcon(type: AutopilotEvent["type"]) {
    switch (type) {
      case "spawn": return Zap;
      case "auto_answer": return MessageSquare;
      case "review_start": return Eye;
      case "review_done": return Eye;
      case "pr_created": return GitPullRequestCreate;
      case "conflict_resolve": return AlertTriangle;
      case "prioritized": return ListOrdered;
      case "staging_rebuild": return GitMerge;
      case "user_command": return MessageSquare;
      case "orchestrator_response": return MessageSquare;
      case "error": return CircleX;
      default: return Zap;
    }
  }

  function formatTime(ts: number): string {
    const d = new Date(ts);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  // Auto-scroll when new events arrive
  $effect(() => {
    if (events.length && listEl && expanded) {
      requestAnimationFrame(() => {
        listEl!.scrollTop = listEl!.scrollHeight;
      });
    }
  });
</script>

{#if enabled}
  <div class="autopilot-pill" class:expanded>
    {#if !expanded}
      <!-- Collapsed pill -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="pill-collapsed" onclick={() => { expanded = true; }}>
        <Zap size={12} class="pill-icon" />
        {#if lastEvent}
          <span class="pill-last-event">{lastEvent.message}</span>
          <span class="pill-status-dim">{statusText}</span>
        {:else}
          <span class="pill-status">{statusText}</span>
        {/if}
        <ChevronUp size={12} class="pill-chevron" />
      </div>
    {:else}
      <!-- Expanded card -->
      <div class="pill-expanded">
        <div class="pill-header">
          <Zap size={12} class="pill-icon" />
          <span class="pill-title">Autopilot</span>
          <span class="pill-status-small">{statusText}</span>
          <button class="pill-collapse-btn" onclick={() => { expanded = false; }} title="Collapse">
            <ChevronDown size={14} />
          </button>
        </div>

        <div class="pill-events" bind:this={listEl}>
          {#if events.length === 0}
            <div class="pill-empty">No activity yet</div>
          {:else}
            {#each events as event (event.id)}
              {@const Icon = eventIcon(event.type)}
              <div
                class="pill-event"
                class:is-error={event.type === "error"}
                class:is-user={event.type === "user_command"}
                class:is-response={event.type === "orchestrator_response"}
              >
                <Icon size={11} class="event-icon" />
                <span class="event-text">
                  {#if event.wsId}
                    <button class="event-ws-link" onclick={() => onCardClick(event.wsId!)}>{event.wsName ?? event.wsId}</button>:
                  {/if}
                  {event.message}
                </span>
                <span class="event-time">{formatTime(event.time)}</span>
              </div>
            {/each}
          {/if}
        </div>

        <div class="pill-input-area">
          <input
            bind:this={inputEl}
            bind:value={inputValue}
            class="pill-input"
            placeholder="Direct the orchestrator…"
            onkeydown={handleKeydown}
          />
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .autopilot-pill {
    position: fixed;
    bottom: 12px;
    right: 12px;
    z-index: 10;
  }

  /* ── Collapsed ──────────────────────────── */

  .pill-collapsed {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.65rem;
    background: var(--bg-card);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 20px;
    cursor: pointer;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    transition: border-color 0.15s;
  }

  .pill-collapsed:hover {
    border-color: var(--accent);
  }

  .pill-collapsed :global(.pill-icon) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .pill-status {
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .pill-last-event {
    font-size: 0.72rem;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 220px;
  }

  .pill-status-dim {
    font-size: 0.62rem;
    color: var(--text-dim);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .pill-collapsed :global(.pill-chevron) {
    color: var(--text-dim);
    flex-shrink: 0;
  }

  /* ── Expanded ──────────────────────────── */

  .autopilot-pill.expanded {
    width: 380px;
  }

  .pill-expanded {
    display: flex;
    flex-direction: column;
    background: var(--bg-card);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 12px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.4);
    max-height: 50vh;
    overflow: hidden;
  }

  .pill-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 0.65rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .pill-header :global(.pill-icon) {
    color: var(--accent);
    flex-shrink: 0;
  }

  .pill-title {
    font-size: 0.78rem;
    font-weight: 700;
    color: var(--accent);
  }

  .pill-status-small {
    flex: 1;
    font-size: 0.65rem;
    color: var(--text-dim);
    text-align: right;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pill-collapse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .pill-collapse-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  /* ── Events list ─────────────────────────── */

  .pill-events {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0.35rem 0;
    max-height: 300px;
  }

  .pill-empty {
    padding: 1.5rem;
    text-align: center;
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  .pill-event {
    display: flex;
    align-items: flex-start;
    gap: 0.35rem;
    padding: 0.25rem 0.65rem;
    font-size: 0.72rem;
    line-height: 1.4;
  }

  .pill-event :global(.event-icon) {
    flex-shrink: 0;
    margin-top: 0.1rem;
    color: var(--text-dim);
  }

  .pill-event.is-error :global(.event-icon) {
    color: var(--diff-del);
  }

  .pill-event.is-user :global(.event-icon) {
    color: var(--accent);
  }

  .pill-event.is-response :global(.event-icon) {
    color: var(--status-ok);
  }

  .event-text {
    flex: 1;
    color: var(--text-secondary);
    word-break: break-word;
  }

  .pill-event.is-error .event-text {
    color: var(--diff-del);
  }

  .pill-event.is-user .event-text {
    color: var(--text-primary);
    font-weight: 600;
  }

  .pill-event.is-response .event-text {
    color: var(--text-primary);
  }

  .event-ws-link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    font-family: inherit;
    font-size: inherit;
    font-weight: 600;
    cursor: pointer;
    text-decoration: underline;
    text-decoration-color: color-mix(in srgb, var(--accent) 30%, transparent);
  }

  .event-ws-link:hover {
    text-decoration-color: var(--accent);
  }

  .event-time {
    font-size: 0.6rem;
    color: var(--text-dim);
    flex-shrink: 0;
    font-family: var(--font-mono);
    margin-top: 0.1rem;
  }

  /* ── Input ─────────────────────────────── */

  .pill-input-area {
    padding: 0.45rem 0.55rem;
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }

  .pill-input {
    width: 100%;
    box-sizing: border-box;
    padding: 0.4rem 0.5rem;
    background: var(--bg-base);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.75rem;
    outline: none;
    transition: border-color 0.15s;
  }

  .pill-input:focus {
    border-color: var(--accent);
  }

  .pill-input::placeholder {
    color: var(--text-dim);
  }
</style>

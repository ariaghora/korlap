<script lang="ts">
  interface Props {
    running: number;
    review: number;
    todo: number;
    done: number;
  }

  let { running, review, todo, done }: Props = $props();

  let total = $derived(running + review + todo + done);
</script>

<footer class="activity-bar">
  {#if total > 0}
    <div class="segments">
      {#if running > 0}
        <div class="seg running" style="flex: {running}"></div>
      {/if}
      {#if review > 0}
        <div class="seg review" style="flex: {review}"></div>
      {/if}
      {#if todo > 0}
        <div class="seg todo" style="flex: {todo}"></div>
      {/if}
      {#if done > 0}
        <div class="seg done" style="flex: {done}"></div>
      {/if}
    </div>
  {:else}
    <div class="segments">
      <div class="seg empty"></div>
    </div>
  {/if}
  <div class="label">
    {#if total > 0}
      {#if running > 0}<span class="l-running">{running} running</span>{/if}
      {#if review > 0}{#if running > 0} · {/if}<span class="l-review">{review} review</span>{/if}
      {#if todo > 0}{#if running > 0 || review > 0} · {/if}<span class="l-todo">{todo} todo</span>{/if}
      {#if done > 0}{#if running > 0 || review > 0 || todo > 0} · {/if}<span class="l-done">{done} done</span>{/if}
    {:else}
      <span class="l-empty">no tasks</span>
    {/if}
  </div>
</footer>

<style>
  .activity-bar {
    flex-shrink: 0;
    background: var(--bg-sidebar);
    border-top: 1px solid var(--border);
  }

  .segments {
    display: flex;
    height: 3px;
  }

  .seg {
    min-width: 2px;
    transition: flex 0.3s ease;
  }

  .seg.running {
    background: var(--accent);
    animation: seg-pulse 2s ease-in-out infinite;
  }

  .seg.review {
    background: var(--accent);
  }

  .seg.todo {
    background: var(--text-muted);
  }

  .seg.done {
    background: var(--status-ok);
    opacity: 0.5;
  }

  .seg.empty {
    flex: 1;
    background: var(--border);
  }

  @keyframes seg-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }

  .label {
    padding: 0.2rem 0.75rem;
    font-size: 0.65rem;
    color: var(--text-dim);
    letter-spacing: 0.02em;
  }

  .l-running { color: var(--accent); }
  .l-review { color: var(--accent); opacity: 0.8; }
  .l-todo { color: var(--text-dim); }
  .l-done { color: var(--status-ok); opacity: 0.6; }
  .l-empty { color: var(--text-muted); }
</style>

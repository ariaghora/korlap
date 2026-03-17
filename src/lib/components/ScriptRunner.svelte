<script lang="ts">
  import { runScript, type ScriptEvent } from "$lib/ipc";

  interface Props {
    workspaceId: string;
  }

  let { workspaceId }: Props = $props();

  let command = $state("");
  let output = $state("");
  let running = $state(false);
  let exitCode = $state<number | null>(null);
  let outputEl: HTMLPreElement | undefined = $state();

  async function handleRun() {
    if (!command.trim() || running) return;
    const cmd = command.trim();
    output = "";
    exitCode = null;
    running = true;

    try {
      await runScript(workspaceId, cmd, (event: ScriptEvent) => {
        if (event.type === "output") {
          output += event.data;
          // Auto-scroll
          if (outputEl) {
            requestAnimationFrame(() => {
              outputEl!.scrollTop = outputEl!.scrollHeight;
            });
          }
        } else if (event.type === "exit") {
          exitCode = event.code;
          running = false;
        }
      });
    } catch (e) {
      output += `\nError: ${e}`;
      running = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleRun();
    }
  }
</script>

<div class="script-runner">
  <form class="script-input" onsubmit={(e) => { e.preventDefault(); handleRun(); }}>
    <span class="prompt">$</span>
    <input
      bind:value={command}
      onkeydown={handleKeydown}
      placeholder="run a command..."
      disabled={running}
    />
    {#if running}
      <span class="running-indicator">running</span>
    {:else}
      <button type="submit" class="run-btn" disabled={!command.trim()}>Run</button>
    {/if}
  </form>

  <pre class="script-output" bind:this={outputEl}>{#if output}{output}{:else if exitCode !== null}<span class="exit-msg">Process exited with code {exitCode}</span>{:else}<span class="placeholder">Output will appear here...</span>{/if}</pre>

  {#if exitCode !== null && output}
    <div class="exit-bar" class:success={exitCode === 0} class:failure={exitCode !== 0}>
      exit {exitCode}
    </div>
  {/if}
</div>

<style>
  .script-runner {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .script-input {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 0.75rem;
    border-bottom: 1px solid #1e1b18;
  }

  .prompt {
    color: #c8a97e;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.85rem;
    font-weight: 600;
  }

  .script-input input {
    flex: 1;
    background: transparent;
    border: none;
    color: #d4c5a9;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.82rem;
    outline: none;
  }

  .script-input input::placeholder {
    color: #4a4540;
  }

  .run-btn {
    padding: 0.2rem 0.5rem;
    background: #1e1b18;
    border: 1px solid #3a3530;
    border-radius: 4px;
    color: #8a7e6a;
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
  }

  .run-btn:hover:not(:disabled) {
    color: #d4c5a9;
    background: #3a3530;
  }

  .run-btn:disabled {
    opacity: 0.4;
  }

  .running-indicator {
    font-size: 0.7rem;
    color: #c8a97e;
    animation: pulse 2s ease-in-out infinite;
  }

  .script-output {
    flex: 1;
    overflow: auto;
    margin: 0;
    padding: 0.75rem;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.78rem;
    line-height: 1.5;
    color: #d4c5a9;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .placeholder {
    color: #4a4540;
  }

  .exit-msg {
    color: #6a6050;
  }

  .exit-bar {
    padding: 0.25rem 0.75rem;
    font-family: "SF Mono", "Fira Code", monospace;
    font-size: 0.72rem;
    border-top: 1px solid #1e1b18;
  }

  .exit-bar.success {
    color: #7e9e6b;
  }

  .exit-bar.failure {
    color: #c87e7e;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.5; }
  }
</style>

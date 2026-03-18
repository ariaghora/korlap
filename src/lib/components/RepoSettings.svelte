<script lang="ts">
  import {
    getRepoSettings, saveRepoSettings, type RepoSettings,
    listGhProfiles, setRepoProfile, type GhProfile,
  } from "$lib/ipc";
  import { onMount } from "svelte";
  import { ArrowLeft, Terminal, Bot, GitBranch, Trash2 } from "lucide-svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  interface Props {
    repoId: string;
    repoName: string;
    repoPath: string;
    currentProfile: string | null;
    onClose: () => void;
    onRemoveRepo: () => void;
  }

  let { repoId, repoName, repoPath, currentProfile, onClose, onRemoveRepo }: Props = $props();

  type Section = "scripts" | "agent" | "git";
  let activeSection = $state<Section>("scripts");
  let confirmingRemove = $state(false);

  let settings = $state<RepoSettings>({
    setup_script: "",
    run_script: "",
    remove_script: "",
    pr_message: "",
    default_thinking: false,
    default_plan: false,
  });
  let saveStatus = $state<"idle" | "saving" | "saved">("idle");
  let saveTimeout: ReturnType<typeof setTimeout> | undefined;
  let ghProfiles = $state<GhProfile[]>([]);
  let selectedProfile = $state<string | null>(currentProfile);

  onMount(() => {
    getRepoSettings(repoId).then((s) => { settings = s; }).catch(() => {});
    listGhProfiles().then((p) => { ghProfiles = p; }).catch(() => {});

    // ⌘, to close (standard macOS settings shortcut)
    function handleKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        onClose();
      }
    }
    window.addEventListener("keydown", handleKey);
    return () => window.removeEventListener("keydown", handleKey);
  });

  // Auto-save with debounce
  function scheduleAutosave() {
    clearTimeout(saveTimeout);
    saveTimeout = setTimeout(async () => {
      saveStatus = "saving";
      try {
        await saveRepoSettings(repoId, settings);
        saveStatus = "saved";
        setTimeout(() => { if (saveStatus === "saved") saveStatus = "idle"; }, 1500);
      } catch {
        saveStatus = "idle";
      }
    }, 600);
  }

  const sections: { id: Section; label: string; icon: typeof Terminal }[] = [
    { id: "scripts", label: "Scripts", icon: Terminal },
    { id: "agent", label: "Agent", icon: Bot },
    { id: "git", label: "Git", icon: GitBranch },
  ];
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="settings-page" onmousedown={(e) => {
  const target = e.target as HTMLElement;
  if (!target.closest('button, input, textarea, a, [role="button"]') && e.buttons === 1) {
    getCurrentWindow().startDragging();
  }
}}>
  <nav class="settings-nav">
    <button class="back-btn" onclick={onClose}>
      <ArrowLeft size={14} />
      <span>Back to app</span>
    </button>

    <div class="nav-group">
      {#each sections as section}
        <button
          class="nav-item"
          class:active={activeSection === section.id}
          onclick={() => (activeSection = section.id)}
        >
          <svelte:component this={section.icon} size={14} />
          {section.label}
        </button>
      {/each}
    </div>

    <div class="nav-footer">
      <span class="nav-repo-label">Repository</span>
      <span class="nav-repo-name">{repoName}</span>
      <button class="remove-repo-btn" onclick={() => (confirmingRemove = true)}>
        <Trash2 size={12} />
        Remove
      </button>
    </div>
  </nav>

  <main class="settings-main">
    {#if activeSection === "scripts"}
      <div class="section-header">
        <h1>Scripts</h1>
        <span class="autosave-status" class:visible={saveStatus !== "idle"}>
          {saveStatus === "saving" ? "Saving..." : "Saved"}
        </span>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Setup</span>
          <span class="setting-desc">Runs in each new workspace after creation</span>
        </div>
        <div class="script-field">
          <span class="script-prompt">$</span>
          <textarea
            bind:value={settings.setup_script}
            oninput={scheduleAutosave}
            placeholder="bun install"
            rows="2"
            spellcheck="false"
          ></textarea>
        </div>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Run</span>
          <span class="setting-desc">Starts when you click ▶ in the Scripts tab</span>
        </div>
        <div class="script-field">
          <span class="script-prompt">$</span>
          <textarea
            bind:value={settings.run_script}
            oninput={scheduleAutosave}
            placeholder="bun run dev"
            rows="2"
            spellcheck="false"
          ></textarea>
        </div>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Remove</span>
          <span class="setting-desc">Runs before a workspace is removed</span>
        </div>
        <div class="script-field">
          <span class="script-prompt">$</span>
          <textarea
            bind:value={settings.remove_script}
            oninput={scheduleAutosave}
            placeholder="optional cleanup command"
            rows="2"
            spellcheck="false"
          ></textarea>
        </div>
      </div>

      <div class="env-hint">
        <span class="env-hint-title">Available environment variables</span>
        <div class="env-vars">
          <code>KORLAP_WORKSPACE_NAME</code>
          <code>KORLAP_WORKSPACE_PATH</code>
          <code>KORLAP_ROOT_PATH</code>
          <code>KORLAP_DEFAULT_BRANCH</code>
        </div>
      </div>

    {:else if activeSection === "agent"}
      <div class="section-header">
        <h1>Agent</h1>
        <span class="autosave-status" class:visible={saveStatus !== "idle"}>
          {saveStatus === "saving" ? "Saving..." : "Saved"}
        </span>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Default modes</span>
          <span class="setting-desc">New workspaces start with these modes enabled</span>
        </div>
        <div class="toggle-group">
          <label class="toggle-row">
            <span class="toggle-label">Thinking</span>
            <button
              class="toggle-switch"
              class:on={settings.default_thinking}
              onclick={() => { settings.default_thinking = !settings.default_thinking; scheduleAutosave(); }}
              role="switch"
              aria-checked={settings.default_thinking}
            >
              <span class="toggle-knob"></span>
            </button>
          </label>
          <label class="toggle-row">
            <span class="toggle-label">Plan</span>
            <button
              class="toggle-switch"
              class:on={settings.default_plan}
              onclick={() => { settings.default_plan = !settings.default_plan; scheduleAutosave(); }}
              role="switch"
              aria-checked={settings.default_plan}
            >
              <span class="toggle-knob"></span>
            </button>
          </label>
        </div>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Create PR message</span>
          <span class="setting-desc">Custom prompt sent to the agent when creating a pull request. Leave empty to use the default.</span>
        </div>
        <textarea
          class="pr-message-field"
          bind:value={settings.pr_message}
          oninput={scheduleAutosave}
          placeholder={`The user likes the current state of the code.\n\nThere are {{file_count}} uncommitted changes.\nThe current branch is {{branch}}.\nThe target branch is origin/{{base_branch}}.\n\nFollow these steps to create a PR:\n- Run \`git diff\` to review uncommitted changes\n- Commit them with a descriptive message\n- Push to origin\n- Use \`gh pr create --base {{base_branch}}\` to create a PR. Keep the title under 80 characters.\n\nIf any step fails, explain the issue.`}
          rows="14"
          spellcheck="false"
        ></textarea>
      </div>

      <div class="env-hint">
        <span class="env-hint-title">Available template variables</span>
        <div class="env-vars">
          <code>{"{{branch}}"}</code>
          <code>{"{{base_branch}}"}</code>
          <code>{"{{file_count}}"}</code>
          <code>{"{{pr_template}}"}</code>
        </div>
        <p class="template-var-hint">
          <code>{"{{pr_template}}"}</code> inserts the repo's PR template (from <code>.github/pull_request_template.md</code>) if one exists.
        </p>
      </div>

    {:else if activeSection === "git"}
      <div class="section-header">
        <h1>Git</h1>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">GitHub account</span>
          <span class="setting-desc">Used for git push, PR creation, and API calls in this repo's workspaces</span>
        </div>
        {#if ghProfiles.length > 0}
          <div class="profile-list">
            <button
              class="profile-option"
              class:selected={selectedProfile === null}
              onclick={async () => {
                selectedProfile = null;
                await setRepoProfile(repoId, null);
              }}
            >
              <span class="profile-name">None</span>
              <span class="profile-hint">Use default SSH key</span>
            </button>
            {#each ghProfiles as profile}
              <button
                class="profile-option"
                class:selected={selectedProfile === profile.login}
                onclick={async () => {
                  selectedProfile = profile.login;
                  await setRepoProfile(repoId, profile.login);
                }}
              >
                <span class="profile-name">{profile.login}</span>
                {#if profile.active}
                  <span class="profile-active">active in gh</span>
                {/if}
              </button>
            {/each}
          </div>
          <p class="profile-note">
            When a profile is selected, git operations use HTTPS with that account's token instead of SSH.
          </p>
        {:else}
          <p class="coming-soon">No GitHub accounts found. Run <code>gh auth login</code> to add one.</p>
        {/if}
      </div>
    {/if}
  </main>

  {#if confirmingRemove}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="confirm-overlay" onmousedown={() => (confirmingRemove = false)}>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="confirm-dialog" onmousedown={(e) => e.stopPropagation()}>
        <h2>Remove repository?</h2>
        <p>This will remove <strong>{repoName}</strong> from Korlap and delete all its workspaces. The repository itself won't be deleted.</p>
        <p class="confirm-path">{repoPath}</p>
        <div class="confirm-actions">
          <button class="confirm-cancel" onclick={() => (confirmingRemove = false)}>Cancel</button>
          <button class="confirm-remove" onclick={onRemoveRepo}>Remove</button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .settings-page {
    position: fixed;
    inset: 0;
    display: flex;
    background: var(--bg-base);
    z-index: 100;
  }

  /* ── Nav ──────────────────────────────── */

  .settings-nav {
    width: 200px;
    background: var(--bg-sidebar);
    border-right: 1px solid var(--border);
    padding: 0.75rem 0;
    padding-top: 2.75rem; /* clear macOS traffic lights */
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  .back-btn {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.5rem 0.85rem;
    margin-bottom: 1.25rem;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.78rem;
  }

  .back-btn:hover {
    color: var(--text-bright);
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    width: 100%;
    text-align: left;
    padding: 0.42rem 0.85rem;
    background: none;
    border: none;
    color: var(--text-secondary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.8rem;
  }

  .nav-item:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .nav-item.active {
    background: var(--border);
    color: var(--text-bright);
  }

  .nav-footer {
    padding: 0.75rem 0.85rem 0;
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .nav-repo-label {
    font-size: 0.65rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .nav-repo-name {
    font-size: 0.78rem;
    color: var(--text-dim);
  }

  /* ── Content ─────────────────────────── */

  .settings-main {
    flex: 1;
    padding: 2rem 2.5rem;
    overflow-y: auto;
    max-width: 580px;
  }

  .section-header {
    display: flex;
    align-items: baseline;
    gap: 0.75rem;
    margin-bottom: 2rem;
  }

  .section-header h1 {
    margin: 0;
    font-size: 1.3rem;
    font-weight: 600;
    color: var(--text-bright);
  }

  .autosave-status {
    font-size: 0.72rem;
    color: var(--status-ok);
    opacity: 0;
    transition: opacity 0.2s;
  }

  .autosave-status.visible {
    opacity: 1;
  }

  /* ── Setting blocks ──────────────────── */

  .setting-block {
    margin-bottom: 1.75rem;
  }

  .setting-meta {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    margin-bottom: 0.5rem;
  }

  .setting-name {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .setting-desc {
    font-size: 0.73rem;
    color: var(--text-dim);
  }

  .script-field {
    display: flex;
    align-items: flex-start;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }

  .script-prompt {
    padding: 0.5rem 0 0.5rem 0.65rem;
    color: var(--accent);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    font-weight: 600;
    user-select: none;
    line-height: 1.5;
  }

  .script-field textarea {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    line-height: 1.5;
    padding: 0.5rem 0.5rem;
    resize: none;
    outline: none;
  }

  .script-field textarea::placeholder {
    color: var(--text-muted);
  }

  .script-field:focus-within {
    border-color: var(--border-light);
  }

  /* ── Env hint ────────────────────────── */

  .env-hint {
    margin-top: 2.5rem;
    padding-top: 1.5rem;
    border-top: 1px solid var(--border);
  }

  .env-hint-title {
    font-size: 0.72rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    display: block;
    margin-bottom: 0.5rem;
  }

  .env-vars {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .env-vars code {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border);
    padding: 0.2rem 0.45rem;
    border-radius: 4px;
  }

  .coming-soon {
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  .coming-soon code {
    font-family: var(--font-mono);
    background: var(--bg-card);
    border: 1px solid var(--border);
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
    font-size: 0.78rem;
  }

  /* ── Toggle switches ─────────────── */

  .toggle-group {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.5rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .toggle-label {
    font-size: 0.82rem;
    color: var(--text-primary);
    font-weight: 500;
  }

  .toggle-switch {
    position: relative;
    width: 36px;
    height: 20px;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 10px;
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s;
    padding: 0;
  }

  .toggle-switch.on {
    background: color-mix(in srgb, var(--accent) 30%, var(--bg-card));
    border-color: var(--accent);
  }

  .toggle-knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    background: var(--text-dim);
    border-radius: 50%;
    transition: transform 0.15s, background 0.15s;
  }

  .toggle-switch.on .toggle-knob {
    transform: translateX(16px);
    background: var(--accent);
  }

  .pr-message-field {
    width: 100%;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 0.8rem;
    line-height: 1.5;
    padding: 0.6rem 0.65rem;
    resize: vertical;
    outline: none;
    box-sizing: border-box;
  }

  .pr-message-field::placeholder {
    color: var(--text-muted);
  }

  .pr-message-field:focus {
    border-color: var(--border-light);
  }

  .template-var-hint {
    margin-top: 0.5rem;
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  .template-var-hint code {
    font-family: var(--font-mono);
    font-size: 0.72rem;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border);
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
  }

  .profile-list {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .profile-option {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    text-align: left;
    padding: 0.55rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
  }

  .profile-option:hover {
    border-color: var(--border-light);
  }

  .profile-option.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 5%, var(--bg-sidebar));
  }

  .profile-name {
    font-weight: 600;
  }

  .profile-hint {
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  .profile-active {
    font-size: 0.65rem;
    color: var(--status-ok);
    background: color-mix(in srgb, var(--status-ok) 10%, transparent);
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
  }

  .profile-note {
    margin-top: 0.5rem;
    font-size: 0.72rem;
    color: var(--text-dim);
  }

  /* ── Remove repo button ─────────────── */

  .remove-repo-btn {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    margin-top: 0.5rem;
    padding: 0.35rem 0;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.72rem;
  }

  .remove-repo-btn:hover {
    color: var(--error, #c87e7e);
  }

  /* ── Confirmation dialog ────────────── */

  .confirm-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 200;
  }

  .confirm-dialog {
    background: var(--bg-sidebar);
    border: 1px solid var(--border-light);
    border-radius: 10px;
    padding: 1.5rem;
    max-width: 380px;
    width: 90%;
  }

  .confirm-dialog h2 {
    margin: 0 0 0.5rem;
    font-size: 1rem;
    font-weight: 600;
    color: var(--text-bright);
  }

  .confirm-dialog p {
    margin: 0 0 0.5rem;
    font-size: 0.82rem;
    color: var(--text-secondary);
    line-height: 1.45;
  }

  .confirm-path {
    font-family: var(--font-mono);
    font-size: 0.72rem !important;
    color: var(--text-dim) !important;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.3rem 0.5rem;
    word-break: break-all;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }

  .confirm-cancel {
    padding: 0.4rem 0.85rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
  }

  .confirm-cancel:hover {
    background: var(--bg-hover);
  }

  .confirm-remove {
    padding: 0.4rem 0.85rem;
    background: color-mix(in srgb, var(--error, #c87e7e) 15%, var(--bg-card));
    border: 1px solid color-mix(in srgb, var(--error, #c87e7e) 40%, transparent);
    border-radius: 6px;
    color: var(--error, #c87e7e);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.82rem;
    font-weight: 600;
  }

  .confirm-remove:hover {
    background: color-mix(in srgb, var(--error, #c87e7e) 25%, var(--bg-card));
  }
</style>

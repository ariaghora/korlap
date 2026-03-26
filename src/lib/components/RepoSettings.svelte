<script lang="ts">
  import {
    getRepoSettings, saveRepoSettings, type RepoSettings, type NamedScript, type LspServerConfig,
    getContextMeta, saveContextScope, buildKnowledgeBase, stopContextBuild,
    readContextFile, writeContextFile, draftContradictionResolution, resolveContradiction, updateKnowledgeBaseIncremental,
    lspStopServer, lspRestartServer,
    type ContextMeta, type ContextBuildStatus, type AgentEvent,
  } from "$lib/ipc";
  import { onMount } from "svelte";
  import { ArrowLeft, Terminal, Bot, Palette, BookOpen, Loader2, Pencil, Trash2, ChevronDown, Sun, Moon, Monitor, Braces, Plus, RotateCcw, Square, RefreshCw } from "lucide-svelte";
  import { tooltip } from "$lib/actions";
  import { themeList, getPreviewColors, type ThemeId } from "$lib/themes";
  import { getThemeId, setTheme, getColorMode, setColorMode, type ColorMode } from "$lib/stores/theme.svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  interface Props {
    repoId: string;
    repoName: string;
    repoPath: string;
    lspStatusMap?: Map<string, { status: string; message: string; repo_id: string }>;
    workspaceId?: string | null;
    onClose: () => void;
  }

  let { repoId, repoName, repoPath, lspStatusMap, workspaceId, onClose }: Props = $props();

  type Section = "scripts" | "agent" | "lsp" | "knowledge" | "appearance";
  let activeSection = $state<Section>("scripts");
  let currentThemeId = $state<ThemeId>(getThemeId());
  let currentColorMode = $state<ColorMode>(getColorMode());
  let settings = $state<RepoSettings>({
    setup_script: "",
    run_scripts: [],
    remove_script: "",
    pr_message: "",
    review_message: "",
    default_thinking: false,
    default_plan: false,
    system_prompt: "",
    lsp_servers: {},
  });

  function addRunScript() {
    settings.run_scripts = [...settings.run_scripts, { name: "", command: "" }];
    scheduleAutosave();
  }

  function removeRunScript(index: number) {
    settings.run_scripts = settings.run_scripts.filter((_, i) => i !== index);
    scheduleAutosave();
  }
  let saveStatus = $state<"idle" | "saving" | "saved">("idle");
  let saveTimeout: ReturnType<typeof setTimeout> | undefined;

  // ── Knowledge base state ──────────────────────────────────────────
  let contextMeta = $state<ContextMeta>({
    include_globs: [],
    exclude_globs: [],
    build_status: "not_built",
    last_built_at: null,
    invariant_count: 0,
    fact_count: 0,
    context_entry_count: 0,
    contradiction_count: 0,
    precheck_model: "",
    built_at_commit: null,
  });
  let includeGlobsText = $state("");
  let excludeGlobsText = $state("");
  let precheckModel = $state("");
  let buildActivity = $state("");
  let buildError = $state("");
  let contradictionsContent = $state("");
  let scopeSaveTimeout: ReturnType<typeof setTimeout> | undefined;

  // ── Review tab state ───────────────────────────────────────────────
  type ReviewTab = "invariants" | "facts" | "context" | "contradictions";
  let reviewTab = $state<ReviewTab>("invariants");
  let invariantsRaw = $state("");
  let factsRaw = $state("");
  let contextRaw = $state("");
  let showConfig = $state(false);

  let editingEntry = $state<{ file: string; idx: number } | null>(null);
  let editBuffer = $state("");

  let resolvingId = $state<string | null>(null);
  let resolveText = $state("");
  let resolveDirection = $state<"exception" | "update_invariant" | "tech_debt">("update_invariant");
  let draftResult = $state<string | null>(null);
  let draftLoading = $state(false);
  let draftError = $state("");

  onMount(() => {
    getRepoSettings(repoId).then((s) => { settings = s; }).catch(() => {});
    loadContextMeta();

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

  async function loadContextMeta() {
    try {
      contextMeta = await getContextMeta(repoId);
      includeGlobsText = contextMeta.include_globs.join("\n");
      excludeGlobsText = contextMeta.exclude_globs.join("\n");
      precheckModel = contextMeta.precheck_model || "claude-haiku-4-5-20251001";
      if (contextMeta.build_status === "built") {
        await loadContextFiles();
      }
    } catch { /* first load, defaults are fine */ }
  }

  async function loadContextFiles() {
    try {
      [invariantsRaw, factsRaw, contextRaw, contradictionsContent] = await Promise.all([
        readContextFile(repoId, "invariants.md"),
        readContextFile(repoId, "facts.md"),
        readContextFile(repoId, "context.md"),
        readContextFile(repoId, "contradictions.md"),
      ]);
    } catch { /* partial loads are fine */ }
  }

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

  // ── LSP state ──────────────────────────────────────────────────────
  const BUILTIN_LSP: Record<string, LspServerConfig> = {
    rust: { command: "rust-analyzer", args: [], extensions: ["rs"], detect_files: ["Cargo.toml"], language_id: "rust", install_hint: "rustup component add rust-analyzer", project_roots: [] },
    typescript: { command: "typescript-language-server", args: ["--stdio"], extensions: ["ts","tsx","js","jsx","mts","mjs","cts","cjs"], detect_files: ["tsconfig.json","package.json"], language_id: "typescript", install_hint: "bun i -g typescript-language-server typescript", project_roots: [] },
    go: { command: "gopls", args: ["serve"], extensions: ["go"], detect_files: ["go.mod"], language_id: "go", install_hint: "go install golang.org/x/tools/gopls@latest", project_roots: [] },
    python: { command: "pyright-langserver", args: ["--stdio"], extensions: ["py","pyi"], detect_files: ["pyproject.toml","requirements.txt","setup.py"], language_id: "python", install_hint: "pip install pyright", project_roots: [] },
  };

  let addingLsp = $state(false);
  let newLspId = $state("");
  let newLspConfig = $state<LspServerConfig>({ command: "", args: [], extensions: [], language_id: "", detect_files: [], install_hint: "", project_roots: [] });

  /** Merged view: built-in defaults + user overrides. */
  function getLspEntries(): { id: string; config: LspServerConfig; isOverride: boolean; isCustom: boolean }[] {
    const merged = { ...BUILTIN_LSP };
    const overrides = settings.lsp_servers ?? {};
    for (const [k, v] of Object.entries(overrides)) {
      merged[k] = v;
    }
    return Object.entries(merged).map(([id, config]) => ({
      id,
      config,
      isOverride: id in BUILTIN_LSP && id in overrides,
      isCustom: !(id in BUILTIN_LSP),
    }));
  }

  function removeLspServer(id: string) {
    const next = { ...settings.lsp_servers };
    delete next[id];
    settings.lsp_servers = next;
    scheduleAutosave();
  }

  function resetLspServer(id: string) {
    // Remove user override so built-in default takes effect
    removeLspServer(id);
  }

  function saveLspOverride(id: string, config: LspServerConfig) {
    settings.lsp_servers = { ...settings.lsp_servers, [id]: config };
    scheduleAutosave();
  }

  function addLspServer() {
    const id = newLspId.trim().toLowerCase().replace(/\s+/g, "-");
    if (!id || !newLspConfig.command.trim()) return;
    settings.lsp_servers = { ...settings.lsp_servers, [id]: { ...newLspConfig } };
    scheduleAutosave();
    addingLsp = false;
    newLspId = "";
    newLspConfig = { command: "", args: [], extensions: [], language_id: "", detect_files: [], install_hint: "", project_roots: [] };
  }

  let lspBusy = $state(new Set<string>());

  async function handleLspStop(serverId: string) {
    lspBusy = new Set([...lspBusy, serverId]);
    try {
      await lspStopServer(repoId, serverId);
    } catch (e) {
      console.error("Failed to stop LSP server:", e);
    } finally {
      const next = new Set(lspBusy);
      next.delete(serverId);
      lspBusy = next;
    }
  }

  async function handleLspRestart(serverId: string) {
    if (!workspaceId) return;
    lspBusy = new Set([...lspBusy, serverId]);
    try {
      await lspRestartServer(repoId, serverId, workspaceId);
    } catch (e) {
      console.error("Failed to restart LSP server:", e);
    } finally {
      const next = new Set(lspBusy);
      next.delete(serverId);
      lspBusy = next;
    }
  }

  function getLspStatus(serverId: string): { status: string; message: string } | null {
    if (!lspStatusMap) return null;
    const info = lspStatusMap.get(serverId);
    if (!info) return null;
    // Only show status for current repo
    if (info.repo_id && info.repo_id !== repoId) return null;
    return info;
  }

  function scheduleScopeSave() {
    clearTimeout(scopeSaveTimeout);
    scopeSaveTimeout = setTimeout(async () => {
      const includes = includeGlobsText.split("\n").map(s => s.trim()).filter(Boolean);
      const excludes = excludeGlobsText.split("\n").map(s => s.trim()).filter(Boolean);
      try {
        await saveContextScope(repoId, includes, excludes, precheckModel);
      } catch { /* silent */ }
    }, 600);
  }

  async function handleBuild() {
    buildActivity = "Starting build agent...";
    buildError = "";
    contextMeta.build_status = "building";

    try {
      await buildKnowledgeBase(repoId, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          if (event.tool_uses.length > 0) {
            const last = event.tool_uses[event.tool_uses.length - 1];
            const verb: Record<string, string> = {
              Read: "Reading", Glob: "Scanning", Grep: "Searching",
              Bash: "Running", Write: "Writing", Agent: "Dispatching subagent",
              Edit: "Editing", TodoWrite: "Planning", ListDirectory: "Listing",
            };
            const action = verb[last.name] ?? last.name;
            buildActivity = last.input_preview
              ? `${action}: ${last.input_preview}`
              : `${action}...`;
          } else if (event.text) {
            const preview = event.text.slice(0, 80).replace(/\n/g, " ");
            buildActivity = `Thinking: ${preview}${event.text.length > 80 ? "…" : ""}`;
          }
        } else if (event.type === "done") {
          loadContextMeta();
        } else if (event.type === "error") {
          buildError = event.message;
          contextMeta.build_status = "failed";
        }
      });
    } catch (e) {
      buildError = String(e);
      contextMeta.build_status = "failed";
    }
  }

  async function handleStopBuild() {
    try {
      await stopContextBuild(repoId);
      await loadContextMeta();
    } catch { /* silent */ }
  }

  async function handleUpdate() {
    buildActivity = "Checking for changes...";
    buildError = "";
    contextMeta.build_status = "building";

    try {
      await updateKnowledgeBaseIncremental(repoId, (event: AgentEvent) => {
        if (event.type === "assistant_message") {
          if (event.tool_uses.length > 0) {
            const last = event.tool_uses[event.tool_uses.length - 1];
            const verb: Record<string, string> = {
              Read: "Reading", Glob: "Scanning", Grep: "Searching",
              Bash: "Running", Write: "Writing", Agent: "Dispatching subagent",
              Edit: "Editing", TodoWrite: "Planning", ListDirectory: "Listing",
            };
            const action = verb[last.name] ?? last.name;
            buildActivity = last.input_preview
              ? `${action}: ${last.input_preview}`
              : `${action}...`;
          } else if (event.text) {
            const preview = event.text.slice(0, 80).replace(/\n/g, " ");
            buildActivity = `Thinking: ${preview}${event.text.length > 80 ? "…" : ""}`;
          }
        } else if (event.type === "done") {
          loadContextMeta();
        } else if (event.type === "error") {
          buildError = event.message;
          contextMeta.build_status = "built"; // revert — existing KB still valid
        }
      });
    } catch (e) {
      buildError = String(e);
      contextMeta.build_status = "built"; // revert — existing KB still valid
    }
  }

  function formatTimestamp(unix: number): string {
    return new Date(unix * 1000).toLocaleDateString(undefined, {
      month: "short", day: "numeric", year: "numeric",
      hour: "2-digit", minute: "2-digit",
    });
  }

  // ── Parsers ─────────────────────────────────────────────────────────

  interface KBEntry {
    id: string;
    title: string;
    content: string;
    fullText: string;
  }

  interface ContraEntry {
    id: string;
    title: string;
    content: string;
    resolved: boolean;
  }

  function parseInvariants(md: string): KBEntry[] {
    return md.split("\n")
      .filter(l => l.startsWith("- INV-"))
      .map(line => {
        const match = line.match(/^- (INV-\d+):\s*(.*)/);
        return {
          id: match?.[1] ?? "",
          title: match?.[2] ?? line.replace(/^- /, ""),
          content: "",
          fullText: line,
        };
      });
  }

  function parseFactSections(md: string): KBEntry[] {
    const entries: KBEntry[] = [];
    let current: KBEntry | null = null;
    for (const line of md.split("\n")) {
      if (line.startsWith("## ")) {
        if (current) { current.content = current.content.trimEnd(); current.fullText = current.fullText.trimEnd(); entries.push(current); }
        const title = line.replace("## ", "").trim();
        current = { id: title, title, content: "", fullText: line + "\n" };
      } else if (line.startsWith("# ")) {
        // skip top-level heading
      } else if (current) {
        current.content += line + "\n";
        current.fullText += line + "\n";
      }
    }
    if (current) { current.content = current.content.trimEnd(); current.fullText = current.fullText.trimEnd(); entries.push(current); }
    return entries;
  }

  function parseContextEntries(md: string): KBEntry[] {
    const entries: KBEntry[] = [];
    let current: KBEntry | null = null;
    for (const line of md.split("\n")) {
      if (line.startsWith("## ")) {
        if (current) { current.content = current.content.trimEnd(); current.fullText = current.fullText.trimEnd(); entries.push(current); }
        const title = line.replace("## ", "").trim();
        current = { id: title, title, content: "", fullText: line + "\n" };
      } else if (line.startsWith("# ")) {
        // skip
      } else if (current) {
        current.content += line + "\n";
        current.fullText += line + "\n";
      }
    }
    if (current) { current.content = current.content.trimEnd(); current.fullText = current.fullText.trimEnd(); entries.push(current); }
    return entries;
  }

  function parseContradictions(md: string): ContraEntry[] {
    const entries: ContraEntry[] = [];
    let current: ContraEntry | null = null;
    for (const line of md.split("\n")) {
      if (line.startsWith("## CONTRA-")) {
        if (current) entries.push(current);
        const rawId = line.replace("## ", "").replace(/\s*<!--.*-->/, "").trim();
        const titleMatch = rawId.match(/^(CONTRA-\d+):\s*(.*)/);
        current = {
          id: titleMatch?.[1] ?? rawId,
          title: titleMatch?.[2] ?? rawId,
          content: "",
          resolved: line.includes("<!-- RESOLVED") || line.includes("<!-- TECH_DEBT"),
        };
      } else if (current) {
        current.content += line + "\n";
      }
    }
    if (current) entries.push(current);
    return entries;
  }

  // ── Entry editing ──────────────────────────────────────────────────

  function startEdit(file: string, idx: number, fullText: string) {
    editingEntry = { file, idx };
    editBuffer = fullText;
  }

  function cancelEdit() {
    editingEntry = null;
    editBuffer = "";
  }

  async function saveEdit() {
    if (!editingEntry) return;
    const { file, idx } = editingEntry;
    const trimmed = editBuffer.trim();

    try {
      if (file === "invariants.md") {
        const entries = parseInvariants(invariantsRaw);
        entries[idx] = { ...entries[idx], fullText: trimmed };
        const rebuilt = "# Invariants\n\n" + entries.map(e => e.fullText).join("\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        invariantsRaw = rebuilt;
      } else if (file === "facts.md") {
        const entries = parseFactSections(factsRaw);
        entries[idx] = { ...entries[idx], fullText: trimmed };
        const rebuilt = "# Facts\n\n" + entries.map(e => e.fullText).join("\n\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        factsRaw = rebuilt;
      } else if (file === "context.md") {
        const entries = parseContextEntries(contextRaw);
        entries[idx] = { ...entries[idx], fullText: trimmed };
        const rebuilt = "# Context\n\n" + entries.map(e => e.fullText).join("\n\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        contextRaw = rebuilt;
      }
    } catch { /* silent */ }

    editingEntry = null;
    editBuffer = "";
    await loadContextMeta();
  }

  async function removeEntry(file: string, idx: number) {
    try {
      if (file === "invariants.md") {
        const entries = parseInvariants(invariantsRaw);
        entries.splice(idx, 1);
        const rebuilt = "# Invariants\n\n" + entries.map(e => e.fullText).join("\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        invariantsRaw = rebuilt;
      } else if (file === "facts.md") {
        const entries = parseFactSections(factsRaw);
        entries.splice(idx, 1);
        const rebuilt = "# Facts\n\n" + entries.map(e => e.fullText).join("\n\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        factsRaw = rebuilt;
      } else if (file === "context.md") {
        const entries = parseContextEntries(contextRaw);
        entries.splice(idx, 1);
        const rebuilt = "# Context\n\n" + entries.map(e => e.fullText).join("\n\n") + "\n";
        await writeContextFile(repoId, file, rebuilt);
        contextRaw = rebuilt;
      }
    } catch { /* silent */ }
    await loadContextMeta();
  }

  // ── Contradiction resolution ───────────────────────────────────────

  function startResolve(id: string) {
    resolvingId = id;
    resolveText = "";
    resolveDirection = "update_invariant";
    draftResult = null;
    draftLoading = false;
    draftError = "";
  }

  function cancelResolve() {
    resolvingId = null;
    resolveText = "";
    draftResult = null;
    draftLoading = false;
    draftError = "";
  }

  async function handleDraft(direction: "exception" | "update_invariant" | "tech_debt") {
    if (!resolvingId) return;
    resolveDirection = direction;
    draftLoading = true;
    draftError = "";
    draftResult = null;
    try {
      draftResult = await draftContradictionResolution(repoId, resolvingId, direction, resolveText.trim());
    } catch (e) {
      draftError = String(e);
    } finally {
      draftLoading = false;
    }
  }

  async function handleApplyDraft() {
    if (!resolvingId || !draftResult) return;
    try {
      await resolveContradiction(repoId, resolvingId, resolveDirection, draftResult);
      resolvingId = null;
      resolveText = "";
      draftResult = null;
      await loadContextMeta();
      await loadContextFiles();
    } catch (e) {
      draftError = String(e);
    }
  }

  const repoSections: { id: Section; label: string; icon: typeof Terminal }[] = [
    { id: "scripts", label: "Scripts", icon: Terminal },
    { id: "agent", label: "Agent", icon: Bot },
    { id: "lsp", label: "Language Servers", icon: Braces },
    { id: "knowledge", label: "Knowledge", icon: BookOpen },
  ];

  const globalSections: { id: Section; label: string; icon: typeof Terminal }[] = [
    { id: "appearance", label: "Appearance", icon: Palette },
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

    <div class="nav-groups">
      <div class="nav-group">
        <span class="nav-group-label">{repoName}</span>
        {#each repoSections as section}
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

      <div class="nav-group">
        <span class="nav-group-label">General</span>
        {#each globalSections as section}
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
    </div>
  </nav>

  <main class="settings-main">
    <div class="settings-content">
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
          <span class="setting-name">Run Scripts</span>
          <span class="setting-desc">Available from ▶ button in workspace. First script is the default.</span>
        </div>
        {#each settings.run_scripts as script, i}
          <div class="run-script-entry" class:is-default={i === 0}>
            <div class="run-script-entry-header">
              {#if i === 0}
                <span class="default-badge">Default</span>
              {/if}
              <input
                class="run-script-name-input"
                bind:value={script.name}
                oninput={scheduleAutosave}
                placeholder="Script name"
                spellcheck="false"
              />
              <button
                class="run-script-delete"
                onclick={() => removeRunScript(i)}
                use:tooltip={{ text: "Remove script" }}
              >
                <Trash2 size={13} />
              </button>
            </div>
            <div class="script-field">
              <span class="script-prompt">$</span>
              <textarea
                bind:value={script.command}
                oninput={scheduleAutosave}
                placeholder="bun run dev"
                rows="2"
                spellcheck="false"
              ></textarea>
            </div>
          </div>
        {/each}
        <button class="add-script-btn" onclick={addRunScript}>+ Add script</button>
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
          <span class="setting-name">System prompt</span>
          <span class="setting-desc">Custom instructions injected into every new conversation. Applied to all workspaces in this repo.</span>
        </div>
        <textarea
          class="system-prompt-field"
          bind:value={settings.system_prompt}
          oninput={scheduleAutosave}
          placeholder="- Never handwave errors or warnings&#10;- Debug log early to verify your assumption&#10;- Search the internet before guessing"
          rows="6"
          spellcheck="false"
        ></textarea>
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

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Review message</span>
          <span class="setting-desc">Extra instructions appended to the default review prompt. The built-in review (CLAUDE.md compliance, bug scanning, validation, inline PR comments) always runs.</span>
        </div>
        <textarea
          class="pr-message-field"
          bind:value={settings.review_message}
          oninput={scheduleAutosave}
          placeholder="e.g. Pay special attention to error handling in async functions. Flag any use of unwrap() outside tests."
          rows="14"
          spellcheck="false"
        ></textarea>
      </div>

      <div class="env-hint">
        <span class="env-hint-title">Available template variables</span>
        <div class="env-vars">
          <code>{"{{branch}}"}</code>
          <code>{"{{base_branch}}"}</code>
          <code>{"{{pr_number}}"}</code>
          <code>{"{{pr_title}}"}</code>
        </div>
      </div>

    {:else if activeSection === "lsp"}
      <div class="section-header">
        <h1>Language Servers</h1>
        <span class="autosave-status" class:visible={saveStatus !== "idle"}>
          {saveStatus === "saving" ? "Saving..." : "Saved"}
        </span>
      </div>

      <p class="section-desc">
        LSP servers give agents compiler-accurate code navigation — go-to-definition, find-references, hover, diagnostics. One shared server per language per repo, across all workspaces.
      </p>

      {#each getLspEntries() as { id, config, isOverride, isCustom }}
        {@const status = getLspStatus(id)}
        {@const isRunning = status?.status === "ready" || status?.status === "starting" || status?.status === "indexing"}
        {@const isBusy = lspBusy.has(id)}
        <div class="lsp-card">
          <div class="lsp-card-header">
            <span class="lsp-id">{id}</span>
            <div class="lsp-badges">
              {#if isOverride}
                <span class="lsp-badge override">override</span>
              {:else if isCustom}
                <span class="lsp-badge custom">custom</span>
              {:else}
                <span class="lsp-badge builtin">built-in</span>
              {/if}
              {#if status}
                <span class="lsp-badge status-{status.status}">{status.status}</span>
              {/if}
            </div>
            <div class="lsp-actions">
              {#if isRunning}
                <button class="lsp-action-btn" use:tooltip={{ text: "Stop server" }} onclick={() => handleLspStop(id)} disabled={isBusy}>
                  <Square size={10} />
                </button>
              {/if}
              {#if workspaceId}
                <button class="lsp-action-btn" use:tooltip={{ text: "Restart server" }} onclick={() => handleLspRestart(id)} disabled={isBusy}>
                  {#if isBusy}
                    <Loader2 size={12} class="lsp-spin" />
                  {:else}
                    <RefreshCw size={12} />
                  {/if}
                </button>
              {/if}
              {#if isOverride}
                <button class="lsp-action-btn" use:tooltip={{ text: "Reset to default" }} onclick={() => resetLspServer(id)}>
                  <RotateCcw size={12} />
                </button>
              {/if}
              {#if isCustom || isOverride}
                <button class="lsp-action-btn danger" use:tooltip={{ text: "Remove" }} onclick={() => removeLspServer(id)}>
                  <Trash2 size={12} />
                </button>
              {/if}
            </div>
          </div>

          <div class="lsp-fields">
            <label class="lsp-field">
              <span>Command</span>
              <input type="text" value={config.command} oninput={(e) => saveLspOverride(id, { ...config, command: (e.target as HTMLInputElement).value })} spellcheck="false" />
            </label>
            <label class="lsp-field">
              <span>Args</span>
              <input type="text" value={config.args.join(" ")} oninput={(e) => saveLspOverride(id, { ...config, args: (e.target as HTMLInputElement).value.split(" ").filter(Boolean) })} spellcheck="false" placeholder="--stdio" />
            </label>
            <label class="lsp-field">
              <span>Extensions</span>
              <input type="text" value={config.extensions.join(", ")} oninput={(e) => saveLspOverride(id, { ...config, extensions: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) })} spellcheck="false" placeholder="rs, ts, go" />
            </label>
            <label class="lsp-field">
              <span>Detect files</span>
              <input type="text" value={config.detect_files.join(", ")} oninput={(e) => saveLspOverride(id, { ...config, detect_files: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) })} spellcheck="false" placeholder="Cargo.toml, package.json" />
            </label>
            <label class="lsp-field">
              <span>Language ID</span>
              <input type="text" value={config.language_id} oninput={(e) => saveLspOverride(id, { ...config, language_id: (e.target as HTMLInputElement).value })} spellcheck="false" />
            </label>
            <label class="lsp-field">
              <span>Install hint</span>
              <input type="text" value={config.install_hint} oninput={(e) => saveLspOverride(id, { ...config, install_hint: (e.target as HTMLInputElement).value })} spellcheck="false" />
            </label>
            <label class="lsp-field">
              <span>Project roots</span>
              <input type="text" value={(config.project_roots ?? []).join(", ")} oninput={(e) => saveLspOverride(id, { ...config, project_roots: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) })} spellcheck="false" placeholder="auto-detect (leave empty)" />
            </label>
          </div>
        </div>
      {/each}

      {#if addingLsp}
        <div class="lsp-card adding">
          <div class="lsp-fields">
            <label class="lsp-field">
              <span>Server ID</span>
              <input type="text" bind:value={newLspId} spellcheck="false" placeholder="e.g. svelte, zig, elixir" />
            </label>
            <label class="lsp-field">
              <span>Command</span>
              <input type="text" bind:value={newLspConfig.command} spellcheck="false" placeholder="svelteserver" />
            </label>
            <label class="lsp-field">
              <span>Args</span>
              <input type="text" value={newLspConfig.args.join(" ")} oninput={(e) => newLspConfig = { ...newLspConfig, args: (e.target as HTMLInputElement).value.split(" ").filter(Boolean) }} spellcheck="false" placeholder="--stdio" />
            </label>
            <label class="lsp-field">
              <span>Extensions</span>
              <input type="text" value={newLspConfig.extensions.join(", ")} oninput={(e) => newLspConfig = { ...newLspConfig, extensions: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) }} spellcheck="false" placeholder="svelte" />
            </label>
            <label class="lsp-field">
              <span>Detect files</span>
              <input type="text" value={newLspConfig.detect_files.join(", ")} oninput={(e) => newLspConfig = { ...newLspConfig, detect_files: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) }} spellcheck="false" placeholder="svelte.config.js" />
            </label>
            <label class="lsp-field">
              <span>Language ID</span>
              <input type="text" bind:value={newLspConfig.language_id} spellcheck="false" placeholder="svelte" />
            </label>
            <label class="lsp-field">
              <span>Install hint</span>
              <input type="text" bind:value={newLspConfig.install_hint} spellcheck="false" placeholder="bun i -g svelte-language-server" />
            </label>
            <label class="lsp-field">
              <span>Project roots</span>
              <input type="text" value={newLspConfig.project_roots.join(", ")} oninput={(e) => newLspConfig = { ...newLspConfig, project_roots: (e.target as HTMLInputElement).value.split(",").map(s => s.trim()).filter(Boolean) }} spellcheck="false" placeholder="auto-detect (leave empty)" />
            </label>
          </div>
          <div class="lsp-add-actions">
            <button class="lsp-save-btn" onclick={addLspServer}>Add server</button>
            <button class="lsp-cancel-btn" onclick={() => { addingLsp = false; }}>Cancel</button>
          </div>
        </div>
      {:else}
        <button class="add-script-btn" onclick={() => { addingLsp = true; }}>
          <Plus size={13} /> Add language server
        </button>
      {/if}

    {:else if activeSection === "knowledge"}
      <div class="section-header">
        <h1>Knowledge base</h1>
        {#if contextMeta.build_status === "built"}
          <span class="kb-status kb-built">Built</span>
        {:else if contextMeta.build_status === "building"}
          <span class="kb-status kb-building"><Loader2 size={12} class="spin" /> Building</span>
        {:else if contextMeta.build_status === "failed"}
          <span class="kb-status kb-failed">Failed</span>
        {:else}
          <span class="kb-status kb-not-built">Not built</span>
        {/if}
      </div>

      <!-- Building progress -->
      {#if contextMeta.build_status === "building"}
        <div class="kb-building-activity">
          <div class="kb-activity-row">
            <Loader2 size={14} class="spin" />
            <span class="kb-activity-text">{buildActivity}</span>
          </div>
          <button class="kb-action-btn kb-cancel-btn" onclick={handleStopBuild}>Cancel</button>
        </div>
      {:else if contextMeta.build_status === "failed" && buildError}
        <div class="kb-error">
          <span>{buildError}</span>
        </div>
      {/if}

      <!-- Summary + actions when built -->
      {#if contextMeta.build_status === "built" && contextMeta.last_built_at}
        <div class="kb-summary-bar">
          <div class="kb-summary-left">
            <span class="kb-summary-counts">
              {contextMeta.invariant_count} invariants &middot;
              {contextMeta.fact_count} facts &middot;
              {contextMeta.context_entry_count} context entries
              {#if contextMeta.contradiction_count > 0}
                &middot; <span class="kb-contradictions-badge">{contextMeta.contradiction_count} contradictions</span>
              {/if}
            </span>
            <span class="kb-summary-date">
              {#if contextMeta.built_at_commit}
                Built at <code class="kb-commit-hash">{contextMeta.built_at_commit.slice(0, 7)}</code> on
              {:else}
                Last built
              {/if}
              {formatTimestamp(contextMeta.last_built_at)}
            </span>
          </div>
          <div class="kb-summary-actions">
            <button
              class="kb-action-btn kb-update-btn"
              onclick={handleUpdate}
              disabled={!contextMeta.built_at_commit}
              use:tooltip={{ text: contextMeta.built_at_commit ? "Incrementally update from changes since last build" : "No baseline commit — run a full build first" }}
            >Update</button>
            <button class="kb-action-btn" onclick={handleBuild}>Rebuild</button>
          </div>
        </div>

        <!-- Review tabs -->
        <div class="kb-tabs">
          {#each (["invariants", "facts", "context", "contradictions"] as ReviewTab[]) as tab}
            <button
              class="kb-tab"
              class:active={reviewTab === tab}
              onclick={() => { reviewTab = tab; cancelEdit(); }}
            >
              {tab === "invariants" ? `Invariants (${contextMeta.invariant_count})`
                : tab === "facts" ? `Facts (${contextMeta.fact_count})`
                : tab === "context" ? `Context (${contextMeta.context_entry_count})`
                : `Contradictions${contextMeta.contradiction_count > 0 ? ` (${contextMeta.contradiction_count})` : ""}`}
            </button>
          {/each}
        </div>

        <!-- Invariants tab -->
        {#if reviewTab === "invariants"}
          <div class="kb-entries">
            {#each parseInvariants(invariantsRaw) as entry, idx}
              <div class="kb-entry-card">
                {#if editingEntry?.file === "invariants.md" && editingEntry.idx === idx}
                  <textarea class="kb-entry-edit" bind:value={editBuffer} rows="2" spellcheck="false"></textarea>
                  <div class="kb-entry-edit-actions">
                    <button class="kb-resolve-btn" onclick={saveEdit}>Save</button>
                    <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelEdit}>Cancel</button>
                  </div>
                {:else}
                  <div class="kb-entry-row">
                    <span class="kb-entry-id">{entry.id}</span>
                    <span class="kb-entry-title">{entry.title}</span>
                    <div class="kb-entry-actions">
                      <button class="kb-icon-btn" use:tooltip={{ text: "Edit" }} onclick={() => startEdit("invariants.md", idx, entry.fullText)}>
                        <Pencil size={12} />
                      </button>
                      <button class="kb-icon-btn kb-icon-danger" use:tooltip={{ text: "Remove" }} onclick={() => removeEntry("invariants.md", idx)}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </div>
                {/if}
              </div>
            {/each}
            {#if parseInvariants(invariantsRaw).length === 0}
              <div class="kb-empty">No invariants found.</div>
            {/if}
          </div>

        <!-- Facts tab -->
        {:else if reviewTab === "facts"}
          <div class="kb-entries">
            {#each parseFactSections(factsRaw) as entry, idx}
              <div class="kb-entry-card">
                {#if editingEntry?.file === "facts.md" && editingEntry.idx === idx}
                  <textarea class="kb-entry-edit" bind:value={editBuffer} rows="8" spellcheck="false"></textarea>
                  <div class="kb-entry-edit-actions">
                    <button class="kb-resolve-btn" onclick={saveEdit}>Save</button>
                    <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelEdit}>Cancel</button>
                  </div>
                {:else}
                  <div class="kb-entry-header">
                    <span class="kb-entry-section-title">{entry.title}</span>
                    <div class="kb-entry-actions">
                      <button class="kb-icon-btn" use:tooltip={{ text: "Edit" }} onclick={() => startEdit("facts.md", idx, entry.fullText)}>
                        <Pencil size={12} />
                      </button>
                      <button class="kb-icon-btn kb-icon-danger" use:tooltip={{ text: "Remove" }} onclick={() => removeEntry("facts.md", idx)}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </div>
                  <pre class="kb-entry-content">{entry.content.trim()}</pre>
                {/if}
              </div>
            {/each}
            {#if parseFactSections(factsRaw).length === 0}
              <div class="kb-empty">No facts found.</div>
            {/if}
          </div>

        <!-- Context tab -->
        {:else if reviewTab === "context"}
          <div class="kb-entries">
            {#each parseContextEntries(contextRaw) as entry, idx}
              <div class="kb-entry-card">
                {#if editingEntry?.file === "context.md" && editingEntry.idx === idx}
                  <textarea class="kb-entry-edit" bind:value={editBuffer} rows="8" spellcheck="false"></textarea>
                  <div class="kb-entry-edit-actions">
                    <button class="kb-resolve-btn" onclick={saveEdit}>Save</button>
                    <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelEdit}>Cancel</button>
                  </div>
                {:else}
                  <div class="kb-entry-header">
                    <span class="kb-entry-section-title">{entry.title}</span>
                    <div class="kb-entry-actions">
                      <button class="kb-icon-btn" use:tooltip={{ text: "Edit" }} onclick={() => startEdit("context.md", idx, entry.fullText)}>
                        <Pencil size={12} />
                      </button>
                      <button class="kb-icon-btn kb-icon-danger" use:tooltip={{ text: "Remove" }} onclick={() => removeEntry("context.md", idx)}>
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </div>
                  <pre class="kb-entry-content">{entry.content.trim()}</pre>
                {/if}
              </div>
            {/each}
            {#if parseContextEntries(contextRaw).length === 0}
              <div class="kb-empty">No context entries found.</div>
            {/if}
          </div>

        <!-- Contradictions tab -->
        {:else if reviewTab === "contradictions"}
          <div class="kb-entries">
            {#each parseContradictions(contradictionsContent) as contra}
              {#if !contra.resolved}
                <div class="kb-entry-card kb-contra-card">
                  <div class="kb-contra-header">
                    <span class="kb-entry-id">{contra.id}</span>
                    <span class="kb-contra-title">{contra.title}</span>
                  </div>
                  <pre class="kb-entry-content">{contra.content.trim()}</pre>

                  {#if resolvingId === contra.id}
                    <div class="kb-resolve-form">
                      {#if draftResult !== null}
                        <!-- Step 3: Review the LLM draft -->
                        <div class="kb-draft-header">
                          <span class="kb-draft-label">Drafted invariants update</span>
                          <span class="kb-draft-hint">Review and edit before applying</span>
                        </div>
                        <textarea
                          class="kb-entry-edit kb-draft-edit"
                          bind:value={draftResult}
                          rows="12"
                          spellcheck="false"
                        ></textarea>
                        {#if draftError}
                          <div class="kb-draft-error">{draftError}</div>
                        {/if}
                        <div class="kb-resolve-actions">
                          <button class="kb-resolve-btn kb-primary-resolve" onclick={handleApplyDraft}>
                            Apply changes
                          </button>
                          <button class="kb-resolve-btn" onclick={() => { draftResult = null; draftError = ""; }}>
                            Back
                          </button>
                          <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelResolve}>
                            Cancel
                          </button>
                        </div>
                      {:else if draftLoading}
                        <!-- Step 2: LLM is drafting -->
                        <div class="kb-draft-loading">
                          <Loader2 size={14} class="spin" />
                          <span>Drafting resolution...</span>
                        </div>
                        <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelResolve}>
                          Cancel
                        </button>
                      {:else}
                        <!-- Step 1: User describes intent + picks direction -->
                        <textarea
                          class="kb-entry-edit"
                          bind:value={resolveText}
                          rows="3"
                          spellcheck="false"
                          placeholder="Describe how this should be resolved (optional for tech debt)"
                        ></textarea>
                        {#if draftError}
                          <div class="kb-draft-error">{draftError}</div>
                        {/if}
                        <div class="kb-resolve-actions">
                          <button class="kb-resolve-btn" onclick={() => handleDraft("exception")}>
                            Both valid — add exception
                          </button>
                          <button class="kb-resolve-btn" onclick={() => handleDraft("update_invariant")}>
                            Update invariant
                          </button>
                          <button class="kb-resolve-btn kb-resolve-debt" onclick={() => handleDraft("tech_debt")}>
                            Tech debt
                          </button>
                          <button class="kb-resolve-btn kb-resolve-debt" onclick={cancelResolve}>
                            Cancel
                          </button>
                        </div>
                      {/if}
                    </div>
                  {:else}
                    <div class="kb-contradiction-actions">
                      <button class="kb-resolve-btn kb-primary-resolve" onclick={() => startResolve(contra.id)}>
                        Resolve
                      </button>
                    </div>
                  {/if}
                </div>
              {/if}
            {/each}
            {#if parseContradictions(contradictionsContent).filter(c => !c.resolved).length === 0}
              <div class="kb-empty">No unresolved contradictions.</div>
            {/if}
          </div>
        {/if}
      {/if}

      <!-- Config (collapsible when built, always shown when not built) -->
      {#if contextMeta.build_status !== "built"}
        <div class="setting-block">
          <div class="setting-meta">
            <span class="setting-name">Scope</span>
            <span class="setting-desc">Glob patterns to include or exclude from analysis. One pattern per line.</span>
          </div>
          <div class="kb-scope-fields">
            <div class="kb-scope-field">
              <label class="kb-scope-label">Include</label>
              <textarea
                bind:value={includeGlobsText}
                oninput={scheduleScopeSave}
                placeholder="src/**&#10;lib/**"
                rows="3"
                spellcheck="false"
                class="system-prompt-field"
              ></textarea>
            </div>
            <div class="kb-scope-field">
              <label class="kb-scope-label">Exclude</label>
              <textarea
                bind:value={excludeGlobsText}
                oninput={scheduleScopeSave}
                placeholder="dist&#10;generated&#10;vendor&#10;node_modules&#10;*.test.*"
                rows="3"
                spellcheck="false"
                class="system-prompt-field"
              ></textarea>
            </div>
          </div>
        </div>

        <div class="setting-block">
          <div class="setting-meta">
            <span class="setting-name">Pre-check model</span>
            <span class="setting-desc">Model used for invariant pre-checks before review. Use a fast, cheap model.</span>
          </div>
          <textarea
            class="system-prompt-field"
            bind:value={precheckModel}
            oninput={scheduleScopeSave}
            placeholder="claude-haiku-4-5-20251001"
            rows="1"
            spellcheck="false"
          ></textarea>
        </div>

        <div class="kb-actions">
          {#if contextMeta.build_status === "building"}
            <!-- actions hidden while building -->
          {:else}
            <button class="kb-action-btn kb-primary-btn" onclick={handleBuild}>Build knowledge base</button>
          {/if}
        </div>
      {:else}
        <!-- Collapsible config when built -->
        <button class="kb-config-toggle" onclick={() => showConfig = !showConfig}>
          <ChevronDown size={14} class={showConfig ? "chevron-open" : "chevron-closed"} />
          <span>Build settings</span>
        </button>
        {#if showConfig}
          <div class="kb-config-panel">
            <div class="kb-scope-fields">
              <div class="kb-scope-field">
                <label class="kb-scope-label">Include</label>
                <textarea
                  bind:value={includeGlobsText}
                  oninput={scheduleScopeSave}
                  placeholder="src/**&#10;lib/**"
                  rows="3"
                  spellcheck="false"
                  class="system-prompt-field"
                ></textarea>
              </div>
              <div class="kb-scope-field">
                <label class="kb-scope-label">Exclude</label>
                <textarea
                  bind:value={excludeGlobsText}
                  oninput={scheduleScopeSave}
                  placeholder="dist&#10;generated&#10;vendor&#10;node_modules&#10;*.test.*"
                  rows="3"
                  spellcheck="false"
                  class="system-prompt-field"
                ></textarea>
              </div>
            </div>
            <div class="setting-block" style="margin-top: 0.75rem">
              <div class="setting-meta">
                <span class="setting-name">Pre-check model</span>
                <span class="setting-desc">Model used for invariant pre-checks before review.</span>
              </div>
              <textarea
                class="system-prompt-field"
                bind:value={precheckModel}
                oninput={scheduleScopeSave}
                placeholder="claude-haiku-4-5-20251001"
                rows="1"
                spellcheck="false"
              ></textarea>
            </div>
          </div>
        {/if}
      {/if}

    {:else if activeSection === "appearance"}
      <div class="section-header">
        <h1>Appearance</h1>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Theme</span>
          <span class="setting-desc">Applied globally across all workspaces</span>
        </div>
        <div class="theme-grid">
          {#each themeList as theme}
            <button
              class="theme-card"
              class:selected={currentThemeId === theme.id}
              onclick={() => {
                currentThemeId = theme.id;
                setTheme(theme.id);
              }}
            >
              <div class="theme-preview" style:grid-template-columns="repeat({Math.ceil(getPreviewColors(theme).length / 3)}, 1fr)">
                {#each getPreviewColors(theme) as color}
                  <div class="theme-swatch" style:background={color}></div>
                {/each}
              </div>
              <span class="theme-name">{theme.name}</span>
            </button>
          {/each}
        </div>
      </div>

      <div class="setting-block">
        <div class="setting-meta">
          <span class="setting-name">Color mode</span>
          <span class="setting-desc">Override system dark/light preference</span>
        </div>
        <div class="color-mode-picker">
          {#each ([
            { mode: "light" as ColorMode, icon: Sun, label: "Light" },
            { mode: "dark" as ColorMode, icon: Moon, label: "Dark" },
            { mode: "system" as ColorMode, icon: Monitor, label: "System" },
          ]) as opt}
            <button
              class="color-mode-btn"
              class:active={currentColorMode === opt.mode}
              onclick={() => {
                currentColorMode = opt.mode;
                setColorMode(opt.mode);
              }}
            >
              <svelte:component this={opt.icon} size={14} />
              <span>{opt.label}</span>
            </button>
          {/each}
        </div>
      </div>
    {/if}
    </div>
  </main>

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

  .nav-groups {
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    flex: 1;
  }

  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .nav-group-label {
    font-size: 0.65rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding: 0 0.85rem 0.35rem;
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

  /* ── Content ─────────────────────────── */

  .settings-main {
    flex: 1;
    padding: 2rem 2.5rem;
    overflow-y: auto;
    display: flex;
    justify-content: center;
  }

  .settings-content {
    width: 100%;
    max-width: 640px;
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

  /* ── Run script list editor ──────────── */

  .run-script-entry {
    margin-bottom: 0.6rem;
    padding: 0.55rem;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg-sidebar);
  }

  .run-script-entry.is-default {
    border-color: color-mix(in srgb, var(--accent) 30%, var(--border));
  }

  .run-script-entry-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin-bottom: 0.35rem;
  }

  .default-badge {
    font-size: 0.6rem;
    font-weight: 600;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    padding: 0.1rem 0.35rem;
    border-radius: 3px;
    flex-shrink: 0;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .run-script-name-input {
    flex: 1;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
    font-size: 0.8rem;
    font-weight: 500;
    padding: 0.2rem 0;
    outline: none;
    font-family: inherit;
  }

  .run-script-name-input:focus {
    border-bottom-color: var(--accent);
  }

  .run-script-name-input::placeholder {
    color: var(--text-muted);
  }

  .run-script-delete {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
  }

  .run-script-delete:hover {
    background: var(--bg-hover);
    color: var(--diff-del);
  }

  .add-script-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.35rem 0.65rem;
    background: none;
    border: 1px dashed var(--border-light);
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 0.73rem;
    cursor: pointer;
    font-family: inherit;
  }

  .add-script-btn:hover {
    border-color: var(--accent);
    color: var(--accent);
  }

  /* ── LSP ──────────────────────────────── */

  .section-desc {
    font-size: 0.76rem;
    color: var(--text-muted);
    line-height: 1.5;
    margin-bottom: 1.5rem;
  }

  .lsp-card {
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.85rem;
    margin-bottom: 0.75rem;
  }

  .lsp-card.adding {
    border-color: var(--accent);
    border-style: dashed;
  }

  .lsp-card-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.65rem;
  }

  .lsp-id {
    font-weight: 600;
    font-size: 0.82rem;
    color: var(--text-bright);
  }

  .lsp-badges {
    display: flex;
    gap: 0.3rem;
    flex: 1;
  }

  .lsp-badge {
    font-size: 0.62rem;
    padding: 0.1rem 0.4rem;
    border-radius: 4px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
    font-weight: 500;
  }

  .lsp-badge.builtin {
    background: var(--bg-dim);
    color: var(--text-dim);
  }

  .lsp-badge.override {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }

  .lsp-badge.custom {
    background: color-mix(in srgb, var(--status-ok) 15%, transparent);
    color: var(--status-ok);
  }

  .lsp-badge.status-ready {
    background: color-mix(in srgb, var(--status-ok) 15%, transparent);
    color: var(--status-ok);
  }

  .lsp-badge.status-starting,
  .lsp-badge.status-indexing {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
  }

  .lsp-badge.status-error {
    background: color-mix(in srgb, var(--status-error) 15%, transparent);
    color: var(--status-error);
  }

  .lsp-badge.status-stopped {
    background: color-mix(in srgb, var(--text-dim) 15%, transparent);
    color: var(--text-dim);
  }

  .lsp-actions {
    display: flex;
    gap: 0.25rem;
  }

  .lsp-action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
  }

  .lsp-action-btn:hover {
    color: var(--text-bright);
    border-color: var(--border-light);
  }

  .lsp-action-btn.danger:hover {
    color: var(--status-error);
    border-color: var(--status-error);
  }

  .lsp-action-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .lsp-action-btn :global(.lsp-spin) {
    animation: lsp-spin 1s linear infinite;
  }

  @keyframes lsp-spin {
    to { transform: rotate(360deg); }
  }

  .lsp-fields {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0.4rem 0.65rem;
  }

  .lsp-field {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .lsp-field span {
    font-size: 0.65rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .lsp-field input {
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.3rem 0.45rem;
    font-size: 0.75rem;
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
  }

  .lsp-field input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .lsp-add-actions {
    display: flex;
    gap: 0.5rem;
    margin-top: 0.65rem;
  }

  .lsp-save-btn {
    padding: 0.3rem 0.75rem;
    background: var(--accent);
    color: var(--on-accent, #fff);
    border: none;
    border-radius: 5px;
    font-size: 0.73rem;
    cursor: pointer;
    font-family: inherit;
  }

  .lsp-cancel-btn {
    padding: 0.3rem 0.75rem;
    background: none;
    border: 1px solid var(--border);
    border-radius: 5px;
    color: var(--text-muted);
    font-size: 0.73rem;
    cursor: pointer;
    font-family: inherit;
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

  .system-prompt-field {
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

  .system-prompt-field::placeholder {
    color: var(--text-muted);
  }

  .system-prompt-field:focus {
    border-color: var(--border-light);
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

  /* ── Theme picker ──────────────── */

  .theme-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 0.75rem;
  }

  .theme-card {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.6rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 8px;
    cursor: pointer;
    font-family: inherit;
    transition: border-color 0.15s;
  }

  .theme-card:hover {
    border-color: var(--border-light);
  }

  .theme-card.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 5%, var(--bg-sidebar));
  }

  .theme-preview {
    display: grid;
    border-radius: 5px;
    overflow: hidden;
    gap: 0;
  }

  .theme-swatch {
    height: 10px;
  }

  .theme-name {
    font-size: 0.78rem;
    font-weight: 600;
    color: var(--text-primary);
    text-align: center;
  }

  .theme-card.selected .theme-name {
    color: var(--accent);
  }

  /* ── Color mode picker ─────────── */

  .color-mode-picker {
    display: flex;
    gap: 0;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    width: fit-content;
  }

  .color-mode-btn {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.45rem 0.85rem;
    background: none;
    border: none;
    border-right: 1px solid var(--border);
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .color-mode-btn:last-child {
    border-right: none;
  }

  .color-mode-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .color-mode-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-sidebar));
  }

  /* ── Knowledge base ──────────────── */

  .kb-status {
    font-size: 0.72rem;
    font-weight: 600;
    padding: 0.15rem 0.5rem;
    border-radius: 4px;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  .kb-not-built {
    color: var(--text-muted);
    background: var(--bg-card);
    border: 1px solid var(--border);
  }

  .kb-building {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, var(--bg-card));
    border: 1px solid var(--accent);
  }

  .kb-built {
    color: var(--status-ok);
    background: color-mix(in srgb, var(--status-ok) 10%, var(--bg-card));
    border: 1px solid var(--status-ok);
  }

  .kb-failed {
    color: var(--status-error);
    background: color-mix(in srgb, var(--status-error) 10%, var(--bg-card));
    border: 1px solid var(--status-error);
  }

  /* Summary bar */

  .kb-summary-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 1.25rem;
  }

  .kb-summary-left {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .kb-summary-counts {
    font-size: 0.78rem;
    color: var(--text-secondary);
  }

  .kb-contradictions-badge {
    color: var(--status-warning, var(--accent));
    font-weight: 600;
  }

  .kb-summary-date {
    font-size: 0.68rem;
    color: var(--text-dim);
  }

  .kb-commit-hash {
    font-family: var(--font-mono);
    font-size: 0.66rem;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border);
    padding: 0.05rem 0.3rem;
    border-radius: 3px;
  }

  .kb-summary-actions {
    display: flex;
    gap: 0.4rem;
    flex-shrink: 0;
  }

  .kb-update-btn {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .kb-update-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 15%, var(--bg-sidebar));
  }

  .kb-update-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  /* Building activity */

  .kb-building-activity {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.6rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-bottom: 1.75rem;
  }

  .kb-activity-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--accent);
    min-width: 0;
    flex: 1;
  }

  .kb-activity-text {
    font-size: 0.78rem;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .kb-error {
    padding: 0.6rem 0.75rem;
    background: color-mix(in srgb, var(--status-error) 8%, var(--bg-sidebar));
    border: 1px solid var(--status-error);
    border-radius: 6px;
    margin-bottom: 1.75rem;
    font-size: 0.78rem;
    color: var(--status-error);
  }

  /* Review tabs */

  .kb-tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 1rem;
  }

  .kb-tab {
    padding: 0.45rem 0.75rem;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
    white-space: nowrap;
  }

  .kb-tab:hover {
    color: var(--text-primary);
  }

  .kb-tab.active {
    color: var(--accent);
    border-bottom-color: var(--accent);
  }

  /* Entry cards */

  .kb-entries {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1.5rem;
  }

  .kb-entry-card {
    padding: 0.6rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .kb-entry-row {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
  }

  .kb-entry-id {
    font-size: 0.68rem;
    font-weight: 700;
    color: var(--accent);
    flex-shrink: 0;
    font-family: var(--font-mono);
  }

  .kb-entry-title {
    font-size: 0.8rem;
    color: var(--text-primary);
    flex: 1;
    min-width: 0;
  }

  .kb-entry-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 0.35rem;
  }

  .kb-entry-section-title {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .kb-entry-content {
    font-family: var(--font-mono);
    font-size: 0.73rem;
    color: var(--text-secondary);
    margin: 0;
    white-space: pre-wrap;
    line-height: 1.5;
    max-height: 200px;
    overflow-y: auto;
  }

  .kb-entry-actions {
    display: flex;
    gap: 0.25rem;
    flex-shrink: 0;
    margin-left: auto;
  }

  .kb-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    background: none;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-dim);
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s, background 0.15s;
  }

  .kb-icon-btn:hover {
    color: var(--text-primary);
    border-color: var(--border);
    background: var(--bg-card);
  }

  .kb-icon-danger:hover {
    color: var(--status-error);
    border-color: var(--status-error);
    background: color-mix(in srgb, var(--status-error) 8%, var(--bg-card));
  }

  /* Edit mode */

  .kb-entry-edit {
    width: 100%;
    background: var(--bg-base);
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--text-primary);
    font-family: var(--font-mono);
    font-size: 0.78rem;
    line-height: 1.5;
    padding: 0.5rem 0.6rem;
    resize: vertical;
    outline: none;
    box-sizing: border-box;
  }

  .kb-entry-edit::placeholder {
    color: var(--text-muted);
  }

  .kb-entry-edit-actions {
    display: flex;
    gap: 0.35rem;
    margin-top: 0.4rem;
  }

  .kb-empty {
    font-size: 0.78rem;
    color: var(--text-dim);
    padding: 1.5rem;
    text-align: center;
  }

  /* Contradiction cards */

  .kb-contra-card {
    border-left: 3px solid var(--status-warning, var(--accent));
  }

  .kb-contra-header {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.35rem;
  }

  .kb-contra-title {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .kb-contradiction-actions {
    display: flex;
    gap: 0.4rem;
    margin-top: 0.6rem;
  }

  .kb-resolve-form {
    margin-top: 0.6rem;
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .kb-resolve-actions {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
  }

  .kb-resolve-btn {
    padding: 0.25rem 0.5rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: inherit;
    font-size: 0.72rem;
    color: var(--text-secondary);
    cursor: pointer;
    transition: border-color 0.15s;
  }

  .kb-resolve-btn:hover {
    border-color: var(--accent);
    color: var(--text-primary);
  }

  .kb-primary-resolve {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .kb-resolve-debt {
    color: var(--text-dim);
  }

  /* Draft review */

  .kb-draft-header {
    display: flex;
    align-items: baseline;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
  }

  .kb-draft-label {
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--text-primary);
  }

  .kb-draft-hint {
    font-size: 0.68rem;
    color: var(--text-dim);
  }

  .kb-draft-edit {
    min-height: 180px;
    max-height: 400px;
  }

  .kb-draft-loading {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 0;
    color: var(--accent);
    font-size: 0.78rem;
  }

  .kb-draft-error {
    font-size: 0.73rem;
    color: var(--status-error);
    padding: 0.3rem 0;
  }

  /* Config section */

  .kb-scope-fields {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .kb-scope-field {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .kb-scope-label {
    font-size: 0.72rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .kb-actions {
    margin-top: 1rem;
    margin-bottom: 2rem;
  }

  .kb-action-btn {
    padding: 0.4rem 0.85rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.78rem;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }

  .kb-action-btn:hover {
    border-color: var(--border-light);
    background: var(--bg-hover);
  }

  .kb-primary-btn {
    background: color-mix(in srgb, var(--accent) 15%, var(--bg-sidebar));
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }

  .kb-primary-btn:hover {
    background: color-mix(in srgb, var(--accent) 25%, var(--bg-sidebar));
  }

  .kb-cancel-btn {
    flex-shrink: 0;
    font-size: 0.75rem;
    padding: 0.3rem 0.6rem;
    color: var(--text-secondary);
  }

  .kb-config-toggle {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    background: none;
    border: none;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.75rem;
    cursor: pointer;
    padding: 0.5rem 0;
    margin-top: 1rem;
    border-top: 1px solid var(--border);
    width: 100%;
  }

  .kb-config-toggle:hover {
    color: var(--text-secondary);
  }

  :global(.chevron-closed) {
    transform: rotate(-90deg);
    transition: transform 0.15s;
  }

  :global(.chevron-open) {
    transform: rotate(0deg);
    transition: transform 0.15s;
  }

  .kb-config-panel {
    padding-top: 0.75rem;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

</style>

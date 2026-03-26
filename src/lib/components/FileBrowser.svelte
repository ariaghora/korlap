<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { Folder, FolderOpen, File as FileIcon, Search, FileSearch } from "lucide-svelte";
  import { listDirectory, readFile, writeFile, listRepoDirectory, readRepoFile, writeRepoFile, searchWorkspaceFiles, searchRepoFiles, lspStartServer, type FileEntry, type FileSearchResult } from "$lib/ipc";
  import { tooltip } from "$lib/actions";
  import ResizeHandle from "./ResizeHandle.svelte";
  import CodeEditor from "./CodeEditor.svelte";
  import SearchModal from "./SearchModal.svelte";

  // ── Devicon imports (Vite resolves as URL strings) ──
  import iconRust from "devicon/icons/rust/rust-original.svg";
  import iconTs from "devicon/icons/typescript/typescript-original.svg";
  import iconJs from "devicon/icons/javascript/javascript-original.svg";
  import iconSvelte from "devicon/icons/svelte/svelte-original.svg";
  import iconPython from "devicon/icons/python/python-original.svg";
  import iconGo from "devicon/icons/go/go-original.svg";
  import iconHtml from "devicon/icons/html5/html5-original.svg";
  import iconCss from "devicon/icons/css3/css3-original.svg";
  import iconSass from "devicon/icons/sass/sass-original.svg";
  import iconJson from "devicon/icons/json/json-original.svg";
  import iconMarkdown from "devicon/icons/markdown/markdown-original.svg";
  import iconBash from "devicon/icons/bash/bash-original.svg";
  import iconDocker from "devicon/icons/docker/docker-original.svg";
  import iconYaml from "devicon/icons/yaml/yaml-original.svg";
  import iconRuby from "devicon/icons/ruby/ruby-original.svg";
  import iconJava from "devicon/icons/java/java-original.svg";
  import iconKotlin from "devicon/icons/kotlin/kotlin-original.svg";
  import iconSwift from "devicon/icons/swift/swift-original.svg";
  import iconC from "devicon/icons/c/c-original.svg";
  import iconCpp from "devicon/icons/cplusplus/cplusplus-original.svg";
  import iconCsharp from "devicon/icons/csharp/csharp-original.svg";
  import iconPhp from "devicon/icons/php/php-original.svg";
  import iconLua from "devicon/icons/lua/lua-original.svg";
  import iconGit from "devicon/icons/git/git-original.svg";

  type BrowserScope =
    | { type: "workspace"; workspaceId: string }
    | { type: "repo"; repoId: string };

  interface Props {
    scope: BrowserScope;
    navigateTo?: string | null;
    navigateToLine?: number | null;
  }

  let { scope, navigateTo = null, navigateToLine = null }: Props = $props();

  // ── Scope-aware IPC dispatchers ──────────────────────
  let scopeKey = $derived(scope.type === "workspace" ? scope.workspaceId : scope.repoId);

  function doListDirectory(relativePath: string): Promise<FileEntry[]> {
    if (scope.type === "workspace") return listDirectory(scope.workspaceId, relativePath);
    return listRepoDirectory(scope.repoId, relativePath);
  }

  function doReadFile(relativePath: string): Promise<string> {
    if (scope.type === "workspace") return readFile(scope.workspaceId, relativePath);
    return readRepoFile(scope.repoId, relativePath);
  }

  function doWriteFile(relativePath: string, content: string): Promise<void> {
    if (scope.type === "workspace") return writeFile(scope.workspaceId, relativePath, content);
    return writeRepoFile(scope.repoId, relativePath, content);
  }

  function doSearchFiles(query: string): Promise<FileSearchResult[]> {
    if (scope.type === "workspace") return searchWorkspaceFiles(scope.workspaceId, query);
    return searchRepoFiles(scope.repoId, query);
  }

  // ── Fuzzy file search state ─────────────────────────────
  let showSearch = $state(false);
  let searchQuery = $state("");
  let searchResults = $state<FileSearchResult[]>([]);
  let searchSelectedIndex = $state(0);
  let searchLoading = $state(false);
  let searchInputEl: HTMLInputElement | undefined = $state();
  let browserEl: HTMLDivElement | undefined = $state();
  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;

  function openSearch() {
    showSearch = true;
    showGrep = false; // close grep if open
    searchQuery = "";
    searchResults = [];
    searchSelectedIndex = 0;
    searchLoading = false;
    // Focus input after it mounts
    queueMicrotask(() => searchInputEl?.focus());
  }

  function closeSearch() {
    showSearch = false;
    searchQuery = "";
    searchResults = [];
    clearTimeout(searchDebounceTimer);
  }

  // Debounced fuzzy search
  $effect(() => {
    const q = searchQuery;
    clearTimeout(searchDebounceTimer);

    if (!q.trim()) {
      searchResults = [];
      searchSelectedIndex = 0;
      searchLoading = false;
      return;
    }

    searchLoading = true;
    searchDebounceTimer = setTimeout(async () => {
      try {
        const results = await doSearchFiles(q);
        searchResults = results.filter((r) => r.kind === "file");
        searchSelectedIndex = 0;
      } catch {
        searchResults = [];
      } finally {
        searchLoading = false;
      }
    }, 150);
  });

  function handleSearchKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeSearch();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (searchResults.length > 0) {
        searchSelectedIndex = (searchSelectedIndex + 1) % searchResults.length;
        scrollSearchItemIntoView();
      }
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (searchResults.length > 0) {
        searchSelectedIndex = (searchSelectedIndex - 1 + searchResults.length) % searchResults.length;
        scrollSearchItemIntoView();
      }
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const result = searchResults[searchSelectedIndex];
      if (result) {
        closeSearch();
        navigateToPath(result.path, null);
      }
      return;
    }
  }

  function scrollSearchItemIntoView() {
    queueMicrotask(() => {
      const el = browserEl?.querySelector(".search-results .search-item.active");
      el?.scrollIntoView({ block: "nearest" });
    });
  }

  function selectSearchResult(result: FileSearchResult) {
    closeSearch();
    navigateToPath(result.path, null);
  }

  // ── Grep (content search) ──────────────────────────────
  let showGrep = $state(false);

  function openGrep() {
    showGrep = true;
    showSearch = false;
  }

  function closeGrep() {
    showGrep = false;
  }

  // Cmd+F / Cmd+Shift+F handler — only when this file browser is visible
  function handleGlobalKeydown(e: KeyboardEvent) {
    if (e.key === "f" && e.metaKey) {
      // Check if this component is visible (not display:none, not in inert container)
      if (browserEl && browserEl.offsetParent !== null && !browserEl.closest("[inert]")) {
        e.preventDefault();
        e.stopPropagation();
        if (e.shiftKey) {
          // Cmd+Shift+F → grep (content search)
          if (!showGrep) {
            openGrep();
          }
        } else {
          // Cmd+F → file search
          if (showSearch) {
            searchInputEl?.focus();
          } else {
            openSearch();
          }
        }
      }
    }
  }

  // ── Tree state ────────────────────────────────────────

  interface TreeNode {
    entry: FileEntry;
    children: TreeNode[] | null; // null = not loaded yet
    loading: boolean;
  }

  let rootEntries = $state<TreeNode[]>([]);
  let expandedPaths = new SvelteSet<string>();
  let selectedPath = $state<string | null>(null);
  let rootLoading = $state(false);
  let rootError = $state("");

  // ── File content state ────────────────────────────────

  let fileContent = $state("");
  let fileLoading = $state(false);
  let fileError = $state("");
  let isEditing = $state(false);
  let editContent = $state("");
  let saving = $state(false);
  let saveMessage = $state("");
  let pendingLine = $state<number | null>(null);
  let editorRef: CodeEditor | undefined = $state();

  // ── Load root on workspace change ─────────────────────

  $effect(() => {
    const _key = scopeKey;
    rootLoading = true;
    rootError = "";
    selectedPath = null;
    fileContent = "";
    isEditing = false;
    expandedPaths.clear();

    // Start LSP servers in background when file browser opens for a workspace
    if (scope.type === "workspace") {
      lspStartServer(scope.workspaceId).catch(() => {});
    }

    doListDirectory("")
      .then((entries) => {
        rootEntries = entries.map(toNode);
      })
      .catch((e) => {
        rootError = String(e);
      })
      .finally(() => {
        rootLoading = false;
      });
  });

  function toNode(entry: FileEntry): TreeNode {
    return { entry, children: null, loading: false };
  }

  // ── Tree interactions ──────────────────────────────────

  async function toggleDir(node: TreeNode) {
    const path = node.entry.path;

    if (expandedPaths.has(path)) {
      expandedPaths.delete(path);
      return;
    }

    if (node.children === null) {
      node.loading = true;
      try {
        const entries = await doListDirectory(path);
        node.children = entries.map(toNode);
      } catch (e) {
        node.children = [];
      } finally {
        node.loading = false;
      }
    }

    expandedPaths.add(path);
  }

  async function selectFile(path: string) {
    if (selectedPath === path) return;
    selectedPath = path;
    isEditing = false;
    saveMessage = "";
    fileLoading = true;
    fileError = "";

    try {
      fileContent = await doReadFile(path);
    } catch (e) {
      fileError = String(e);
      fileContent = "";
    } finally {
      fileLoading = false;
    }
  }

  /** Navigate to a file+line from LSP go-to-definition (Cmd+click). */
  async function handleGotoDef(filePath: string, line: number) {
    await selectFile(filePath);
    // Wait a tick for the editor to mount with new content, then jump to line
    requestAnimationFrame(() => {
      editorRef?.goToLine(line);
    });
  }

  function startEditing() {
    editContent = fileContent;
    isEditing = true;
    saveMessage = "";
  }

  function cancelEditing() {
    isEditing = false;
    saveMessage = "";
  }

  async function saveFile() {
    if (!selectedPath) return;
    saving = true;
    saveMessage = "";

    try {
      await doWriteFile(selectedPath, editContent);
      fileContent = editContent;
      isEditing = false;
      saveMessage = "Saved";
      setTimeout(() => { saveMessage = ""; }, 2000);
    } catch (e) {
      saveMessage = `Error: ${e}`;
    } finally {
      saving = false;
    }
  }

  // ── Helpers ────────────────────────────────────────────

  function fileName(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function fileExtension(name: string): string {
    const dot = name.lastIndexOf(".");
    return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
  }

  const extIconMap: Record<string, string> = {
    rs: iconRust,
    ts: iconTs, tsx: iconTs,
    js: iconJs, jsx: iconJs, mjs: iconJs, cjs: iconJs,
    svelte: iconSvelte,
    py: iconPython, pyw: iconPython,
    go: iconGo,
    html: iconHtml, htm: iconHtml,
    css: iconCss,
    scss: iconSass, sass: iconSass,
    json: iconJson,
    md: iconMarkdown, mdx: iconMarkdown,
    sh: iconBash, bash: iconBash, zsh: iconBash,
    yml: iconYaml, yaml: iconYaml,
    rb: iconRuby,
    java: iconJava,
    kt: iconKotlin, kts: iconKotlin,
    swift: iconSwift,
    c: iconC, h: iconC,
    cpp: iconCpp, cc: iconCpp, cxx: iconCpp, hpp: iconCpp,
    cs: iconCsharp,
    php: iconPhp,
    lua: iconLua,
    dockerfile: iconDocker,
  };

  // Special filenames that map to icons regardless of extension
  const nameIconMap: Record<string, string> = {
    "Dockerfile": iconDocker,
    "Makefile": iconBash,
    ".gitignore": iconGit,
    ".gitmodules": iconGit,
    ".gitattributes": iconGit,
  };

  function fileIconUrl(name: string): string | null {
    if (nameIconMap[name]) return nameIconMap[name];
    const ext = fileExtension(name);
    return extIconMap[ext] ?? null;
  }

  function isBinaryExt(name: string): boolean {
    const ext = fileExtension(name);
    return ["png", "jpg", "jpeg", "gif", "ico", "svg", "woff", "woff2", "ttf", "eot", "mp3", "mp4", "zip", "tar", "gz", "bin", "exe", "dll", "so", "dylib", "pdf"].includes(ext);
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  let treeWidth = $state(260);
  const TREE_MIN = 140;
  const TREE_MAX = 500;

  function handleTreeResize(delta: number) {
    treeWidth = Math.min(TREE_MAX, Math.max(TREE_MIN, treeWidth + delta));
  }

  async function refreshTree() {
    rootLoading = true;
    rootError = "";
    try {
      const entries = await doListDirectory("");
      rootEntries = entries.map(toNode);
      expandedPaths.clear();
    } catch (e) {
      rootError = String(e);
    } finally {
      rootLoading = false;
    }
  }

  // ── Deep-link navigation ────────────────────────────────

  let lastNavigated = $state<string | null>(null);
  let lastNavigatedLine = $state<number | null>(null);

  $effect(() => {
    const target = navigateTo;
    const line = navigateToLine ?? null;
    if (!target) return;
    if (target === lastNavigated && line === lastNavigatedLine) return;
    lastNavigated = target;
    lastNavigatedLine = line;

    // Same file, different line — just jump without re-loading
    if (target === selectedPath && line && line > 0 && editorRef && !fileLoading) {
      pendingLine = line;
      return;
    }

    navigateToPath(target, line);
  });

  async function navigateToPath(target: string, line: number | null) {
    // Split "src/lib/components/Foo.svelte" → ["src", "src/lib", "src/lib/components"]
    const parts = target.split("/");
    const dirSegments: string[] = [];
    for (let i = 0; i < parts.length - 1; i++) {
      dirSegments.push(parts.slice(0, i + 1).join("/"));
    }

    // Ensure root entries are loaded before navigating
    if (rootEntries.length === 0) {
      try {
        const entries = await doListDirectory("");
        rootEntries = entries.map(toNode);
        rootLoading = false;
      } catch {
        return;
      }
    }

    // Expand each directory level sequentially (lazy-loaded)
    let nodes = rootEntries;
    for (const dirPath of dirSegments) {
      const node = nodes.find((n) => n.entry.path === dirPath && n.entry.is_dir);
      if (!node) return; // path not found in tree

      if (node.children === null) {
        node.loading = true;
        try {
          const entries = await doListDirectory(dirPath);
          node.children = entries.map(toNode);
        } catch {
          node.children = [];
          return;
        } finally {
          node.loading = false;
        }
      }

      expandedPaths.add(dirPath);
      nodes = node.children;
    }

    // Select the target file
    const fileNode = nodes.find((n) => n.entry.path === target && !n.entry.is_dir);
    if (fileNode && !isBinaryExt(fileNode.entry.name)) {
      pendingLine = line;
      await selectFile(target);
    }
  }

  // ── Jump to line after editor mounts with new content ──

  $effect(() => {
    const line = pendingLine;
    const ref = editorRef;
    if (line && line > 0 && ref && !fileLoading) {
      // Use tick to let CM mount
      queueMicrotask(() => {
        ref.goToLine(line);
        pendingLine = null;
      });
    }
  });
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window onkeydown={handleGlobalKeydown} />

<div class="file-browser" bind:this={browserEl}>
  {#if rootLoading && rootEntries.length === 0}
    <div class="browser-empty">Loading...</div>
  {:else if rootError}
    <div class="browser-empty browser-error">{rootError}</div>
  {:else}
    <div class="browser-layout">
      <!-- File tree sidebar -->
      <div class="tree-sidebar" style="width: {treeWidth}px">
        <div class="tree-header">
          <span class="tree-title">Files</span>
          <div class="tree-header-actions">
            <button class="header-icon-btn" onclick={openSearch} use:tooltip={{ text: "Find file", shortcut: "⌘F" }}>
              <Search size={13} />
            </button>
            <button class="header-icon-btn" onclick={openGrep} use:tooltip={{ text: "Search in files", shortcut: "⇧⌘F" }}>
              <FileSearch size={13} />
            </button>
            <button class="refresh-btn" onclick={refreshTree} use:tooltip={{ text: "Refresh" }}>↻</button>
          </div>
        </div>

        {#if showSearch}
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="search-bar" onkeydown={handleSearchKeydown}>
            <div class="search-input-row">
              <Search size={12} class="search-icon" />
              <input
                bind:this={searchInputEl}
                bind:value={searchQuery}
                type="text"
                class="search-input"
                placeholder="Search files..."
                spellcheck="false"
                autocomplete="off"
              />
              {#if searchLoading}
                <span class="search-spinner"></span>
              {/if}
            </div>
            {#if searchResults.length > 0}
              <div class="search-results">
                {#each searchResults as result, i}
                  <button
                    class="search-item"
                    class:active={i === searchSelectedIndex}
                    onclick={() => selectSearchResult(result)}
                    onmouseenter={() => { searchSelectedIndex = i; }}
                  >
                    <span class="search-item-icon">
                      {#if fileIconUrl(result.name)}
                        <img src={fileIconUrl(result.name)} alt="" class="devicon" />
                      {:else}
                        <FileIcon size={13} />
                      {/if}
                    </span>
                    <span class="search-item-name">{result.name}</span>
                    <span class="search-item-path">{result.path}</span>
                  </button>
                {/each}
              </div>
            {:else if searchQuery.trim() && !searchLoading}
              <div class="search-empty">No files found</div>
            {/if}
          </div>
        {/if}

        <div class="tree-list">
          {#each rootEntries as node}
            {@render treeItem(node, 0)}
          {/each}
        </div>
      </div>
      <ResizeHandle onResize={handleTreeResize} />

      <!-- File content -->
      <div class="file-content-area">
        {#if !selectedPath}
          <div class="browser-empty">Select a file to view</div>
        {:else if fileLoading}
          <div class="browser-empty">Loading...</div>
        {:else if fileError}
          <div class="browser-empty browser-error">{fileError}</div>
        {:else}
          <div class="file-header">
            <span class="file-header-path">{selectedPath}</span>
            <div class="file-header-actions">
              {#if saveMessage}
                <span class="save-message" class:error={saveMessage.startsWith("Error")}>{saveMessage}</span>
              {/if}
              {#if isEditing}
                <button class="action-btn" onclick={saveFile} disabled={saving}>
                  {saving ? "Saving..." : "Save"}
                </button>
                <button class="action-btn secondary" onclick={cancelEditing}>Cancel</button>
              {:else}
                <button class="action-btn" onclick={startEditing}>Edit</button>
              {/if}
            </div>
          </div>
          <div class="file-body">
            {#if isEditing}
              <CodeEditor
                content={editContent}
                filename={selectedPath}
                readonly={false}
                onchange={(c) => { editContent = c; }}
                lsp={scope.type === "workspace" && selectedPath ? { workspaceId: scope.workspaceId, filePath: selectedPath, onGotoDef: handleGotoDef } : null}
              />
            {:else}
              <CodeEditor
                bind:this={editorRef}
                content={fileContent}
                filename={selectedPath}
                readonly={true}
                lsp={scope.type === "workspace" && selectedPath ? { workspaceId: scope.workspaceId, filePath: selectedPath, onGotoDef: handleGotoDef } : null}
              />
            {/if}
          </div>
        {/if}
      </div>

      {#if showGrep}
        <SearchModal
          workspaceId={scope.type === "workspace" ? scope.workspaceId : undefined}
          repoId={scope.type === "repo" ? scope.repoId : undefined}
          onClose={closeGrep}
          enterLabel="open file"
          onAddToContext={(path, _name, line) => {
            closeGrep();
            navigateToPath(path, line);
          }}
        />
      {/if}
    </div>
  {/if}
</div>

{#snippet treeItem(node: TreeNode, depth: number)}
  {#if node.entry.is_dir}
    <button
      class="tree-node"
      class:active={false}
      style="padding-left: {0.5 + depth * 0.9}rem"
      onclick={() => toggleDir(node)}
    >
      <span class="tree-chevron" class:expanded={expandedPaths.has(node.entry.path)}>
        {#if node.loading}
          <span class="tree-spinner">...</span>
        {:else}
          ›
        {/if}
      </span>
      <span class="tree-icon dir">
        {#if expandedPaths.has(node.entry.path)}
          <FolderOpen size={14} />
        {:else}
          <Folder size={14} />
        {/if}
      </span>
      <span class="tree-name">{node.entry.name}</span>
    </button>
    {#if expandedPaths.has(node.entry.path) && node.children}
      {#each node.children as child}
        {@render treeItem(child, depth + 1)}
      {/each}
    {/if}
  {:else}
    <button
      class="tree-node"
      class:active={selectedPath === node.entry.path}
      style="padding-left: {0.5 + depth * 0.9 + 1.1}rem"
      onclick={() => {
        if (!isBinaryExt(node.entry.name)) selectFile(node.entry.path);
      }}
      disabled={isBinaryExt(node.entry.name)}
    >
      <span class="tree-icon file">
        {#if fileIconUrl(node.entry.name)}
          <img src={fileIconUrl(node.entry.name)} alt="" class="devicon" />
        {:else}
          <FileIcon size={14} />
        {/if}
      </span>
      <span class="tree-name" class:binary={isBinaryExt(node.entry.name)}>{node.entry.name}</span>
      <span class="tree-size">{formatSize(node.entry.size)}</span>
    </button>
  {/if}
{/snippet}

<style>
  .file-browser {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .browser-empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  .browser-error {
    color: var(--diff-del);
  }

  .browser-layout {
    flex: 1;
    display: flex;
    min-height: 0;
  }

  /* ── Tree sidebar ───────────────────────── */

  .tree-sidebar {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
  }

  .tree-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.4rem 0.6rem;
    border-bottom: 1px solid var(--border);
    font-size: 0.72rem;
  }

  .tree-title {
    color: var(--text-secondary);
    font-weight: 600;
  }

  .tree-header-actions {
    display: flex;
    align-items: center;
    gap: 0.15rem;
  }

  .header-icon-btn {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    padding: 0.1rem 0.2rem;
    display: flex;
    align-items: center;
    border-radius: 3px;
  }

  .header-icon-btn:hover {
    color: var(--text-primary);
    background: var(--bg-hover);
  }

  .refresh-btn {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0 0.2rem;
  }

  .refresh-btn:hover {
    color: var(--text-primary);
  }

  /* ── File search overlay ─────────────────── */

  .search-bar {
    border-bottom: 1px solid var(--border);
    background: var(--bg-surface);
  }

  .search-input-row {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.3rem 0.5rem;
  }

  .search-input-row :global(.search-icon) {
    flex-shrink: 0;
    color: var(--text-dim);
  }

  .search-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.73rem;
    min-width: 0;
  }

  .search-input::placeholder {
    color: var(--text-dim);
  }

  .search-spinner {
    width: 10px;
    height: 10px;
    border: 1.5px solid var(--text-dim);
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    flex-shrink: 0;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .search-results {
    max-height: 240px;
    overflow-y: auto;
    padding: 0.15rem 0;
  }

  .search-item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.25rem 0.5rem;
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.73rem;
    text-align: left;
    white-space: nowrap;
  }

  .search-item:hover,
  .search-item.active {
    background: var(--bg-hover);
  }

  .search-item-icon {
    width: 1rem;
    height: 1rem;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
  }

  .search-item-name {
    flex-shrink: 0;
    font-weight: 500;
  }

  .search-item-path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-dim);
    font-size: 0.65rem;
    direction: rtl;
    text-align: left;
  }

  .search-empty {
    padding: 0.5rem;
    text-align: center;
    color: var(--text-dim);
    font-size: 0.72rem;
  }

  .tree-list {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  /* ── Tree nodes ─────────────────────────── */

  .tree-node {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.18rem 0.5rem;
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.75rem;
    text-align: left;
    white-space: nowrap;
  }

  .tree-node:hover {
    background: var(--bg-hover);
  }

  .tree-node.active {
    background: var(--border);
  }

  .tree-node:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .tree-chevron {
    width: 0.8rem;
    flex-shrink: 0;
    font-size: 0.75rem;
    color: var(--text-dim);
    transition: transform 0.1s;
    display: inline-block;
    text-align: center;
  }

  .tree-chevron.expanded {
    transform: rotate(90deg);
  }

  .tree-spinner {
    font-size: 0.6rem;
    animation: pulse-spin 0.8s ease-in-out infinite;
  }

  @keyframes pulse-spin {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .tree-icon {
    width: 1.1rem;
    height: 1.1rem;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
  }

  .tree-icon.dir {
    color: var(--accent);
  }

  .devicon {
    width: 14px;
    height: 14px;
    filter: brightness(0) invert(0.55);
  }

  .tree-name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 0.73rem;
  }

  .tree-name.binary {
    color: var(--text-dim);
  }

  .tree-size {
    flex-shrink: 0;
    font-size: 0.6rem;
    color: var(--text-dim);
    opacity: 0.6;
    padding-right: 0.4rem;
  }

  /* ── File content area ──────────────────── */

  .file-content-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .file-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.35rem 0.75rem;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
    gap: 0.5rem;
  }

  .file-header-path {
    font-family: var(--font-mono);
    font-size: 0.73rem;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file-header-actions {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex-shrink: 0;
  }

  .save-message {
    font-size: 0.7rem;
    color: var(--status-ok);
  }

  .save-message.error {
    color: var(--diff-del);
  }

  .action-btn {
    padding: 0.2rem 0.55rem;
    background: transparent;
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 4px;
    color: var(--accent);
    cursor: pointer;
    font-family: inherit;
    font-size: 0.7rem;
    font-weight: 600;
  }

  .action-btn:hover {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .action-btn.secondary {
    color: var(--text-dim);
    border-color: var(--border-light);
  }

  .action-btn.secondary:hover {
    background: var(--bg-hover);
  }

  .file-body {
    flex: 1;
    overflow: auto;
    min-height: 0;
  }

  .file-body :global(.code-editor) {
    flex: 1;
    min-height: 0;
  }

</style>

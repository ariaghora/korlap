<script lang="ts">
  import { type PastedImage } from "$lib/chat-utils";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { X, TextSearch, Lightbulb, BookOpen, ChevronDown } from "lucide-svelte";
  import { tooltip } from "$lib/actions";
  import MentionInput, { type Mention, type MentionInputValue, type MentionInputApi } from "./MentionInput.svelte";
  import MentionAutocomplete, { type MentionAutocompleteApi, type FileSearchResult } from "./MentionAutocomplete.svelte";
  import SearchModal from "./SearchModal.svelte";
  import { searchRepoFiles, MODEL_OPTIONS } from "$lib/ipc";

  export interface TaskData {
    title: string;
    description: string;
    newImages: PastedImage[];
    existingPaths: string[];
    mentions: Mention[];
    planMode: boolean;
    thinkingMode: boolean;
    model: string;
  }

  interface Props {
    repoId?: string;
    initialTitle?: string;
    initialDescription?: string;
    initialImagePaths?: string[];
    initialMentions?: Mention[];
    initialPlanMode?: boolean;
    initialThinkingMode?: boolean;
    initialModel?: string;
    submitLabel?: string;
    onSubmit: (data: TaskData) => void;
    onSubmitAndStart?: (data: TaskData) => void;
    onCancel: () => void;
  }

  let {
    repoId,
    initialTitle = "",
    initialDescription = "",
    initialImagePaths = [],
    initialMentions = [],
    initialPlanMode = false,
    initialThinkingMode = false,
    initialModel = "",
    submitLabel = "Add",
    onSubmit,
    onSubmitAndStart,
    onCancel,
  }: Props = $props();

  let title = $state(initialTitle);
  let existingPaths = $state<string[]>([...initialImagePaths]);
  let newImages = $state<PastedImage[]>([]);
  let mentions = $state<Mention[]>([...initialMentions]);
  let planMode = $state(initialPlanMode);
  let thinkingMode = $state(initialThinkingMode);
  let model = $state(initialModel);
  let showModelDropdown = $state(false);
  let modelBtnEl: HTMLButtonElement | undefined = $state();
  let titleRef: HTMLInputElement | undefined = $state();

  // Mention input + autocomplete state
  let mentionInputApi: MentionInputApi | undefined = $state();
  let autocompleteApi: MentionAutocompleteApi | undefined = $state();
  let autocompleteVisible = $state(false);
  let autocompleteResults = $state<FileSearchResult[]>([]);
  let autocompleteLoading = $state(false);
  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let descWrapperEl: HTMLDivElement | undefined = $state();

  // SearchModal
  let showSearchModal = $state(false);

  $effect(() => {
    requestAnimationFrame(() => titleRef?.focus());
  });

  // Seed initial description + inline mention chips on mount
  let contentRestored = false;
  $effect(() => {
    if (mentionInputApi && !contentRestored && (initialDescription || initialMentions.length > 0)) {
      contentRestored = true;
      mentionInputApi.restoreContent(initialDescription, initialMentions);
    }
  });

  let canSubmit = $derived(
    title.trim().length > 0 ||
    existingPaths.length > 0 ||
    newImages.length > 0 ||
    mentions.length > 0
  );

  function buildTaskData(): TaskData | null {
    if (!canSubmit) return null;
    // Serialize description + inline mentions from MentionInput
    const descValue = mentionInputApi?.getValue() ?? { text: "", mentions: [] };
    // Merge inline mentions with separately-tracked mentions (from SearchModal, etc.)
    const allMentions = [...mentions];
    for (const m of descValue.mentions) {
      if (!allMentions.some((existing) => existing.path === m.path)) {
        allMentions.push(m);
      }
    }
    return {
      title: title.trim(),
      description: descValue.text.trim(),
      newImages: [...newImages],
      existingPaths: [...existingPaths],
      mentions: allMentions,
      planMode,
      thinkingMode,
      model,
    };
  }

  function submit() {
    const data = buildTaskData();
    if (data) onSubmit(data);
  }

  function submitAndStart() {
    if (!onSubmitAndStart) return;
    const data = buildTaskData();
    if (data) onSubmitAndStart(data);
  }

  function handleOverlayKeydown(e: KeyboardEvent) {
    if (showSearchModal) return; // SearchModal handles its own keys
    if (e.key === "Escape") {
      e.preventDefault();
      if (showModelDropdown) {
        showModelDropdown = false;
      } else if (autocompleteVisible) {
        autocompleteVisible = false;
        autocompleteResults = [];
      } else {
        onCancel();
      }
    }
    if (e.key === "Enter" && e.metaKey && e.shiftKey) {
      e.preventDefault();
      submitAndStart();
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      submit();
    }
  }

  function handleTitleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      mentionInputApi?.focus();
    }
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
    if (e.key === "Enter" && e.metaKey && e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();
      submitAndStart();
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      submit();
    }
  }

  function handleTitlePaste(e: ClipboardEvent) {
    handlePaste(e);
  }

  function handlePaste(e: ClipboardEvent) {
    const items = e.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (!item.type.startsWith("image/")) continue;
      e.preventDefault();
      const file = item.getAsFile();
      if (!file) continue;
      const ext = item.type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
      const reader = new FileReader();
      reader.onload = () => {
        const dataUrl = reader.result as string;
        const base64 = dataUrl.split(",")[1] ?? "";
        newImages = [...newImages, { id: crypto.randomUUID(), dataUrl, base64, extension: ext }];
      };
      reader.readAsDataURL(file);
    }
  }

  function removeNewImage(id: string) {
    newImages = newImages.filter((i) => i.id !== id);
  }

  function removeExistingPath(path: string) {
    existingPaths = existingPaths.filter((p) => p !== path);
  }

  function removeMention(path: string) {
    mentions = mentions.filter((m) => m.path !== path);
  }

  // ── Mention autocomplete ────────────────────────────────────────

  function handleQueryChange(query: string | null) {
    if (!repoId) {
      // No repo → no file search
      autocompleteVisible = false;
      return;
    }
    if (query === null) {
      autocompleteVisible = false;
      autocompleteResults = [];
      autocompleteLoading = false;
      if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
      return;
    }

    autocompleteVisible = true;
    if (!query) {
      autocompleteResults = [];
      autocompleteLoading = false;
      return;
    }

    autocompleteLoading = true;
    if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
    searchDebounceTimer = setTimeout(async () => {
      try {
        const results = await searchRepoFiles(repoId!, query);
        autocompleteResults = results;
      } catch {
        autocompleteResults = [];
      }
      autocompleteLoading = false;
    }, 100);
  }

  function handleAutocompleteSelect(result: FileSearchResult) {
    const mention: Mention = {
      type: result.kind === "folder" ? "folder" : "file",
      path: result.path,
      displayName: result.name,
    };
    mentionInputApi?.insertMention(mention);
    // Also track in our mentions array for TaskData
    if (!mentions.some((m) => m.path === mention.path)) {
      mentions = [...mentions, mention];
    }
    autocompleteVisible = false;
    autocompleteResults = [];
  }

  function handleDescKeydown(e: KeyboardEvent) {
    // When autocomplete is open, intercept arrow keys and Enter
    if (autocompleteVisible && autocompleteResults.length > 0) {
      if (e.key === "ArrowUp") {
        e.preventDefault();
        autocompleteApi?.moveUp();
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        autocompleteApi?.moveDown();
      } else if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        autocompleteApi?.selectCurrent();
      } else if (e.key === "Escape") {
        e.preventDefault();
        autocompleteVisible = false;
        autocompleteResults = [];
      }
      return;
    }

    // Cmd+Shift+Enter to submit and start
    if (e.key === "Enter" && e.metaKey && e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();
      submitAndStart();
    }
    // Cmd+Enter to submit
    else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      e.stopPropagation();
      submit();
    }
    // Escape to cancel
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }

  function handleMentionSubmit(value: MentionInputValue) {
    // When Enter is pressed in MentionInput (without autocomplete), just submit the form
    // Collect mentions from the serialized value
    for (const m of value.mentions) {
      if (!mentions.some((existing) => existing.path === m.path)) {
        mentions = [...mentions, m];
      }
    }
    submit();
  }

  let modelLabel = $derived(MODEL_OPTIONS.find((m) => m.value === model)?.label ?? "Default");

  function selectModel(value: string) {
    model = value;
    showModelDropdown = false;
  }

  function handleSearchAddToContext(path: string, displayName: string, lineNumber: number) {
    showSearchModal = false;
    const mention: Mention = { type: "file", path, displayName, lineNumber };
    if (!mentions.some((m) => m.path === path)) {
      mentions = [...mentions, mention];
    }
    mentionInputApi?.appendMention(mention);
    mentionInputApi?.focus();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel} onkeydown={handleOverlayKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => { e.stopPropagation(); showModelDropdown = false; }}>
    <input
      class="task-title"
      bind:this={titleRef}
      bind:value={title}
      onkeydown={handleTitleKeydown}
      onpaste={handleTitlePaste}
      placeholder="Task title"
    />
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="desc-wrapper" bind:this={descWrapperEl} onkeydown={handleDescKeydown}>
      <MentionInput
        placeholder={repoId ? "Description — @mention files, paste images" : "Description (optional) — paste images here"}
        multiline
        onSubmit={handleMentionSubmit}
        onQueryChange={handleQueryChange}
        onPaste={handlePaste}
        bind:ref={mentionInputApi}
      />
    </div>

    {#if existingPaths.length > 0 || newImages.length > 0 || mentions.length > 0}
      <div class="attachment-strip">
        {#each mentions as mention (mention.path)}
          <div class="mention-pill">
            <span class="mention-pill-icon">{mention.type === "folder" ? "📁" : "📄"}</span>
            <span class="mention-pill-name">{mention.displayName}</span>
            <button class="pill-remove" onclick={() => removeMention(mention.path)}>
              <X size={8} />
            </button>
          </div>
        {/each}
        {#each existingPaths as path (path)}
          <div class="image-thumb">
            <img src={convertFileSrc(path)} alt="Attached" />
            <button class="image-remove" onclick={() => removeExistingPath(path)}>
              <X size={8} />
            </button>
          </div>
        {/each}
        {#each newImages as img (img.id)}
          <div class="image-thumb">
            <img src={img.dataUrl} alt="Attached" />
            <button class="image-remove" onclick={() => removeNewImage(img.id)}>
              <X size={8} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="dialog-footer">
      <div class="footer-row">
      <div class="footer-left">
        <button
          type="button"
          class="mode-pill"
          class:active={thinkingMode}
          onclick={() => { thinkingMode = !thinkingMode; }}
          use:tooltip={{ text: "Extended thinking" }}
        >
          <Lightbulb size={13} strokeWidth={2} />
          Thinking
        </button>
        <button
          type="button"
          class="mode-pill"
          class:active={planMode}
          onclick={() => { planMode = !planMode; }}
          use:tooltip={{ text: "Plan mode" }}
        >
          <BookOpen size={13} strokeWidth={2} />
          Plan
        </button>
        <div class="model-selector">
          <button
            type="button"
            class="mode-pill"
            class:active={model !== ""}
            bind:this={modelBtnEl}
            onclick={(e) => { e.stopPropagation(); showModelDropdown = !showModelDropdown; }}
            use:tooltip={{ text: "Model" }}
          >
            {modelLabel}
            <ChevronDown size={11} strokeWidth={2} />
          </button>
          {#if showModelDropdown}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="model-dropdown" onclick={(e) => e.stopPropagation()}>
              {#each MODEL_OPTIONS as opt (opt.value)}
                <button
                  class="model-option"
                  class:selected={model === opt.value}
                  onclick={() => selectModel(opt.value)}
                >
                  {opt.label}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      <div class="footer-actions">
        {#if repoId}
          <button
            class="search-btn"
            use:tooltip={{ text: "Search file contents", shortcut: "⌘⇧F" }}
            onclick={() => { showSearchModal = true; }}
          >
            <TextSearch size={14} />
          </button>
        {/if}
        <button class="cancel-btn" onclick={onCancel}>Cancel</button>
        {#if onSubmitAndStart}
          <button class="add-start-btn" onclick={submitAndStart} disabled={!canSubmit}>{submitLabel} & Start</button>
        {/if}
        <button class="submit-btn" onclick={submit} disabled={!canSubmit}>{submitLabel}</button>
      </div>
      </div>
      <span class="footer-hint">⌘Enter to {submitLabel.toLowerCase()}{onSubmitAndStart ? " · ⌘⇧Enter to start" : ""} · {repoId ? "@mention files · " : ""}Paste images</span>
    </div>
  </div>

  <MentionAutocomplete
    results={autocompleteResults}
    visible={autocompleteVisible}
    loading={autocompleteLoading}
    anchorEl={descWrapperEl ?? null}
    onSelect={handleAutocompleteSelect}
    bind:ref={autocompleteApi}
  />
</div>

{#if showSearchModal && repoId}
  <SearchModal
    {repoId}
    onClose={() => { showSearchModal = false; }}
    onAddToContext={handleSearchAddToContext}
  />
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dialog {
    width: 480px;
    max-width: 90vw;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem;
    background: color-mix(in srgb, var(--bg-sidebar) 97%, white);
    border: 0.5px solid color-mix(in srgb, var(--border-light) 60%, transparent);
    border-radius: 12px;
    box-shadow:
      0 0 0 0.5px rgba(0, 0, 0, 0.3),
      0 8px 32px rgba(0, 0, 0, 0.45),
      0 2px 8px rgba(0, 0, 0, 0.2);
  }

  .task-title {
    width: 100%;
    box-sizing: border-box;
    padding: 0.55rem 0.65rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 8px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 1rem;
    font-weight: 600;
    outline: none;
  }

  .task-title::placeholder {
    color: var(--text-muted);
    font-weight: 400;
  }

  .task-title:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .desc-wrapper {
    background: var(--input-inset-bg);
    border-radius: 8px;
    min-height: 120px;
    max-height: 200px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
  }

  .desc-wrapper :global(.mention-input) {
    min-height: 100px;
    max-height: none;
    font-size: 0.88rem;
  }

  .desc-wrapper:focus-within {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .attachment-strip {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    padding: 0.15rem 0;
  }

  .mention-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.2rem;
    padding: 0.15rem 0.45rem;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    border-radius: 5px;
    font-size: 0.75rem;
    color: var(--accent);
    font-family: var(--font-mono);
  }

  .mention-pill-icon {
    font-size: 0.65rem;
    opacity: 0.7;
  }

  .mention-pill-name {
    max-width: 140px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pill-remove {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: transparent;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    margin-left: 0.1rem;
    opacity: 0.5;
  }

  .pill-remove:hover {
    opacity: 1;
    background: color-mix(in srgb, var(--accent) 20%, transparent);
  }

  .image-thumb {
    position: relative;
    width: 56px;
    height: 56px;
    flex-shrink: 0;
  }

  .image-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 6px;
  }

  .image-remove {
    position: absolute;
    top: -5px;
    right: -5px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--img-remove-bg);
    border: none;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .image-remove:hover {
    background: var(--img-remove-hover);
    color: var(--text-bright);
  }

  .dialog-footer {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.15rem;
  }

  .footer-hint {
    font-size: 0.65rem;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .footer-left {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .mode-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.55rem;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 12px;
    color: var(--text-dim);
    font-family: inherit;
    font-size: 0.72rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
    line-height: 1;
  }

  .mode-pill:hover {
    border-color: var(--border-light);
    color: var(--text-secondary);
  }

  .mode-pill.active {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    color: var(--accent);
  }

  .mode-pill.active:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .model-selector {
    position: relative;
  }

  .model-dropdown {
    position: absolute;
    bottom: calc(100% + 4px);
    left: 0;
    min-width: 140px;
    background: var(--bg-sidebar);
    border: 1px solid var(--border-light);
    border-radius: 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    padding: 0.25rem;
    z-index: 10;
    display: flex;
    flex-direction: column;
  }

  .model-option {
    padding: 0.35rem 0.6rem;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.75rem;
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .model-option:hover {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    color: var(--text-bright);
  }

  .model-option.selected {
    color: var(--accent);
    font-weight: 600;
  }

  .footer-actions {
    display: flex;
    gap: 0.35rem;
    align-items: center;
  }

  .search-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    cursor: pointer;
    padding: 0;
  }

  .search-btn:hover {
    background: var(--btn-subtle-hover);
    color: var(--accent);
  }

  .cancel-btn {
    padding: 0.35rem 0.7rem;
    background: var(--btn-subtle-bg);
    border: none;
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .cancel-btn:hover {
    background: var(--btn-subtle-hover);
    color: var(--text-primary);
  }

  .add-start-btn {
    padding: 0.35rem 0.7rem;
    background: transparent;
    border: 1px solid var(--border-light);
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
  }

  .add-start-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .add-start-btn:hover:not(:disabled) {
    border-color: var(--accent);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .submit-btn {
    padding: 0.35rem 0.9rem;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: var(--bg-base);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
  }

  .submit-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }
</style>

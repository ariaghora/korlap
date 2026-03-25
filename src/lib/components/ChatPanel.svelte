<script lang="ts">
  import { messagesByWorkspace, sendingByWorkspace, tokensByWorkspace, type Message } from "$lib/stores/messages.svelte";
  import { searchWorkspaceFiles, suggestReplies, getCachedModels, getModelLabel, type FileSearchResult } from "$lib/ipc";
  import { Lightbulb, BookOpen, Play, ArrowUp, Square, Loader2, Timer, Settings, Pencil, ChevronDown } from "lucide-svelte";
  import { renderMarkdown, renderUserMarkdown } from "$lib/markdown";
  import { externalLinks, copyCodeBlocks, tooltip } from "$lib/actions";
  import MentionInput, { type Mention, type MentionInputValue, type MentionInputApi } from "./MentionInput.svelte";
  import MentionAutocomplete, { type MentionAutocompleteApi } from "./MentionAutocomplete.svelte";
  import VirtualScroller from "./VirtualScroller.svelte";
  import AskUserQuestion from "./chat/AskUserQuestion.svelte";
  import EditDiffBlock from "./chat/EditDiffBlock.svelte";
  import TodoListBlock from "./chat/TodoListBlock.svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import {
    toolIcons,
    buildVisualBlocks,
    type PastedImage,
    type ChatPanelApi,
    type QueueDisplayItem,
    type VisualBlock,
    type ToolEntry,
  } from "$lib/chat-utils";

  // Re-export types so existing imports from ChatPanel.svelte still work
  export type { PastedImage, ChatPanelApi, QueueDisplayItem };

  interface Props {
    workspaceId: string;
    creating?: boolean;
    planMode?: boolean;
    thinkingMode?: boolean;
    model?: string;
    queue?: QueueDisplayItem[];
    contextWarning?: boolean;
    onSend: (prompt: string, images: PastedImage[], mentions: Mention[], planMode: boolean) => void;
    onSendImmediate?: (prompt: string) => void;
    onStop: () => void;
    onRemoveFromQueue?: (id: string) => void;
    onPlanModeChange?: (enabled: boolean) => void;
    onThinkingModeChange?: (enabled: boolean) => void;
    onModelChange?: (model: string) => void;
    onExecutePlan?: () => void;
    onMentionClick?: (path: string) => void;
    onReady?: (api: ChatPanelApi) => void;
  }

  let { workspaceId, creating = false, planMode = false, thinkingMode = false, model = "", queue = [], contextWarning = false, onSend, onSendImmediate, onStop, onRemoveFromQueue, onPlanModeChange, onThinkingModeChange, onModelChange, onExecutePlan, onMentionClick, onReady }: Props = $props();

  let messagesMap = $derived(messagesByWorkspace.get(workspaceId));
  let messages = $derived(messagesMap ? [...messagesMap.values()] : []);
  let sending = $derived(sendingByWorkspace.get(workspaceId) ?? false);

  // Token consumption for this workspace
  let tokens = $derived(tokensByWorkspace.get(workspaceId));
  let totalTokens = $derived(tokens ? tokens.input + tokens.output : 0);

  function formatTokens(n: number): string {
    if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
    if (n >= 10_000) return (n / 1_000).toFixed(0) + "k";
    if (n >= 1_000) return (n / 1_000).toFixed(1) + "k";
    return n.toString();
  }

  // Elapsed timer for "thinking" indicator
  let thinkingStartTime = $state<number | null>(null);
  let thinkingElapsed = $state("0.00");
  let thinkingInterval: ReturnType<typeof setInterval> | undefined;

  $effect(() => {
    if (sending) {
      if (!thinkingStartTime) {
        thinkingStartTime = Date.now();
        thinkingElapsed = "0.00";
        thinkingInterval = setInterval(() => {
          const elapsed = (Date.now() - thinkingStartTime!) / 1000;
          thinkingElapsed = elapsed.toFixed(1);
        }, 50);
      }
    } else {
      if (thinkingInterval) {
        clearInterval(thinkingInterval);
        thinkingInterval = undefined;
      }
      thinkingStartTime = null;
    }
  });

  let pastedImages = $state<PastedImage[]>([]);
  let userScrolledUp = $state(false);
  let inputEl: HTMLDivElement | undefined = $state();
  let showModelDropdown = $state(false);

  let modelLabel = $derived(getModelLabel(model));

  // Mention input + autocomplete state
  let mentionInputApi: MentionInputApi | undefined = $state();

  // Expose ChatPanelApi via callback
  $effect(() => {
    if (mentionInputApi && onReady) {
      onReady({
        addMention: (mention: Mention) => {
          mentionInputApi?.appendMention(mention);
          mentionInputApi?.focus();
        },
        insertText: (text: string) => {
          mentionInputApi?.insertText(text);
          mentionInputApi?.focus();
        },
        refreshSuggestions,
      });
    }
  });
  let autocompleteApi: MentionAutocompleteApi | undefined = $state();
  let autocompleteVisible = $state(false);
  let autocompleteResults = $state<FileSearchResult[]>([]);
  let autocompleteLoading = $state(false);
  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Track which tool groups are expanded (collapsed by default)
  const expandedGroups = new SvelteSet<string>();

  function toggleGroup(key: string) {
    if (expandedGroups.has(key)) {
      expandedGroups.delete(key);
    } else {
      expandedGroups.add(key);
    }
  }

  // Track which thinking blocks are expanded (collapsed by default).
  // Needed because VirtualScroller destroys/recreates DOM elements on scroll,
  // which resets the native <details> open state.
  const expandedThinking = new SvelteSet<string>();

  // Parent-owned state for EditDiffBlock — survives VirtualScroller recycling
  const collapsedDiffs = new SvelteSet<string>();

  // Parent-owned state for TodoListBlock — survives VirtualScroller recycling
  const expandedTodoBlocks = new SvelteSet<string>();

  // Parent-owned state for AskUserQuestion — survives VirtualScroller recycling
  const askSelectedOptions = new SvelteMap<string, SvelteSet<string>>();
  const askCustomInputs = new SvelteMap<string, string>();
  const askShowCustomInput = new SvelteSet<string>();
  const askBatchAnswers = new SvelteMap<string, SvelteMap<number, string>>();
  const askSubmittedBatches = new SvelteSet<string>();

  let visualBlocks = $derived(buildVisualBlocks(messages));


  // Tracks the message count when user clicked Revise — hides plan actions until new messages arrive
  let planActionsHiddenAt = $state<number | null>(null);


  // Show "Execute plan" button only when the most recent user-or-action message
  // is a plan-mode user message and Claude has responded after it.
  let showExecutePlan = $derived.by(() => {
    if (sending || messages.length < 2) return false;
    if (planActionsHiddenAt !== null && messages.length <= planActionsHiddenAt) return false;
    // Find the last non-assistant message (user or action)
    let lastNonAssistantIdx = -1;
    for (let i = messages.length - 1; i >= 0; i--) {
      if (messages[i].role !== "assistant") { lastNonAssistantIdx = i; break; }
    }
    if (lastNonAssistantIdx < 0) return false;
    const msg = messages[lastNonAssistantIdx];
    // Must be a plan-mode user message (not an action like "Executing plan")
    if (msg.role !== "user" || !msg.planMode) return false;
    // And there must be an assistant response after it
    const lastMsg = messages[messages.length - 1];
    return lastMsg.role === "assistant";
  });

  // Files touched in the latest agent turn (after last user message)
  let recentFiles = $derived.by(() => {
    const seen = new Set<string>();
    const files: string[] = [];
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i];
      if (msg.role === "user") break;
      for (const chunk of msg.chunks) {
        if (chunk.type === "tool" && chunk.filePath) {
          const name = chunk.filePath.split("/").pop() ?? chunk.filePath;
          if (!seen.has(name)) {
            seen.add(name);
            files.push(name);
          }
        }
      }
    }
    return files;
  });

  // ── Suggested replies ──────────────────────────────────────────────
  // Called explicitly by the parent when the agent emits "done".
  // No $effect — avoids reactive loops on the messages array.

  let suggestedReplies = $state<string[]>([]);
  let suggestionRequestId = 0; // monotonic counter to discard stale responses

  const CONFIRM_RE = /\b(?:want me to|should i|shall i|would you like|do you want|can i|may i|like me to|ready to|proceed|go ahead|continue|fix (?:th|it|them))\b/i;

  function refreshSuggestions() {
    suggestedReplies = [];

    // Find last assistant text
    let lastText = "";
    for (let i = messages.length - 1; i >= 0; i--) {
      const msg = messages[i];
      if (msg.role === "user") break;
      if (msg.role === "assistant") {
        for (let j = msg.chunks.length - 1; j >= 0; j--) {
          const chunk = msg.chunks[j];
          if (chunk.type === "text") {
            lastText = chunk.content.trim();
            break;
          }
        }
        if (lastText) break;
      }
    }

    if (!lastText) return;

    // Find the last line that ends with "?"
    const questionLine = lastText.split("\n").findLast(line => line.trimEnd().endsWith("?"));
    if (!questionLine) return;

    // Fast path: confirmation questions get instant Yes/No
    const isConfirmation = CONFIRM_RE.test(questionLine);
    if (isConfirmation) {
      suggestedReplies = ["Yes", "No"];
    }

    // Always fire AI call — swaps in smarter suggestions when ready
    const requestId = ++suggestionRequestId;
    suggestReplies(lastText).then((replies) => {
      if (requestId === suggestionRequestId && replies.length > 0) {
        suggestedReplies = replies;
      }
    }).catch(() => {});
  }

  function sendSuggestion(value: string) {
    suggestedReplies = [];
    suggestionRequestId++; // discard any in-flight AI call
    if (onSendImmediate) {
      onSendImmediate(value);
    } else {
      onSend(value, [], [], false);
    }
  }

  // Footer items (file pills, plan actions, suggestions, thinking) rendered
  // as a single extra item at the end of the virtual list so they scroll naturally.
  let hasFooter = $derived(
    (recentFiles.length > 0 && !sending) || showExecutePlan || sending || suggestedReplies.length > 0 || totalTokens > 0,
  );
  let virtualCount = $derived(visualBlocks.length + (hasFooter ? 1 : 0));

  function handleMentionSubmit(value: MentionInputValue) {
    if (creating) return;
    if (!value.text.trim() && value.mentions.length === 0 && pastedImages.length === 0) return;
    const prompt = value.text.trim();
    const images = [...pastedImages];
    const mentions = [...value.mentions];
    pastedImages = [];
    onSend(prompt, images, mentions, planMode);
  }

  function handleQueryChange(query: string | null) {
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
        const results = await searchWorkspaceFiles(workspaceId, query);
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
    autocompleteVisible = false;
    autocompleteResults = [];
  }

  function handleInputKeydown(e: KeyboardEvent) {
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
    }
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
        pastedImages = [
          ...pastedImages,
          { id: crypto.randomUUID(), dataUrl, base64, extension: ext },
        ];
      };
      reader.readAsDataURL(file);
    }
  }

  function removeImage(id: string) {
    pastedImages = pastedImages.filter((img) => img.id !== id);
  }
</script>

<div class="chat-panel">
  {#if creating}
    <div class="chat-area-static">
      <div class="chat-empty">
        <div class="creating-spinner"></div>
        <p class="creating-text">Setting up workspace...</p>
      </div>
    </div>
  {:else if messages.length === 0 && !sending}
    <div class="chat-area-static">
      <div class="chat-empty">
        <p>Send a message to start the agent.</p>
      </div>
    </div>
  {:else}
    <VirtualScroller
      count={virtualCount}
      estimatedHeight={60}
      gap={10}
      overscan={5}
      stickToBottom={!userScrolledUp}
      onscrolledUp={(v) => { userScrolledUp = v; }}
    >
      {#snippet children(index)}
        {#if index < visualBlocks.length}
          {@const block = visualBlocks[index]}
          {@const bi = index}
          <div class="virtual-block">
            {#if block.kind === "action"}
              <div class="action-msg">
                <span class="action-indicator">{block.msg.actionLabel ?? "Action"}</span>
              </div>
            {:else if block.kind === "user"}
              <div class="user-msg">
                {#if block.msg.imageDataUrls && block.msg.imageDataUrls.length > 0}
                  <div class="user-images">
                    {#each block.msg.imageDataUrls as dataUrl, di (di)}
                      <div class="user-image-thumb">
                        <img src={dataUrl} alt="Attached" />
                      </div>
                    {/each}
                  </div>
                {/if}
                {#if block.msg.planMode}
                  <div class="plan-badge-row">
                    <span class="plan-badge">
                      <BookOpen size={11} strokeWidth={2} />
                      Plan
                    </span>
                  </div>
                {/if}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="user-bubble markdown-body"
                  use:externalLinks
                  use:copyCodeBlocks
                  onclick={(e: MouseEvent) => {
                    const chip = (e.target as HTMLElement).closest('[data-mention-path]');
                    if (chip) onMentionClick?.(chip.getAttribute('data-mention-path')!);
                  }}
                >
                  {#each block.msg.chunks as chunk, ci (ci)}
                    {#if chunk.type === "text"}
                      {@html renderUserMarkdown(chunk.content, block.msg.mentions)}
                    {/if}
                  {/each}
                </div>
              </div>
            {:else if block.kind === "assistant-label"}
              <!-- no label needed -->
            {:else if block.kind === "thinking"}
              <div class="assistant-msg">
                <details
                  class="thinking-block"
                  open={expandedThinking.has(block.key)}
                  ontoggle={(e: Event) => {
                    const open = (e.currentTarget as HTMLDetailsElement).open;
                    if (open) expandedThinking.add(block.key);
                    else expandedThinking.delete(block.key);
                  }}
                >
                  <summary class="thinking-summary">
                    <span class="thinking-icon">
                      <Lightbulb size={14} strokeWidth={2} />
                    </span>
                    <span class="thinking-label">Thinking</span>
                    <span class="thinking-chevron"></span>
                  </summary>
                  <div class="thinking-content">
                    <p class="thinking-text">{block.chunk.content}</p>
                  </div>
                </details>
              </div>
            {:else if block.kind === "text"}
              <div class="assistant-msg">
                <div class="assistant-card">
                  <div class="assistant-text markdown-body" use:externalLinks use:copyCodeBlocks>{@html renderMarkdown(block.chunk.content)}</div>
                </div>
              </div>
            {:else if block.kind === "tool-group"}
              {@const isExpanded = expandedGroups.has(block.key)}
              {@const lastTool = block.tools[block.tools.length - 1].chunk}
              {@const LastIcon = toolIcons[lastTool.name] ?? Settings}
              {@const isActive = sending && bi === visualBlocks.length - 1}
              {@const toolCount = block.tools.length}
              <div class="assistant-msg">
                <div class="tool-group" class:expanded={isExpanded}>
                  <button class="tool-group-header" onclick={() => toggleGroup(block.key)}>
                    {#if isActive}
                      <span class="tool-group-spinner"><Loader2 size={13} strokeWidth={2} /></span>
                    {:else}
                      <span class="tool-group-gear"><Settings size={13} strokeWidth={2} /></span>
                    {/if}
                    <span class="tool-group-latest">
                      <span class="tool-group-latest-icon"><LastIcon size={12} strokeWidth={2} /></span>
                      <span class="tool-group-latest-label">{lastTool.input || lastTool.name}</span>
                    </span>
                    {#if toolCount > 1}
                      <span class="tool-group-count">{toolCount} actions</span>
                    {/if}
                    <span class="tool-group-chevron" class:expanded={isExpanded}>▾</span>
                  </button>
                  {#if isExpanded}
                    <div class="tool-group-body">
                      {#each block.tools as t (`${t.msgId}:${t.ci}`)}
                        {@const ToolIcon = toolIcons[t.chunk.name] ?? Settings}
                        <span class="tool-pill">
                          <span class="tool-icon"><ToolIcon size={13} strokeWidth={2} /></span>
                          {t.chunk.input || t.chunk.name}
                        </span>
                      {/each}
                    </div>
                  {/if}
                </div>
              </div>
            {:else if block.kind === "special-tool" && block.chunk.name === "AskUserQuestion"}
              <div class="assistant-msg">
                <AskUserQuestion
                  chunk={block.chunk}
                  batchKey={`${block.msgId}:${block.ci}`}
                  {sending}
                  selectedOptions={askSelectedOptions}
                  customInputs={askCustomInputs}
                  showCustomInput={askShowCustomInput}
                  batchAnswers={askBatchAnswers}
                  submittedBatches={askSubmittedBatches}
                  {onSend}
                  {onSendImmediate}
                />
              </div>
            {:else if block.kind === "special-tool" && block.chunk.oldString != null && block.chunk.newString != null}
              {@const diffKey = `${block.msgId}:${block.ci}`}
              <div class="assistant-msg">
                <EditDiffBlock
                  chunk={block.chunk}
                  collapsed={collapsedDiffs.has(diffKey)}
                  onToggle={() => { collapsedDiffs.has(diffKey) ? collapsedDiffs.delete(diffKey) : collapsedDiffs.add(diffKey); }}
                />
              </div>
            {:else if block.kind === "todo-list"}
              <div class="assistant-msg">
                <TodoListBlock
                  chunk={block.chunk}
                  isLatest={block.isLatest}
                  collapsed={block.isLatest ? false : !expandedTodoBlocks.has(block.key)}
                  onToggle={() => { expandedTodoBlocks.has(block.key) ? expandedTodoBlocks.delete(block.key) : expandedTodoBlocks.add(block.key); }}
                />
              </div>
            {/if}
          </div>
        {:else}
          <!-- Footer: file pills + plan actions + thinking indicator -->
          <div class="virtual-block virtual-footer">
            {#if recentFiles.length > 0 && !sending}
              <div class="file-pills-row">
                {#each recentFiles as file (file)}
                  <span class="file-pill">
                    <span class="file-pill-icon">∞</span>
                    {file}
                  </span>
                {/each}
              </div>
            {/if}
            {#if showExecutePlan}
              <div class="plan-actions-row">
                <button
                  type="button"
                  class="plan-action-btn execute"
                  onclick={() => onExecutePlan?.()}
                >
                  <Play size={14} strokeWidth={2} />
                  Execute plan
                </button>
                <button
                  type="button"
                  class="plan-action-btn revise"
                  onclick={() => { planActionsHiddenAt = messages.length; mentionInputApi?.focus(); }}
                >
                  <Pencil size={14} strokeWidth={2} />
                  Revise
                </button>
              </div>
            {/if}
            {#if suggestedReplies.length > 0 && !sending}
              <div class="suggested-replies">
                {#each suggestedReplies as reply, i (i)}
                  <button
                    type="button"
                    class="suggestion-pill"
                    onclick={() => sendSuggestion(reply)}
                  >{reply}</button>
                {/each}
              </div>
            {/if}
            {#if sending}
              <div class="assistant-msg">
                <div class="thinking">
                  <Timer size={13} strokeWidth={2} />
                  <span class="thinking-timer">{thinkingElapsed}s</span>
                  {#if totalTokens > 0}
                    <span class="token-separator">·</span>
                    <span class="token-count">{formatTokens(totalTokens)} tokens</span>
                  {/if}
                </div>
              </div>
            {:else if totalTokens > 0}
              <div class="assistant-msg">
                <div class="thinking token-summary">
                  <span class="token-count">{formatTokens(totalTokens)} tokens</span>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {/snippet}
    </VirtualScroller>
  {/if}

  {#if queue.length > 0}
    <div class="queue-strip">
      <span class="queue-label">Queued ({queue.length})</span>
      <div class="queue-items">
        {#each queue as item (item.id)}
          <div class="queue-item">
            <span class="queue-text">
              {#if item.planMode}
                <BookOpen size={11} strokeWidth={2} />
              {/if}
              {item.prompt.length > 80 ? item.prompt.slice(0, 80) + '…' : item.prompt}
              {#if item.imageCount > 0}
                <span class="queue-meta">{item.imageCount} img</span>
              {/if}
              {#if item.mentionCount > 0}
                <span class="queue-meta">{item.mentionCount} file{item.mentionCount > 1 ? 's' : ''}</span>
              {/if}
            </span>
            <button
              type="button"
              class="queue-remove"
              onclick={() => onRemoveFromQueue?.(item.id)}
              use:tooltip={{ text: "Remove from queue" }}
            >&times;</button>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if contextWarning && messages.length === 0}
    <div class="context-warning">
      Knowledge base not built — agent will start without repo context
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="input-form" onkeydown={handleInputKeydown} onclick={() => { showModelDropdown = false; }} bind:this={inputEl}>
    {#if pastedImages.length > 0}
      <div class="image-preview-strip">
        {#each pastedImages as img (img.id)}
          <div class="image-preview">
            <img src={img.dataUrl} alt="Pasted" />
            <button
              type="button"
              class="image-remove-btn"
              onclick={() => removeImage(img.id)}
            >
              &times;
            </button>
          </div>
        {/each}
      </div>
    {/if}
    <MentionInput
      placeholder={planMode ? "Describe what to analyze…" : "Ask to make changes, @mention files"}
      disabled={creating}
      onSubmit={handleMentionSubmit}
      onQueryChange={handleQueryChange}
      onPaste={handlePaste}
      bind:ref={mentionInputApi}
    />
    <div class="input-toolbar">
      <div class="toolbar-left">
        <button
          type="button"
          class="mode-pill"
          class:active={thinkingMode}
          onclick={() => onThinkingModeChange?.(!thinkingMode)}
          use:tooltip={{ text: "Extended thinking" }}
        >
          <Lightbulb size={13} strokeWidth={2} />
          Thinking
        </button>
        <button
          type="button"
          class="mode-pill"
          class:active={planMode}
          onclick={() => onPlanModeChange?.(!planMode)}
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
              {#each getCachedModels() as opt (opt.value)}
                <button
                  class="model-option"
                  class:selected={model === opt.value}
                  onclick={() => { onModelChange?.(opt.value); showModelDropdown = false; }}
                >
                  {opt.label}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      </div>
      {#if sending}
        <button type="button" class="stop-btn" onclick={onStop} use:tooltip={{ text: "Stop", shortcut: "Esc" }}>
          <Square size={14} strokeWidth={2.5} />
        </button>
      {:else}
        <button type="button" class="send-btn" disabled={creating}
          onclick={() => mentionInputApi?.submit()}
          use:tooltip={{ text: "Send", shortcut: "⏎" }}
        >
          <ArrowUp size={16} strokeWidth={2.5} />
        </button>
      {/if}
    </div>
    <MentionAutocomplete
      results={autocompleteResults}
      visible={autocompleteVisible}
      loading={autocompleteLoading}
      anchorEl={inputEl ?? null}
      onSelect={handleAutocompleteSelect}
      bind:ref={autocompleteApi}
    />
  </div>
</div>

<style>
  .chat-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  .chat-area-static {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .virtual-block {
    display: flex;
    flex-direction: column;
    padding: 0 1.25rem;
  }

  .virtual-footer {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .chat-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.75rem;
    color: var(--text-dim);
    font-size: 0.85rem;
  }

  .creating-spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-light);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .creating-text {
    color: var(--text-secondary);
    font-size: 0.82rem;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* ── User messages ─────────────────────────── */

  .action-msg {
    align-self: center;
    margin: 0.5rem 0;
  }

  .action-indicator {
    font-size: 0.72rem;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    padding: 0.25rem 0.75rem;
    border-radius: 12px;
    font-weight: 500;
  }

  .user-msg {
    align-self: flex-end;
    max-width: 75%;
  }

  .user-bubble {
    background: var(--border);
    border: 1px solid var(--border-light);
    border-radius: 10px;
    padding: 0.5rem 0.85rem;
    color: var(--text-bright);
    font-size: 0.85rem;
    line-height: 1.5;
    word-break: break-word;
  }

  .user-bubble :global(.msg-mention-chip) {
    display: inline;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--accent);
    border: none;
    border-radius: 4px;
    padding: 0.05rem 0.35rem;
    font-family: var(--font-mono);
    font-size: 0.8rem;
    cursor: pointer;
    white-space: nowrap;
  }

  .user-bubble :global(.msg-mention-chip:hover) {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
  }

  /* ── Assistant messages ────────────────────── */

  .assistant-label {
    font-size: 0.68rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-top: 0.5rem;
    margin-bottom: 0.15rem;
  }

  .assistant-msg {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    max-width: 90%;
  }

  .assistant-card {
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0.6rem 0.85rem;
  }

  .assistant-text {
    margin: 0;
    font-size: 0.85rem;
    line-height: 1.55;
    color: var(--text-primary);
    word-break: break-word;
  }

  /* ── Markdown body (user + assistant messages) ─── */

  .markdown-body :global(h1),
  .markdown-body :global(h2),
  .markdown-body :global(h3),
  .markdown-body :global(h4) {
    margin: 0.6rem 0 0.3rem;
    color: var(--text-bright);
    font-weight: 600;
    line-height: 1.3;
  }

  .markdown-body :global(h1) { font-size: 1.1rem; }
  .markdown-body :global(h2) { font-size: 1rem; }
  .markdown-body :global(h3) { font-size: 0.92rem; }
  .markdown-body :global(h4) { font-size: 0.85rem; }

  .markdown-body :global(p) {
    margin: 0.35rem 0;
    line-height: 1.55;
  }

  .markdown-body :global(> p:first-child) {
    margin-top: 0;
  }

  .markdown-body :global(> p:last-child) {
    margin-bottom: 0;
  }

  .markdown-body :global(ul),
  .markdown-body :global(ol) {
    margin: 0.3rem 0;
    padding-left: 1.5rem;
  }

  .markdown-body :global(li) {
    margin: 0.15rem 0;
    line-height: 1.5;
  }

  .markdown-body :global(li > p) {
    margin: 0.1rem 0;
  }

  .markdown-body :global(strong) {
    color: var(--text-bright);
    font-weight: 600;
  }

  .markdown-body :global(em) {
    font-style: italic;
    color: var(--text-primary);
  }

  .markdown-body :global(a) {
    color: var(--accent);
    text-decoration: none;
  }

  .markdown-body :global(a:hover) {
    text-decoration: underline;
  }

  /* Inline code */
  .markdown-body :global(code) {
    font-family: var(--font-mono);
    font-size: 0.8rem;
    background: var(--bg-active);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0.1rem 0.35rem;
    color: var(--text-bright);
  }

  /* Code blocks */
  .markdown-body :global(pre) {
    margin: 0.4rem 0;
    padding: 0.6rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow-x: auto;
    line-height: 1.5;
  }

  .markdown-body :global(pre code) {
    background: none;
    border: none;
    border-radius: 0;
    padding: 0;
    font-size: 0.78rem;
    color: var(--text-primary);
  }

  .markdown-body :global(.copy-code-btn) {
    position: absolute;
    top: 0.35rem;
    right: 0.35rem;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 0;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s, color 0.15s, border-color 0.15s;
  }

  .markdown-body :global(pre:hover .copy-code-btn) {
    opacity: 1;
  }

  .markdown-body :global(.copy-code-btn:hover) {
    color: var(--text-bright);
    border-color: var(--text-muted);
  }

  /* Tables */
  .markdown-body :global(table) {
    border-collapse: collapse;
    margin: 0.4rem 0;
    font-size: 0.8rem;
    width: 100%;
  }

  .markdown-body :global(th) {
    background: var(--bg-active);
    color: var(--text-bright);
    font-weight: 600;
    text-align: left;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--border);
  }

  .markdown-body :global(td) {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border);
  }

  /* Blockquotes */
  .markdown-body :global(blockquote) {
    margin: 0.4rem 0;
    padding: 0.2rem 0.75rem;
    border-left: 3px solid var(--accent);
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }

  .markdown-body :global(blockquote p) {
    margin: 0.2rem 0;
  }

  /* Horizontal rules */
  .markdown-body :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0.6rem 0;
  }

  /* ── Collapsible tool group ─────────────────── */

  .tool-group {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-card);
  }

  .tool-group-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.35rem 0.65rem;
    background: none;
    border: none;
    cursor: pointer;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 0.73rem;
    text-align: left;
  }

  .tool-group-header:hover {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .tool-group-gear {
    display: flex;
    align-items: center;
    opacity: 0.4;
  }

  .tool-group-latest {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .tool-group-latest-icon {
    display: flex;
    align-items: center;
    opacity: 0.5;
    flex-shrink: 0;
  }

  .tool-group-latest-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tool-group-count {
    opacity: 0.4;
    flex-shrink: 0;
  }

  .tool-group-chevron {
    font-size: 0.65rem;
    opacity: 0.5;
    transition: transform 0.15s ease;
    transform: rotate(-90deg);
  }

  .tool-group-chevron.expanded {
    transform: rotate(0deg);
  }

  .tool-group-spinner {
    display: flex;
    align-items: center;
    color: var(--accent);
    animation: spin 1s linear infinite;
  }

  .tool-group-body {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0.35rem 0.65rem 0.5rem;
    border-top: 1px solid var(--border);
  }

  /* ── Tool use pills ────────────────────────── */

  .tool-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
    padding: 0.25rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    font-size: 0.75rem;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    letter-spacing: -0.01em;
  }

  .tool-icon {
    display: flex;
    align-items: center;
    opacity: 0.6;
  }


  /* ── Thinking indicator (while streaming) ──── */

  .thinking {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    font-size: 0.78rem;
    font-family: var(--font-mono);
    color: var(--accent);
    padding: 0.3rem 0;
    opacity: 0.7;
  }

  .thinking-timer {
    min-width: 4.5ch;
  }

  .token-separator {
    opacity: 0.5;
  }

  .token-count {
    white-space: nowrap;
  }

  .token-summary {
    opacity: 0.5;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.5;
    }
  }

  /* ── Thinking block (collapsible) ──────────── */

  .thinking-block {
    border: 1px solid color-mix(in srgb, var(--accent) 15%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-card));
    overflow: hidden;
  }

  .thinking-summary {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.75rem;
    cursor: pointer;
    user-select: none;
    list-style: none;
    font-size: 0.78rem;
    color: var(--text-dim);
    transition: color 0.15s;
  }

  .thinking-summary::-webkit-details-marker {
    display: none;
  }

  .thinking-summary:hover {
    color: var(--text-secondary);
  }

  .thinking-icon {
    display: flex;
    align-items: center;
    color: var(--accent);
    opacity: 0.7;
  }

  .thinking-label {
    font-weight: 500;
    letter-spacing: 0.01em;
  }

  .thinking-chevron {
    margin-left: auto;
    transition: transform 0.2s ease;
  }

  .thinking-chevron::after {
    content: "▸";
    font-size: 0.7rem;
  }

  .thinking-block[open] .thinking-chevron {
    transform: rotate(90deg);
  }

  .thinking-content {
    padding: 0 0.75rem 0.5rem;
    border-top: 1px solid color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .thinking-text {
    margin: 0.4rem 0 0;
    font-size: 0.8rem;
    line-height: 1.55;
    color: var(--text-dim);
    white-space: pre-wrap;
    word-break: break-word;
    font-style: italic;
  }

  /* ── File pills (inline in chat) ─────────────── */

  .file-pills-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0.2rem 0;
  }

  .file-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    padding: 0.2rem 0.6rem;
    background: var(--bg-card);
    border: 1px solid var(--border);
    border-radius: 14px;
    font-size: 0.73rem;
    color: var(--text-secondary);
    font-family: var(--font-mono);
  }

  .file-pill-icon {
    font-size: 0.8rem;
    opacity: 0.5;
  }

  /* ── User attached images (in chat history) ── */

  .user-images {
    display: flex;
    gap: 0.35rem;
    flex-wrap: wrap;
    justify-content: flex-end;
    margin-bottom: 0.3rem;
  }

  .user-image-thumb {
    width: 64px;
    height: 64px;
    border-radius: 6px;
    overflow: hidden;
    border: 1px solid var(--border-light);
  }

  .user-image-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  /* ── Queue strip ────────── */

  .queue-strip {
    padding: 0.35rem 0.75rem;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-card));
    flex-shrink: 0;
  }

  .queue-label {
    font-size: 0.68rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-weight: 500;
  }

  .queue-items {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin-top: 0.3rem;
  }

  .queue-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0.5rem;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    border-radius: 6px;
    font-size: 0.75rem;
    color: var(--text-secondary);
  }

  .queue-text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }

  .queue-meta {
    font-size: 0.65rem;
    color: var(--text-dim);
    flex-shrink: 0;
  }

  .queue-remove {
    background: none;
    border: none;
    color: var(--text-dim);
    cursor: pointer;
    font-size: 1rem;
    padding: 0 0.2rem;
    flex-shrink: 0;
    line-height: 1;
  }

  .queue-remove:hover {
    color: var(--diff-del);
  }

  /* ── Suggested replies ────────────────────── */

  .suggested-replies {
    display: flex;
    gap: 0.35rem;
    padding: 0 0.75rem 0.35rem;
    flex-shrink: 0;
  }

  .suggestion-pill {
    padding: 0.3rem 0.75rem;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-radius: 14px;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.78rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .suggestion-pill:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  /* ── Input (Slack-style container) ────────── */

  .input-form {
    display: flex;
    flex-direction: column;
    margin: 0 0.75rem 0.6rem;
    border: 1px solid var(--border-light);
    border-radius: 10px;
    background: var(--bg-card);
    overflow: hidden;
  }

  .input-form:focus-within {
    border-color: color-mix(in srgb, var(--accent) 50%, var(--border-light));
  }

  .image-preview-strip {
    display: flex;
    gap: 0.4rem;
    padding: 0.5rem 0.65rem 0;
    flex-wrap: wrap;
  }

  .image-preview {
    position: relative;
    width: 56px;
    height: 56px;
    border-radius: 6px;
    overflow: visible;
    flex-shrink: 0;
  }

  .image-preview img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--border-light);
  }

  .image-remove-btn {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--bg-card);
    border: 1px solid var(--border-light);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    padding: 0;
  }

  .image-remove-btn:hover {
    background: var(--border);
    color: var(--text-bright);
  }

  .input-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.3rem 0.55rem;
  }

  .toolbar-left {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    flex: 1;
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

  /* ── Plan mode badge on user messages ──── */

  .plan-badge-row {
    display: flex;
    justify-content: flex-end;
    margin-bottom: 0.2rem;
  }

  .plan-badge {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.65rem;
    font-weight: 500;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-radius: 10px;
    padding: 0.1rem 0.45rem;
  }

  /* ── Plan action buttons (execute / revise) ── */

  .plan-actions-row {
    display: flex;
    gap: 0.4rem;
    padding: 0.3rem 0;
  }

  .plan-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.85rem;
    border-radius: 8px;
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .plan-action-btn.execute {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
    color: var(--accent);
  }

  .plan-action-btn.execute:hover {
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .plan-action-btn.revise {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-secondary);
  }

  .plan-action-btn.revise:hover {
    border-color: var(--border-light);
    color: var(--text-primary);
  }


  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: var(--text-primary);
    border: none;
    border-radius: 6px;
    color: var(--bg-base);
    cursor: pointer;
    flex-shrink: 0;
  }

  .send-btn:hover:not(:disabled) {
    background: var(--text-bright);
  }

  .send-btn:disabled {
    opacity: 0.25;
    cursor: default;
  }

  .stop-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: var(--diff-del-bg);
    border: 1px solid var(--diff-del);
    border-radius: 6px;
    color: var(--diff-del);
    cursor: pointer;
    flex-shrink: 0;
  }

  .stop-btn:hover {
    filter: brightness(1.2);
  }

  .context-warning {
    padding: 0.35rem 0.75rem;
    font-size: 0.72rem;
    color: var(--text-dim);
    background: color-mix(in srgb, var(--accent) 5%, var(--bg-base));
    border: 1px solid var(--border);
    border-radius: 6px;
    margin: 0 0.5rem 0.35rem;
    text-align: center;
  }
</style>

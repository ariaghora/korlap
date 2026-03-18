<script lang="ts">
  import { messagesByWorkspace, sendingByWorkspace, type Message, type MessageChunk, type MessageMention } from "$lib/stores/messages.svelte";
  import { searchWorkspaceFiles, type FileSearchResult } from "$lib/ipc";
  import { FileText, Pencil, FilePlus, Terminal, FolderSearch, TextSearch, Bot, Globe, Zap, Settings, Lightbulb, BookOpen, Play, ArrowUp, Square, MessageCircleQuestion, Loader2, Timer } from "lucide-svelte";
  import { renderMarkdown } from "$lib/markdown";
  import MentionInput, { type Mention, type MentionInputValue, type MentionInputApi } from "./MentionInput.svelte";
  import MentionAutocomplete, { type MentionAutocompleteApi } from "./MentionAutocomplete.svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";

  const toolIcons: Record<string, typeof Settings> = {
    Read: FileText,
    Edit: Pencil,
    Write: FilePlus,
    Bash: Terminal,
    Glob: FolderSearch,
    Grep: TextSearch,
    Agent: Bot,
    WebFetch: Globe,
    WebSearch: Globe,
    Skill: Zap,
    ToolSearch: Settings,
    AskUserQuestion: MessageCircleQuestion,
  };

  export interface PastedImage {
    id: string;
    dataUrl: string;    // for thumbnail preview
    base64: string;     // raw base64 data (no prefix)
    extension: string;  // png, jpg, etc.
  }

  export interface ChatPanelApi {
    addMention: (mention: Mention) => void;
  }

  interface Props {
    workspaceId: string;
    creating?: boolean;
    planMode?: boolean;
    thinkingMode?: boolean;
    onSend: (prompt: string, images: PastedImage[], mentions: Mention[], planMode: boolean) => void;
    onStop: () => void;
    onPlanModeChange?: (enabled: boolean) => void;
    onThinkingModeChange?: (enabled: boolean) => void;
    onExecutePlan?: () => void;
    onMentionClick?: (path: string) => void;
    onReady?: (api: ChatPanelApi) => void;
  }

  let { workspaceId, creating = false, planMode = false, thinkingMode = false, onSend, onStop, onPlanModeChange, onThinkingModeChange, onExecutePlan, onMentionClick, onReady }: Props = $props();

  /** Split text into segments, replacing @displayName with mention references. */
  type TextSegment = { kind: "text"; value: string } | { kind: "mention"; mention: MessageMention };

  function splitTextWithMentions(text: string, mentions: MessageMention[]): TextSegment[] {
    if (mentions.length === 0) return [{ kind: "text", value: text }];

    // Build regex matching any @displayName, longest first to avoid partial matches
    const sorted = [...mentions].sort((a, b) => b.displayName.length - a.displayName.length);
    const escaped = sorted.map((m) => `@${m.displayName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`);
    const regex = new RegExp(`(${escaped.join("|")})`, "g");

    const segments: TextSegment[] = [];
    let lastIndex = 0;
    let match: RegExpExecArray | null;
    while ((match = regex.exec(text)) !== null) {
      if (match.index > lastIndex) {
        segments.push({ kind: "text", value: text.slice(lastIndex, match.index) });
      }
      const displayName = match[0].slice(1); // strip @
      const mention = mentions.find((m) => m.displayName === displayName);
      if (mention) {
        segments.push({ kind: "mention", mention });
      } else {
        segments.push({ kind: "text", value: match[0] });
      }
      lastIndex = regex.lastIndex;
    }
    if (lastIndex < text.length) {
      segments.push({ kind: "text", value: text.slice(lastIndex) });
    }
    return segments;
  }

  let messages = $derived(messagesByWorkspace.get(workspaceId) ?? []);
  let sending = $derived(sendingByWorkspace.get(workspaceId) ?? false);

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
  let chatArea: HTMLDivElement | undefined = $state();
  let userScrolledUp = $state(false);
  let inputEl: HTMLDivElement | undefined = $state();

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
      });
    }
  });
  let autocompleteApi: MentionAutocompleteApi | undefined = $state();
  let autocompleteVisible = $state(false);
  let autocompleteResults = $state<FileSearchResult[]>([]);
  let autocompleteLoading = $state(false);
  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;

  // Track which edit diffs are collapsed (by "msgId:chunkIdx" key)
  const collapsedDiffs = new SvelteSet<string>();

  // Track which tool groups are expanded (collapsed by default)
  const expandedGroups = new SvelteSet<string>();

  function toggleGroup(key: string) {
    if (expandedGroups.has(key)) {
      expandedGroups.delete(key);
    } else {
      expandedGroups.add(key);
    }
  }

  // Build visual blocks from the messages array, grouping consecutive tool calls
  // across message boundaries into collapsible groups.
  type ToolEntry = { chunk: MessageChunk & { type: "tool" }; msgId: string; ci: number };
  type VisualBlock =
    | { kind: "user"; msg: Message; key: string }
    | { kind: "action"; msg: Message; key: string }
    | { kind: "assistant-label"; key: string }
    | { kind: "thinking"; chunk: MessageChunk & { type: "thinking" }; key: string }
    | { kind: "text"; chunk: MessageChunk & { type: "text" }; msgId: string; key: string }
    | { kind: "special-tool"; chunk: MessageChunk & { type: "tool" }; msgId: string; ci: number; key: string }
    | { kind: "tool-group"; tools: ToolEntry[]; key: string };

  function buildVisualBlocks(msgs: Message[]): VisualBlock[] {
    const blocks: VisualBlock[] = [];
    let pendingTools: ToolEntry[] = [];
    let lastRole: string | null = null;

    function flushTools() {
      if (pendingTools.length > 0) {
        blocks.push({ kind: "tool-group", tools: pendingTools, key: `tg:${pendingTools[0].msgId}:${pendingTools[0].ci}` });
        pendingTools = [];
      }
    }

    for (const msg of msgs) {
      if (msg.role === "user" || msg.role === "action") {
        flushTools();
        blocks.push({
          kind: msg.role as "user" | "action",
          msg,
          key: msg.id,
        });
        lastRole = msg.role;
        continue;
      }

      // assistant message
      if (lastRole !== "assistant") {
        flushTools();
        blocks.push({ kind: "assistant-label", key: `label:${msg.id}` });
      }
      lastRole = "assistant";

      for (let ci = 0; ci < msg.chunks.length; ci++) {
        const chunk = msg.chunks[ci];
        if (chunk.type === "thinking") {
          flushTools();
          blocks.push({ kind: "thinking", chunk, key: `think:${msg.id}` });
        } else if (chunk.type === "text") {
          flushTools();
          blocks.push({ kind: "text", chunk, msgId: msg.id, key: `text:${msg.id}` });
        } else if (chunk.type === "tool") {
          const isSpecial = chunk.name === "AskUserQuestion" ||
                            (chunk.oldString != null && chunk.newString != null);
          if (isSpecial) {
            flushTools();
            blocks.push({ kind: "special-tool", chunk, msgId: msg.id, ci, key: `st:${msg.id}:${ci}` });
          } else {
            pendingTools.push({ chunk, msgId: msg.id, ci });
          }
        }
      }
    }
    flushTools();
    return blocks;
  }

  let visualBlocks = $derived(buildVisualBlocks(messages));

  // AskUserQuestion: multi-select toggles and custom input per question
  // qKey = "msgId:chunkIdx:questionIdx" — identifies a single question
  // batchKey = "msgId:chunkIdx" — identifies the entire AskUserQuestion tool call
  const selectedOptions = new SvelteMap<string, SvelteSet<string>>();
  const customInputs = new SvelteMap<string, string>();
  const showCustomInput = new SvelteSet<string>();
  // Stores each question's answer within a batch: batchKey → Map<questionIdx, answerText>
  const batchAnswers = new SvelteMap<string, SvelteMap<number, string>>();
  // Tracks which batches have been fully submitted
  const submittedBatches = new SvelteSet<string>();

  function batchKeyOf(qKey: string): string {
    const lastColon = qKey.lastIndexOf(":");
    return qKey.substring(0, lastColon);
  }

  function questionIdxOf(qKey: string): number {
    const lastColon = qKey.lastIndexOf(":");
    return parseInt(qKey.substring(lastColon + 1), 10);
  }

  /** Store an answer for one question in the batch. If single-question batch, submit immediately. */
  function recordAnswer(qKey: string, answer: string, totalInBatch: number) {
    const bKey = batchKeyOf(qKey);
    const qi = questionIdxOf(qKey);
    let answers = batchAnswers.get(bKey);
    if (!answers) {
      answers = new SvelteMap<number, string>();
      batchAnswers.set(bKey, answers);
    }
    answers.set(qi, answer);

    // Single-question batch: submit immediately
    if (totalInBatch === 1) {
      submitBatch(bKey, totalInBatch);
    }
  }

  /** Format all answers in the batch and send as one message. */
  function submitBatch(batchKey: string, totalInBatch: number) {
    const answers = batchAnswers.get(batchKey);
    if (!answers || answers.size < totalInBatch) return;

    submittedBatches.add(batchKey);

    if (totalInBatch === 1) {
      onSend(answers.get(0)!, [], [], false);
    } else {
      const parts: string[] = [];
      for (let i = 0; i < totalInBatch; i++) {
        parts.push(`${i + 1}. ${answers.get(i) ?? "(no answer)"}`);
      }
      onSend(parts.join("\n"), [], [], false);
    }
  }

  /** Re-open a submitted batch for editing. Clears all answers so user re-selects. */
  function unsubmitBatch(batchKey: string) {
    submittedBatches.delete(batchKey);
    batchAnswers.delete(batchKey);
    // Clear per-question state for this batch
    for (const key of [...selectedOptions.keys()]) {
      if (key.startsWith(batchKey + ":")) {
        selectedOptions.delete(key);
        customInputs.delete(key);
        showCustomInput.delete(key);
      }
    }
  }

  function toggleOption(key: string, label: string) {
    let current = selectedOptions.get(key);
    if (!current) {
      current = new SvelteSet<string>();
      selectedOptions.set(key, current);
    }
    if (current.has(label)) {
      current.delete(label);
    } else {
      current.add(label);
    }
  }

  function submitMultiSelect(key: string, totalInBatch: number) {
    const selected = selectedOptions.get(key);
    if (!selected || selected.size === 0) return;
    const custom = customInputs.get(key)?.trim();
    const parts = [...selected];
    if (custom) parts.push(custom);
    recordAnswer(key, parts.join(", "), totalInBatch);
  }

  function submitCustomInput(key: string, totalInBatch: number) {
    const text = customInputs.get(key)?.trim();
    if (!text) return;
    recordAnswer(key, text, totalInBatch);
  }

  function submitOption(key: string, label: string, totalInBatch: number) {
    recordAnswer(key, label, totalInBatch);
  }

  // Tracks the message count when user clicked Revise — hides plan actions until new messages arrive
  let planActionsHiddenAt = $state<number | null>(null);

  function handleScroll(e: Event) {
    const el = e.target as HTMLElement;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    userScrolledUp = !atBottom;
  }

  $effect(() => {
    messages.length;
    sending;
    if (!userScrolledUp && chatArea) {
      requestAnimationFrame(() => {
        chatArea!.scrollTop = chatArea!.scrollHeight;
      });
    }
  });

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

  function handleMentionSubmit(value: MentionInputValue) {
    if (sending || creating) return;
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
  <div class="chat-area" bind:this={chatArea} onscroll={handleScroll}>
    {#if creating}
      <div class="chat-empty">
        <div class="creating-spinner"></div>
        <p class="creating-text">Setting up workspace...</p>
      </div>
    {:else if messages.length === 0 && !sending}
      <div class="chat-empty">
        <p>Send a message to start the agent.</p>
      </div>
    {:else}
      {#each visualBlocks as block, bi (block.key)}
        {#if block.kind === "action"}
          <div class="action-msg">
            <span class="action-indicator">{block.msg.actionLabel ?? "Action"}</span>
          </div>
        {:else if block.kind === "user"}
          <div class="user-msg">
            {#if block.msg.imageDataUrls && block.msg.imageDataUrls.length > 0}
              <div class="user-images">
                {#each block.msg.imageDataUrls as dataUrl}
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
            <div class="user-bubble">
              {#each block.msg.chunks as chunk}
                {#if chunk.type === "text"}
                  {#if block.msg.mentions && block.msg.mentions.length > 0}
                    {#each splitTextWithMentions(chunk.content, block.msg.mentions) as seg}
                      {#if seg.kind === "text"}{seg.value}{:else}
                        <button
                          class="msg-mention-chip"
                          onclick={() => onMentionClick?.(seg.mention.path)}
                        >@{seg.mention.displayName}</button>
                      {/if}
                    {/each}
                  {:else}
                    {chunk.content}
                  {/if}
                {/if}
              {/each}
            </div>
          </div>
        {:else if block.kind === "assistant-label"}
          <!-- no label needed -->
        {:else if block.kind === "thinking"}
          <div class="assistant-msg">
            <details class="thinking-block">
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
              <div class="assistant-text markdown-body">{@html renderMarkdown(block.chunk.content)}</div>
            </div>
          </div>
        {:else if block.kind === "tool-group"}
          {@const isExpanded = expandedGroups.has(block.key)}
          {@const lastTool = block.tools[block.tools.length - 1].chunk}
          {@const LastIcon = toolIcons[lastTool.name] ?? Settings}
          {@const isActive = sending && bi === visualBlocks.length - 1}
          {@const count = block.tools.length}
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
                {#if count > 1}
                  <span class="tool-group-count">{count} actions</span>
                {/if}
                <span class="tool-group-chevron" class:expanded={isExpanded}>▾</span>
              </button>
              {#if isExpanded}
                <div class="tool-group-body">
                  {#each block.tools as t}
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
          {@const chunk = block.chunk}
          {@const parsed = (() => { try { return JSON.parse(chunk.input); } catch { return null; } })()}
          {@const bKey = `${block.msgId}:${block.ci}`}
          {@const totalQ = parsed && Array.isArray(parsed) ? parsed.length : 0}
          {@const batchSubmitted = submittedBatches.has(bKey)}
          {@const bAnswers = batchAnswers.get(bKey)}
          {@const answeredInBatch = bAnswers?.size ?? 0}
          <div class="assistant-msg">
            {#if parsed && Array.isArray(parsed)}
              {#if batchSubmitted}
                <!-- Collapsed summary after submission -->
                <div class="question-card answered">
                  <div class="question-header">
                    <span class="question-icon"><MessageCircleQuestion size={15} strokeWidth={2} /></span>
                    <span class="question-label">{totalQ === 1 ? (parsed[0].header || "Question") : `${totalQ} questions`}</span>
                    <button
                      type="button"
                      class="question-change-btn"
                      disabled={sending}
                      onclick={() => unsubmitBatch(bKey)}
                    >Change</button>
                  </div>
                  <div class="question-answers-summary">
                    {#each parsed as q, qi}
                      <div class="answer-summary-row">
                        <span class="answer-summary-label">{q.header || q.question || `Q${qi + 1}`}</span>
                        <span class="question-answer-pill">{bAnswers?.get(qi)}</span>
                      </div>
                    {/each}
                  </div>
                </div>
              {:else}
                <!-- Expanded: individual question cards -->
                {#each parsed as q, qi}
                  {@const qKey = `${block.msgId}:${block.ci}:${qi}`}
                  {@const isMulti = q.multiSelect === true}
                  {@const answerText = bAnswers?.get(qi)}
                  {@const hasAnswer = answerText != null}
                  {@const selected = selectedOptions.get(qKey) ?? new SvelteSet()}
                  {@const customText = customInputs.get(qKey) ?? ""}
                  {@const showCustom = showCustomInput.has(qKey)}
                  <div class="question-card">
                    <div class="question-header">
                      <span class="question-icon"><MessageCircleQuestion size={15} strokeWidth={2} /></span>
                      <span class="question-label">{q.header || "Question"}</span>
                      {#if isMulti}
                        <span class="question-multi-badge">Multi-select</span>
                      {/if}
                      {#if hasAnswer}
                        <span class="question-answer-pill">{answerText}</span>
                      {/if}
                    </div>
                    {#if q.question}
                      <div class="question-text">{q.question}</div>
                    {/if}
                    {#if q.options && q.options.length > 0}
                      <div class="question-options">
                        {#each q.options as opt}
                          {#if isMulti}
                            <button
                              type="button"
                              class="question-option"
                              class:selected={selected.has(opt.label)}
                              disabled={sending}
                              onclick={() => toggleOption(qKey, opt.label)}
                            >
                              <span class="option-check">{selected.has(opt.label) ? "◉" : "○"}</span>
                              <span class="option-content">
                                <span class="option-label">{opt.label}</span>
                                {#if opt.description}
                                  <span class="option-desc">{opt.description}</span>
                                {/if}
                              </span>
                            </button>
                          {:else}
                            <button
                              type="button"
                              class="question-option"
                              class:selected-answer={hasAnswer && answerText === opt.label}
                              disabled={sending}
                              onclick={() => submitOption(qKey, opt.label, totalQ)}
                            >
                              <span class="option-content">
                                <span class="option-label">{opt.label}</span>
                                {#if opt.description}
                                  <span class="option-desc">{opt.description}</span>
                                {/if}
                              </span>
                            </button>
                          {/if}
                        {/each}
                        {#if showCustom}
                          <div class="custom-input-row">
                            <input
                              type="text"
                              class="custom-input"
                              placeholder="Type your answer…"
                              value={customText}
                              disabled={sending}
                              oninput={(e) => customInputs.set(qKey, (e.target as HTMLInputElement).value)}
                              onkeydown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); isMulti ? submitMultiSelect(qKey, totalQ) : submitCustomInput(qKey, totalQ); } }}
                            />
                            <button
                              type="button"
                              class="custom-submit-btn"
                              disabled={sending || (!customText.trim() && (!isMulti || selected.size === 0))}
                              onclick={() => isMulti ? submitMultiSelect(qKey, totalQ) : submitCustomInput(qKey, totalQ)}
                            >
                              <ArrowUp size={14} strokeWidth={2.5} />
                            </button>
                          </div>
                        {:else}
                          <button
                            type="button"
                            class="question-option other-option"
                            disabled={sending}
                            onclick={() => showCustomInput.add(qKey)}
                          >
                            <span class="option-content">
                              <span class="option-label">Other</span>
                              <span class="option-desc">Type a custom answer</span>
                            </span>
                          </button>
                        {/if}
                        {#if isMulti && selected.size > 0}
                          <button
                            type="button"
                            class="multi-submit-btn"
                            disabled={sending}
                            onclick={() => submitMultiSelect(qKey, totalQ)}
                          >
                            Submit ({selected.size} selected)
                          </button>
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/each}
                {#if totalQ > 1 && answeredInBatch >= totalQ}
                  <button
                    type="button"
                    class="batch-submit-btn"
                    disabled={sending}
                    onclick={() => submitBatch(bKey, totalQ)}
                  >
                    <ArrowUp size={14} strokeWidth={2.5} />
                    Submit all {totalQ} answers
                  </button>
                {:else if totalQ > 1 && answeredInBatch > 0}
                  <div class="batch-progress">
                    {answeredInBatch} of {totalQ} questions answered
                  </div>
                {/if}
              {/if}
            {:else}
              <div class="question-card">
                <div class="question-header">
                  <span class="question-icon"><MessageCircleQuestion size={15} strokeWidth={2} /></span>
                  <span class="question-label">Question</span>
                </div>
                <div class="question-text">{chunk.input}</div>
              </div>
            {/if}
          </div>
        {:else if block.kind === "special-tool" && block.chunk.oldString != null && block.chunk.newString != null}
          {@const chunk = block.chunk}
          {@const diffKey = `${block.msgId}:${block.ci}`}
          {@const isCollapsed = collapsedDiffs.has(diffKey)}
          <div class="assistant-msg">
            <div class="edit-diff-block">
              <button class="edit-diff-header" onclick={() => {
                if (collapsedDiffs.has(diffKey)) {
                  collapsedDiffs.delete(diffKey);
                } else {
                  collapsedDiffs.add(diffKey);
                }
              }}>
                <span class="edit-diff-chevron" class:collapsed={isCollapsed}>▾</span>
                <span class="edit-diff-icon"><Pencil size={13} strokeWidth={2} /></span>
                <span class="edit-diff-label">{chunk.input}</span>
              </button>
              {#if !isCollapsed}
                <div class="edit-diff-body">
                  {#each (chunk.oldString ?? "").split("\n") as line, li}
                    <div class="diff-line remove"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">-</span><span class="diff-code">{line}</span></div>
                  {/each}
                  {#each (chunk.newString ?? "").split("\n") as line, li}
                    <div class="diff-line add"><span class="diff-ln">{li + 1}</span><span class="diff-prefix">+</span><span class="diff-code">{line}</span></div>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        {/if}
      {/each}

      <!-- File pills after last assistant turn -->
      {#if recentFiles.length > 0 && !sending}
        <div class="file-pills-row">
          {#each recentFiles as file}
            <span class="file-pill">
              <span class="file-pill-icon">∞</span>
              {file}
            </span>
          {/each}
        </div>
      {/if}

      <!-- Plan approval buttons after a plan-mode response -->
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

      {#if sending}
        <div class="assistant-msg">
          <div class="thinking">
            <Timer size={13} strokeWidth={2} />
            <span class="thinking-timer">{thinkingElapsed}s</span>
          </div>
        </div>
      {/if}
    {/if}
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="input-form" onkeydown={handleInputKeydown} bind:this={inputEl}>
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
      disabled={creating || sending}
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
          title="Extended thinking: deeper reasoning before responding"
        >
          <Lightbulb size={13} strokeWidth={2} />
          Thinking
        </button>
        <button
          type="button"
          class="mode-pill"
          class:active={planMode}
          onclick={() => onPlanModeChange?.(!planMode)}
          title="Plan mode: analyze and plan without making changes"
        >
          <BookOpen size={13} strokeWidth={2} />
          Plan
        </button>
      </div>
      {#if sending}
        <button type="button" class="stop-btn" onclick={onStop} title="Stop">
          <Square size={14} strokeWidth={2.5} />
        </button>
      {:else}
        <button type="button" class="send-btn" disabled={creating}
          onclick={() => mentionInputApi?.submit()}
          title="Send"
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

  .chat-area {
    flex: 1;
    overflow-y: auto;
    padding: 1rem 1.25rem;
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
    white-space: pre-wrap;
    word-break: break-word;
  }

  .msg-mention-chip {
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

  .msg-mention-chip:hover {
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

  /* ── Markdown body (rendered assistant text) ─── */

  .assistant-text.markdown-body :global(h1),
  .assistant-text.markdown-body :global(h2),
  .assistant-text.markdown-body :global(h3),
  .assistant-text.markdown-body :global(h4) {
    margin: 0.6rem 0 0.3rem;
    color: var(--text-bright);
    font-weight: 600;
    line-height: 1.3;
  }

  .assistant-text.markdown-body :global(h1) { font-size: 1.1rem; }
  .assistant-text.markdown-body :global(h2) { font-size: 1rem; }
  .assistant-text.markdown-body :global(h3) { font-size: 0.92rem; }
  .assistant-text.markdown-body :global(h4) { font-size: 0.85rem; }

  .assistant-text.markdown-body :global(p) {
    margin: 0.35rem 0;
    line-height: 1.55;
  }

  .assistant-text.markdown-body :global(> p:first-child) {
    margin-top: 0;
  }

  .assistant-text.markdown-body :global(> p:last-child) {
    margin-bottom: 0;
  }

  .assistant-text.markdown-body :global(ul),
  .assistant-text.markdown-body :global(ol) {
    margin: 0.3rem 0;
    padding-left: 1.5rem;
  }

  .assistant-text.markdown-body :global(li) {
    margin: 0.15rem 0;
    line-height: 1.5;
  }

  .assistant-text.markdown-body :global(li > p) {
    margin: 0.1rem 0;
  }

  .assistant-text.markdown-body :global(strong) {
    color: var(--text-bright);
    font-weight: 600;
  }

  .assistant-text.markdown-body :global(em) {
    font-style: italic;
    color: var(--text-primary);
  }

  .assistant-text.markdown-body :global(a) {
    color: var(--accent);
    text-decoration: none;
  }

  .assistant-text.markdown-body :global(a:hover) {
    text-decoration: underline;
  }

  /* Inline code */
  .assistant-text.markdown-body :global(code) {
    font-family: var(--font-mono);
    font-size: 0.8rem;
    background: var(--bg-active);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0.1rem 0.35rem;
    color: var(--text-bright);
  }

  /* Code blocks */
  .assistant-text.markdown-body :global(pre) {
    margin: 0.4rem 0;
    padding: 0.6rem 0.75rem;
    background: var(--bg-sidebar);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow-x: auto;
    line-height: 1.5;
  }

  .assistant-text.markdown-body :global(pre code) {
    background: none;
    border: none;
    border-radius: 0;
    padding: 0;
    font-size: 0.78rem;
    color: var(--text-primary);
  }

  /* Tables */
  .assistant-text.markdown-body :global(table) {
    border-collapse: collapse;
    margin: 0.4rem 0;
    font-size: 0.8rem;
    width: 100%;
  }

  .assistant-text.markdown-body :global(th) {
    background: var(--bg-active);
    color: var(--text-bright);
    font-weight: 600;
    text-align: left;
    padding: 0.35rem 0.6rem;
    border: 1px solid var(--border);
  }

  .assistant-text.markdown-body :global(td) {
    padding: 0.3rem 0.6rem;
    border: 1px solid var(--border);
  }

  /* Blockquotes */
  .assistant-text.markdown-body :global(blockquote) {
    margin: 0.4rem 0;
    padding: 0.2rem 0.75rem;
    border-left: 3px solid var(--accent);
    color: var(--text-secondary);
    background: color-mix(in srgb, var(--accent) 4%, transparent);
  }

  .assistant-text.markdown-body :global(blockquote p) {
    margin: 0.2rem 0;
  }

  /* Horizontal rules */
  .assistant-text.markdown-body :global(hr) {
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

  /* ── Inline edit diff ─────────────────────── */

  .edit-diff-block {
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--bg-card);
  }

  .edit-diff-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    width: 100%;
    padding: 0.4rem 0.7rem;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 0.73rem;
    text-align: left;
  }

  .edit-diff-header:hover {
    background: color-mix(in srgb, var(--accent) 5%, transparent);
  }

  .edit-diff-chevron {
    font-size: 0.65rem;
    opacity: 0.5;
    transition: transform 0.15s ease;
  }

  .edit-diff-chevron.collapsed {
    transform: rotate(-90deg);
  }

  .edit-diff-icon {
    display: flex;
    align-items: center;
    opacity: 0.6;
    color: var(--accent);
  }

  .edit-diff-label {
    color: var(--text-secondary);
  }

  .edit-diff-body {
    overflow: auto;
    max-height: 300px;
    font-family: var(--font-mono);
    font-size: 0.75rem;
    line-height: 1.55;
  }

  .edit-diff-body .diff-line {
    display: flex;
    padding: 0 0.7rem;
    white-space: pre;
  }

  .edit-diff-body .diff-line.add {
    background: var(--diff-add-bg);
    color: var(--diff-add);
  }

  .edit-diff-body .diff-line.remove {
    background: var(--diff-del-bg);
    color: var(--diff-del);
  }

  .edit-diff-body .diff-ln {
    display: inline-block;
    width: 3ch;
    flex-shrink: 0;
    text-align: right;
    padding-right: 0.5ch;
    user-select: none;
    opacity: 0.35;
    color: var(--text-dim);
  }

  .edit-diff-body .diff-prefix {
    display: inline-block;
    width: 1.5ch;
    flex-shrink: 0;
    user-select: none;
    opacity: 0.7;
  }

  .edit-diff-body .diff-code {
    flex: 1;
    min-width: 0;
  }

  /* ── AskUserQuestion card ────────────────────── */

  .question-card {
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 6%, var(--bg-card));
    overflow: hidden;
  }

  .question-card.answered {
    opacity: 0.65;
    border-color: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  .question-header {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding: 0.4rem 0.75rem;
    border-bottom: 1px solid color-mix(in srgb, var(--accent) 15%, transparent);
    font-size: 0.72rem;
    font-weight: 500;
    color: var(--accent);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .question-icon {
    display: flex;
    align-items: center;
    opacity: 0.8;
  }

  .question-text {
    padding: 0.55rem 0.75rem;
    font-size: 0.85rem;
    line-height: 1.55;
    color: var(--text-primary);
    white-space: pre-wrap;
    word-break: break-word;
  }

  .question-options {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.4rem 0.75rem 0.6rem;
  }

  .question-multi-badge {
    margin-left: auto;
    font-size: 0.62rem;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-dim);
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    padding: 0.1rem 0.4rem;
    border-radius: 8px;
  }

  .question-option {
    display: flex;
    align-items: flex-start;
    gap: 0.5rem;
    padding: 0.45rem 0.7rem;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-base));
    border: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
    border-radius: 6px;
    text-align: left;
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .question-option:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 12%, var(--bg-base));
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }

  .question-option.selected {
    background: color-mix(in srgb, var(--accent) 15%, var(--bg-base));
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .question-option:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .question-option.other-option {
    border-style: dashed;
  }

  .option-check {
    flex-shrink: 0;
    font-size: 0.85rem;
    line-height: 1;
    color: var(--accent);
    margin-top: 0.1rem;
  }

  .option-content {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
  }

  .option-label {
    font-size: 0.82rem;
    font-weight: 500;
    color: var(--text-bright);
  }

  .option-desc {
    font-size: 0.75rem;
    color: var(--text-dim);
    line-height: 1.4;
  }

  .custom-input-row {
    display: flex;
    gap: 0.4rem;
    align-items: center;
  }

  .custom-input {
    flex: 1;
    padding: 0.45rem 0.7rem;
    background: color-mix(in srgb, var(--accent) 4%, var(--bg-base));
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-radius: 6px;
    color: var(--text-bright);
    font-family: inherit;
    font-size: 0.82rem;
    outline: none;
  }

  .custom-input::placeholder {
    color: var(--text-dim);
  }

  .custom-input:focus {
    border-color: var(--accent);
  }

  .custom-submit-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    background: var(--accent);
    border: none;
    border-radius: 6px;
    color: var(--bg-base);
    cursor: pointer;
    flex-shrink: 0;
  }

  .custom-submit-btn:hover:not(:disabled) {
    filter: brightness(1.1);
  }

  .custom-submit-btn:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .multi-submit-btn {
    padding: 0.4rem 0.75rem;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
    border-radius: 6px;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.8rem;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .multi-submit-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 20%, transparent);
    border-color: var(--accent);
  }

  .multi-submit-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ── Selected single-select answer highlight ──── */
  .question-option.selected-answer {
    background: color-mix(in srgb, var(--accent) 15%, var(--bg-base));
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  /* ── Answer pill shown in header after answering ──── */
  .question-answer-pill {
    margin-left: auto;
    font-size: 0.68rem;
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-bright);
    background: color-mix(in srgb, var(--accent) 18%, transparent);
    padding: 0.12rem 0.5rem;
    border-radius: 8px;
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Batch submit button (multi-question) ──── */
  .batch-submit-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.4rem;
    width: 100%;
    padding: 0.55rem 0.75rem;
    margin-top: 0.3rem;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    border: 1px solid color-mix(in srgb, var(--accent) 50%, transparent);
    border-radius: 8px;
    color: var(--accent);
    font-family: inherit;
    font-size: 0.82rem;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .batch-submit-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 25%, transparent);
    border-color: var(--accent);
  }

  .batch-submit-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ── Collapsed answer summary (after submission) ──── */
  .question-answers-summary {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    padding: 0.35rem 0.75rem 0.45rem;
  }

  .answer-summary-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.78rem;
  }

  .answer-summary-label {
    color: var(--text-dim);
    flex-shrink: 0;
  }

  /* ── Change button in collapsed header ──── */
  .question-change-btn {
    margin-left: auto;
    background: none;
    border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
    border-radius: 6px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.65rem;
    font-weight: 500;
    padding: 0.1rem 0.45rem;
    cursor: pointer;
    text-transform: none;
    letter-spacing: 0;
    transition: all 0.15s ease;
  }

  .question-change-btn:hover:not(:disabled) {
    color: var(--accent);
    border-color: color-mix(in srgb, var(--accent) 50%, transparent);
  }

  .question-change-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* ── Batch progress indicator ──── */
  .batch-progress {
    text-align: center;
    font-size: 0.72rem;
    color: var(--text-dim);
    padding: 0.4rem 0 0.1rem;
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
</style>

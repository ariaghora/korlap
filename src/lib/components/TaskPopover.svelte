<script lang="ts">
  import { type PastedImage } from "./ChatPanel.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { X } from "lucide-svelte";

  export interface TaskData {
    title: string;
    description: string;
    newImages: PastedImage[];
    existingPaths: string[];
  }

  interface Props {
    initialTitle?: string;
    initialDescription?: string;
    initialImagePaths?: string[];
    submitLabel?: string;
    onSubmit: (data: TaskData) => void;
    onCancel: () => void;
  }

  let {
    initialTitle = "",
    initialDescription = "",
    initialImagePaths = [],
    submitLabel = "Add",
    onSubmit,
    onCancel,
  }: Props = $props();

  let title = $state(initialTitle);
  let description = $state(initialDescription);
  let existingPaths = $state<string[]>([...initialImagePaths]);
  let newImages = $state<PastedImage[]>([]);
  let titleRef: HTMLInputElement | undefined = $state();
  let descRef: HTMLTextAreaElement | undefined = $state();

  $effect(() => {
    requestAnimationFrame(() => titleRef?.focus());
  });

  let canSubmit = $derived(title.trim().length > 0 || existingPaths.length > 0 || newImages.length > 0);

  function submit() {
    if (!canSubmit) return;
    onSubmit({ title: title.trim(), description: description.trim(), newImages: [...newImages], existingPaths: [...existingPaths] });
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
    if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      submit();
    }
  }

  function handleTitleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.metaKey) {
      e.preventDefault();
      descRef?.focus();
    }
    handleKeydown(e);
  }

  function handleDescKeydown(e: KeyboardEvent) {
    handleKeydown(e);
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
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay" onclick={onCancel} onkeydown={handleKeydown}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <input
      class="task-title"
      bind:this={titleRef}
      bind:value={title}
      onkeydown={handleTitleKeydown}
      onpaste={handlePaste}
      placeholder="Task title"
    />
    <textarea
      class="task-desc"
      bind:this={descRef}
      bind:value={description}
      onkeydown={handleDescKeydown}
      onpaste={handlePaste}
      rows={6}
      placeholder="Description (optional) — paste images here"
    ></textarea>
    {#if existingPaths.length > 0 || newImages.length > 0}
      <div class="image-strip">
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
      <span class="footer-hint">⌘Enter to {submitLabel.toLowerCase()} · Esc cancel · Paste images</span>
      <div class="footer-actions">
        <button class="cancel-btn" onclick={onCancel}>Cancel</button>
        <button class="submit-btn" onclick={submit} disabled={!canSubmit}>{submitLabel}</button>
      </div>
    </div>
  </div>
</div>

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

  .task-desc {
    width: 100%;
    box-sizing: border-box;
    padding: 0.55rem 0.65rem;
    background: var(--input-inset-bg);
    border: none;
    border-radius: 8px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.88rem;
    line-height: 1.5;
    outline: none;
    resize: vertical;
    min-height: 120px;
  }

  .task-desc::placeholder {
    color: var(--text-muted);
  }

  .task-desc:focus {
    background: var(--input-inset-focus);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 35%, transparent);
  }

  .image-strip {
    display: flex;
    gap: 0.4rem;
    flex-wrap: wrap;
    padding: 0.15rem 0;
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
    align-items: center;
    justify-content: space-between;
    margin-top: 0.15rem;
  }

  .footer-hint {
    font-size: 0.65rem;
    color: var(--text-muted);
    opacity: 0.7;
  }

  .footer-actions {
    display: flex;
    gap: 0.35rem;
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

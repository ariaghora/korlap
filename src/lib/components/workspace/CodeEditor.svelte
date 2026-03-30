<script lang="ts">
  import { untrack } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, rectangularSelection, hoverTooltip, Decoration, type DecorationSet } from "@codemirror/view";
  import { EditorState, Compartment, StateField, StateEffect, type Extension, RangeSetBuilder } from "@codemirror/state";
  import { lspHover, lspGotoDefinition, lspDiagnostics, type LspDiagnostic } from "$lib/ipc";
  import { renderMarkdown } from "$lib/markdown";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import { bracketMatching, foldGutter, foldKeymap, indentOnInput, syntaxHighlighting, HighlightStyle } from "@codemirror/language";
  import { tags } from "@lezer/highlight";
  import { getEditorColors, getEditorColorsLight } from "$lib/stores/theme.svelte";
  import type { EditorColors } from "$lib/themes";

  // Language imports
  import { javascript } from "@codemirror/lang-javascript";
  import { rust } from "@codemirror/lang-rust";
  import { python } from "@codemirror/lang-python";
  import { html } from "@codemirror/lang-html";
  import { css } from "@codemirror/lang-css";
  import { json } from "@codemirror/lang-json";
  import { markdown } from "@codemirror/lang-markdown";
  import { java } from "@codemirror/lang-java";
  import { cpp } from "@codemirror/lang-cpp";
  import { php } from "@codemirror/lang-php";
  import { go } from "@codemirror/lang-go";
  import { xml } from "@codemirror/lang-xml";
  import { yaml } from "@codemirror/lang-yaml";

  interface Props {
    content: string;
    filename: string;
    readonly?: boolean;
    initialLine?: number | null;
    onchange?: (content: string) => void;
    /** LSP context — when provided, enables hover tooltips, cmd+click go-to-definition, and diagnostics. */
    lsp?: { workspaceId: string; filePath: string; onGotoDef?: (filePath: string, line: number) => void } | null;
  }

  let { content, filename, readonly = false, initialLine = null, onchange, lsp = null }: Props = $props();

  let container: HTMLDivElement | undefined = $state();

  // NOT reactive — CM owns its own lifecycle, we just hold a reference
  let view: EditorView | undefined;
  let langCompartment = new Compartment();
  let readonlyCompartment = new Compartment();
  let editableCompartment = new Compartment();
  let themeCompartment = new Compartment();
  let lspCompartment = new Compartment();

  // Guard: skip content sync when the change came from the editor itself
  let suppressContentSync = false;

  let diagnosticCount = $state(0);

  // ── Theme builders ──────────────────────────────────────

  function buildHighlight(c: EditorColors) {
    return HighlightStyle.define([
      { tag: tags.keyword, color: c.keyword },
      { tag: tags.controlKeyword, color: c.keyword },
      { tag: tags.operatorKeyword, color: c.keyword },
      { tag: tags.definitionKeyword, color: c.keyword },
      { tag: tags.moduleKeyword, color: c.keyword },
      { tag: tags.operator, color: c.operator },
      { tag: tags.punctuation, color: c.operator },
      { tag: tags.string, color: c.string },
      { tag: tags.regexp, color: c.number },
      { tag: tags.number, color: c.number },
      { tag: tags.bool, color: c.number },
      { tag: tags.null, color: c.number },
      { tag: tags.comment, color: c.comment, fontStyle: "italic" },
      { tag: tags.lineComment, color: c.comment, fontStyle: "italic" },
      { tag: tags.blockComment, color: c.comment, fontStyle: "italic" },
      { tag: tags.docComment, color: c.docComment, fontStyle: "italic" },
      { tag: tags.variableName, color: c.variable },
      { tag: tags.definition(tags.variableName), color: c.variable },
      { tag: tags.function(tags.variableName), color: c.variable },
      { tag: tags.typeName, color: c.keyword },
      { tag: tags.className, color: c.keyword },
      { tag: tags.propertyName, color: c.property },
      { tag: tags.definition(tags.propertyName), color: c.property },
      { tag: tags.function(tags.propertyName), color: c.property },
      { tag: tags.attributeName, color: c.keyword },
      { tag: tags.attributeValue, color: c.string },
      { tag: tags.tagName, color: c.keyword },
      { tag: tags.angleBracket, color: c.operator },
      { tag: tags.meta, color: c.operator },
      { tag: tags.heading, color: c.keyword, fontWeight: "bold" },
      { tag: tags.emphasis, fontStyle: "italic" },
      { tag: tags.strong, fontWeight: "bold" },
      { tag: tags.link, color: c.link, textDecoration: "underline" },
      { tag: tags.escape, color: c.number },
      { tag: tags.self, color: c.keyword },
      { tag: tags.atom, color: c.number },
      { tag: tags.labelName, color: c.property },
      { tag: tags.namespace, color: c.operator },
      { tag: tags.macroName, color: c.keyword },
      { tag: tags.special(tags.string), color: c.string },
    ]);
  }

  function buildDarkTheme(c: EditorColors) {
    const ar = c.accentRgba;
    return EditorView.theme({
      "&": {
        color: c.text,
        backgroundColor: "transparent",
        fontSize: "0.78rem",
        fontFamily: "var(--font-mono)",
      },
      ".cm-content": {
        caretColor: c.keyword,
        lineHeight: "1.6",
        padding: "0.5rem 0",
      },
      ".cm-cursor, .cm-dropCursor": {
        borderLeftColor: c.keyword,
      },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
        backgroundColor: `rgba(${ar}, 0.15)`,
      },
      ".cm-activeLine": {
        backgroundColor: "rgba(255, 255, 255, 0.03)",
      },
      ".cm-gutters": {
        backgroundColor: c.bg,
        borderRight: "1px solid var(--border)",
        color: c.textMuted,
      },
      ".cm-activeLineGutter": {
        backgroundColor: c.bg,
        color: c.textSecondary,
      },
      ".cm-lineNumbers .cm-gutterElement": {
        padding: "0 0.5rem 0 0.4rem",
        minWidth: "2.5rem",
        fontSize: "0.7rem",
      },
      ".cm-foldGutter .cm-gutterElement": {
        padding: "0 0.2rem",
        color: c.textMuted,
        cursor: "pointer",
      },
      ".cm-foldGutter .cm-gutterElement:hover": {
        color: c.textSecondary,
      },
      ".cm-foldPlaceholder": {
        backgroundColor: `rgba(${ar}, 0.1)`,
        border: `1px solid rgba(${ar}, 0.2)`,
        color: c.textSecondary,
        borderRadius: "3px",
        padding: "0 0.3rem",
        margin: "0 0.2rem",
      },
      ".cm-matchingBracket": {
        backgroundColor: `rgba(${ar}, 0.2)`,
        outline: `1px solid rgba(${ar}, 0.3)`,
      },
      ".cm-selectionMatch": {
        backgroundColor: `rgba(${ar}, 0.1)`,
      },
      ".cm-searchMatch": {
        backgroundColor: `rgba(${ar}, 0.25)`,
        outline: `1px solid rgba(${ar}, 0.4)`,
      },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: `rgba(${ar}, 0.4)`,
      },
      ".cm-panels": {
        backgroundColor: c.bgCard,
        color: c.text,
        borderBottom: "1px solid var(--border)",
      },
      ".cm-panels.cm-panels-top": {
        borderBottom: "1px solid var(--border)",
      },
      ".cm-panel.cm-search": {
        padding: "0.3rem 0.5rem",
      },
      ".cm-panel.cm-search input, .cm-panel.cm-search button": {
        fontFamily: "var(--font-mono)",
        fontSize: "0.73rem",
      },
      ".cm-panel.cm-search input": {
        backgroundColor: "rgba(0, 0, 0, 0.25)",
        border: "1px solid var(--border-light)",
        color: c.text,
        borderRadius: "3px",
        padding: "0.15rem 0.35rem",
      },
      ".cm-panel.cm-search button": {
        backgroundColor: "transparent",
        border: "1px solid var(--border-light)",
        color: c.textSecondary,
        borderRadius: "3px",
        padding: "0.15rem 0.4rem",
        cursor: "pointer",
      },
      ".cm-panel.cm-search button:hover": {
        backgroundColor: "var(--bg-hover)",
        color: c.text,
      },
      ".cm-panel.cm-search label": {
        fontSize: "0.7rem",
        color: c.textSecondary,
      },
      ".cm-tooltip": {
        backgroundColor: c.bgCard,
        border: "1px solid var(--border-light)",
        color: c.text,
      },
      "&.cm-focused": {
        outline: "none",
      },
      ".cm-scroller": {
        overflow: "auto",
      },
    });
  }

  function buildLightTheme(c: EditorColors) {
    const ar = c.accentRgba;
    return EditorView.theme({
      "&": {
        color: c.text,
        backgroundColor: "transparent",
      },
      ".cm-content": {
        caretColor: c.keyword,
      },
      ".cm-cursor, .cm-dropCursor": {
        borderLeftColor: c.keyword,
      },
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
        backgroundColor: `rgba(${ar}, 0.15)`,
      },
      ".cm-activeLine": {
        backgroundColor: "rgba(0, 0, 0, 0.03)",
      },
      ".cm-gutters": {
        backgroundColor: c.bg,
        color: c.textMuted,
      },
      ".cm-activeLineGutter": {
        backgroundColor: c.bg,
        color: c.textSecondary,
      },
      ".cm-matchingBracket": {
        backgroundColor: `rgba(${ar}, 0.2)`,
        outline: `1px solid rgba(${ar}, 0.3)`,
      },
      ".cm-panels": {
        backgroundColor: c.bgCard,
        color: c.text,
      },
      ".cm-panel.cm-search input": {
        backgroundColor: "rgba(255, 255, 255, 0.5)",
        border: `1px solid ${c.borderColor}`,
        color: c.text,
      },
      ".cm-panel.cm-search button": {
        border: `1px solid ${c.borderColor}`,
        color: c.textSecondary,
      },
      ".cm-searchMatch": {
        backgroundColor: `rgba(${ar}, 0.2)`,
        outline: `1px solid rgba(${ar}, 0.3)`,
      },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: `rgba(${ar}, 0.35)`,
      },
      ".cm-tooltip": {
        backgroundColor: c.bgCard,
        border: `1px solid ${c.borderColor}`,
        color: c.text,
      },
    }, { dark: false });
  }

  /** Build the full theme extension (base theme + syntax highlighting) for the current colors + color scheme. */
  function buildCurrentTheme(): Extension[] {
    const dark = isDarkMode();
    const dc = getEditorColors();
    const lc = getEditorColorsLight();

    if (dark) {
      return [
        buildDarkTheme(dc),
        syntaxHighlighting(buildHighlight(dc)),
      ];
    }
    return [
      EditorView.theme({}, { dark: false }),
      syntaxHighlighting(buildHighlight(lc)),
      buildLightTheme(lc),
    ];
  }

  // ── LSP extensions ────────────────────────────────────

  // Diagnostic underline decorations
  const setDiagnostics = StateEffect.define<LspDiagnostic[]>();
  const diagnosticField = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      for (const e of tr.effects) {
        if (e.is(setDiagnostics)) {
          const builder = new RangeSetBuilder<Decoration>();
          const sorted = [...e.value].sort((a, b) => a.line - b.line || a.character - b.character);
          for (const d of sorted) {
            try {
              const line = tr.state.doc.line(Math.min(d.line, tr.state.doc.lines));
              const from = line.from + Math.min(d.character - 1, line.length);
              const endLine = tr.state.doc.line(Math.min(d.end_line, tr.state.doc.lines));
              let to = endLine.from + Math.min(d.end_character - 1, endLine.length);
              if (to <= from) to = Math.min(from + 1, line.to); // at least 1 char
              const cls = d.severity === "error" ? "cm-lsp-error" : d.severity === "warning" ? "cm-lsp-warning" : "cm-lsp-info";
              builder.add(from, to, Decoration.mark({ class: cls, attributes: { title: d.message } }));
            } catch { /* skip invalid ranges */ }
          }
          return builder.finish();
        }
      }
      return value;
    },
    provide: f => EditorView.decorations.from(f),
  });

  function buildLspExtensions(ctx: NonNullable<typeof lsp>): Extension[] {
    const { workspaceId, filePath, onGotoDef } = ctx;

    // Hover tooltip
    const hover = hoverTooltip(async (view, pos) => {
      const line = view.state.doc.lineAt(pos);
      const lineNum = line.number;
      const col = pos - line.from + 1;
      try {
        const result = await lspHover(workspaceId, filePath, lineNum, col);
        if (!result) return null;
        return {
          pos,
          above: true,
          create: () => {
            const dom = document.createElement("div");
            dom.className = "cm-lsp-hover";
            if (result.kind === "markdown") {
              dom.innerHTML = renderMarkdown(result.text);
            } else {
              const pre = document.createElement("pre");
              pre.textContent = result.text;
              dom.appendChild(pre);
            }
            return { dom };
          },
        };
      } catch { return null; }
    }, { hoverTime: 400 });

    // Cmd+click go-to-definition + cmd-hold underline
    let cmdLinkMark: DecorationSet = Decoration.none;
    const cmdLinkEffect = StateEffect.define<{ from: number; to: number } | null>();
    const cmdLinkField = StateField.define<DecorationSet>({
      create: () => Decoration.none,
      update(value, tr) {
        for (const e of tr.effects) {
          if (e.is(cmdLinkEffect)) {
            if (e.value) {
              return Decoration.set([
                Decoration.mark({ class: "cm-lsp-link" }).range(e.value.from, e.value.to),
              ]);
            }
            return Decoration.none;
          }
        }
        return value;
      },
      provide: f => EditorView.decorations.from(f),
    });

    function wordRangeAt(view: EditorView, pos: number): { from: number; to: number } | null {
      const line = view.state.doc.lineAt(pos);
      const text = line.text;
      const col = pos - line.from;
      if (col >= text.length) return null;
      const wordChar = /[\w$]/;
      if (!wordChar.test(text[col])) return null;
      let from = col;
      let to = col;
      while (from > 0 && wordChar.test(text[from - 1])) from--;
      while (to < text.length && wordChar.test(text[to])) to++;
      return { from: line.from + from, to: line.from + to };
    }

    const gotoDefHandler = EditorView.domEventHandlers({
      click: (event, view) => {
        if (!(event.metaKey || event.ctrlKey)) return false;
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        if (pos == null) return false;
        // Clear underline
        view.dispatch({ effects: cmdLinkEffect.of(null) });
        const line = view.state.doc.lineAt(pos);
        const lineNum = line.number;
        const col = pos - line.from + 1;
        lspGotoDefinition(workspaceId, filePath, lineNum, col).then((loc) => {
          if (loc && onGotoDef) {
            onGotoDef(loc.file_path, loc.line);
          }
        }).catch(() => {});
        return true;
      },
      mousemove: (event, view) => {
        if (!(event.metaKey || event.ctrlKey)) {
          view.dispatch({ effects: cmdLinkEffect.of(null) });
          return false;
        }
        const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
        if (pos == null) {
          view.dispatch({ effects: cmdLinkEffect.of(null) });
          return false;
        }
        const range = wordRangeAt(view, pos);
        view.dispatch({ effects: cmdLinkEffect.of(range) });
        return false;
      },
      keydown: (event, view) => {
        if (event.key === "Meta" || event.key === "Control") {
          view.dom.classList.add("cm-lsp-cmd-held");
        }
        return false;
      },
      keyup: (event, view) => {
        if (event.key === "Meta" || event.key === "Control") {
          view.dom.classList.remove("cm-lsp-cmd-held");
          view.dispatch({ effects: cmdLinkEffect.of(null) });
        }
        return false;
      },
    });

    return [hover, gotoDefHandler, cmdLinkField, diagnosticField];
  }

  /** Fetch and apply diagnostics from LSP. */
  async function refreshDiagnostics() {
    if (!lsp || !view) return;
    try {
      const diags = await lspDiagnostics(lsp.workspaceId, lsp.filePath);
      diagnosticCount = diags.length;
      view.dispatch({ effects: setDiagnostics.of(diags) });
    } catch { /* silent — LSP may not be available */ }
  }

  // ── Language detection ─────────────────────────────────

  function languageFromFilename(name: string): Extension[] {
    const ext = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";

    switch (ext) {
      case "ts": case "tsx":
        return [javascript({ typescript: true, jsx: ext === "tsx" })];
      case "js": case "jsx": case "mjs": case "cjs":
        return [javascript({ jsx: ext === "jsx" })];
      case "svelte":
        return [html()];
      case "rs":
        return [rust()];
      case "py": case "pyw":
        return [python()];
      case "html": case "htm":
        return [html()];
      case "css": case "scss": case "sass":
        return [css()];
      case "json":
        return [json()];
      case "md": case "mdx":
        return [markdown()];
      case "java": case "kt": case "kts":
        return [java()];
      case "c": case "h": case "cpp": case "cc": case "cxx": case "hpp":
        return [cpp()];
      case "go":
        return [go()];
      case "php":
        return [php()];
      case "xml": case "svg": case "xsl": case "xslt":
        return [xml()];
      case "yml": case "yaml":
        return [yaml()];
      default:
        return [];
    }
  }

  // ── Detect system color scheme ─────────────────────────

  function isDarkMode(): boolean {
    if (typeof window === "undefined") return true;
    return !window.matchMedia("(prefers-color-scheme: light)").matches;
  }

  // ── Mount: create editor once ─────────────────────────

  $effect(() => {
    const el = container;
    if (!el) return;

    // Read props once without tracking — sync effects handle subsequent changes
    const initialDoc = untrack(() => content);
    const initialFilename = untrack(() => filename);
    const initialReadonly = untrack(() => readonly);
    const initialLineNum = untrack(() => initialLine);

    const state = EditorState.create({
      doc: initialDoc,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightActiveLine(),
        foldGutter(),
        drawSelection(),
        rectangularSelection(),
        bracketMatching(),
        highlightSelectionMatches(),
        indentOnInput(),
        history(),
        keymap.of([
          ...defaultKeymap,
          ...historyKeymap,
          ...foldKeymap,
          ...searchKeymap,
          indentWithTab,
        ]),
        // Theme — reconfigured dynamically via compartment
        themeCompartment.of(buildCurrentTheme()),
        // Dynamic compartments
        lspCompartment.of(lsp ? buildLspExtensions(lsp) : []),
        langCompartment.of(languageFromFilename(initialFilename)),
        readonlyCompartment.of(EditorState.readOnly.of(initialReadonly)),
        editableCompartment.of(EditorView.editable.of(!initialReadonly)),
        // Change listener (always installed — guarded by readonly check internally)
        EditorView.updateListener.of((update) => {
          if (update.docChanged && onchange) {
            suppressContentSync = true;
            onchange(update.state.doc.toString());
          }
        }),
      ],
    });

    view = new EditorView({ state, parent: el });

    // Jump to initial line
    if (initialLineNum && initialLineNum > 0 && view) {
      const lineInfo = view.state.doc.line(Math.min(initialLineNum, view.state.doc.lines));
      view.dispatch({
        selection: { anchor: lineInfo.from },
        effects: EditorView.scrollIntoView(lineInfo.from, { y: "center" }),
      });
    }

    // Reconfigure theme in-place on theme/color-scheme change (preserves cursor, undo, scroll)
    function onThemeChange() {
      view?.dispatch({
        effects: themeCompartment.reconfigure(buildCurrentTheme()),
      });
    }
    window.addEventListener("korlap-theme-change", onThemeChange);

    return () => {
      window.removeEventListener("korlap-theme-change", onThemeChange);
      view?.destroy();
      view = undefined;
    };
  });

  // ── Sync content from parent (new file selected) ──────

  $effect(() => {
    // Read `content` to establish dependency
    const newContent = content;

    if (!view) return;

    // If this change originated from the editor itself, skip
    if (suppressContentSync) {
      suppressContentSync = false;
      return;
    }

    // External content change (e.g. different file selected) — replace doc
    const currentDoc = view.state.doc.toString();
    if (newContent !== currentDoc) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: newContent },
      });
    }
  });

  // ── Sync language when filename changes ───────────────

  $effect(() => {
    const fname = filename;
    if (!view) return;
    view.dispatch({
      effects: langCompartment.reconfigure(languageFromFilename(fname)),
    });
  });

  // ── Sync readonly when it changes ─────────────────────

  $effect(() => {
    const ro = readonly;
    if (!view) return;
    view.dispatch({
      effects: [
        readonlyCompartment.reconfigure(EditorState.readOnly.of(ro)),
        editableCompartment.reconfigure(EditorView.editable.of(!ro)),
      ],
    });
  });

  // ── Sync LSP context ────────────────────────────────────

  $effect(() => {
    const ctx = lsp;
    if (!view) return;
    view.dispatch({
      effects: lspCompartment.reconfigure(ctx ? buildLspExtensions(ctx) : []),
    });
    untrack(() => { diagnosticCount = 0; });
  });

  // Fetch diagnostics when LSP server becomes ready
  $effect(() => {
    if (!lsp) return;
    let unlisten: (() => void) | undefined;

    listen<{ server_id: string; status: string; message: string }>("lsp-status", (e) => {
      if (e.payload.status === "ready") {
        refreshDiagnostics();
      }
    }).then((fn) => { unlisten = fn; });

    return () => { unlisten?.(); };
  });

  // ── Public methods ────────────────────────────────────

  export function goToLine(line: number) {
    if (!view) return;
    const lineCount = view.state.doc.lines;
    const targetLine = Math.min(Math.max(1, line), lineCount);
    const lineInfo = view.state.doc.line(targetLine);
    view.dispatch({
      selection: { anchor: lineInfo.from },
      effects: EditorView.scrollIntoView(lineInfo.from, { y: "center" }),
    });
    view.focus();
  }

  export function getContent(): string {
    return view?.state.doc.toString() ?? content;
  }
</script>

<div class="code-editor" bind:this={container}></div>

<style>
  .code-editor {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .code-editor :global(.cm-editor) {
    flex: 1;
    min-height: 0;
  }

  /* LSP hover tooltip — rendered markdown */
  .code-editor :global(.cm-lsp-hover) {
    max-width: 520px;
    max-height: 350px;
    overflow: auto;
    padding: 0.5rem 0.7rem;
    font-size: 0.73rem;
    line-height: 1.55;
    color: var(--text-primary);
  }

  .code-editor :global(.cm-lsp-hover p) {
    margin: 0.3rem 0;
  }

  .code-editor :global(.cm-lsp-hover p:first-child) {
    margin-top: 0;
  }

  .code-editor :global(.cm-lsp-hover p:last-child) {
    margin-bottom: 0;
  }

  .code-editor :global(.cm-lsp-hover pre) {
    margin: 0.35rem 0;
    padding: 0.35rem 0.5rem;
    background: rgba(0, 0, 0, 0.15);
    border-radius: 4px;
    overflow-x: auto;
  }

  .code-editor :global(.cm-lsp-hover code) {
    font-family: var(--font-mono);
    font-size: 0.72rem;
  }

  .code-editor :global(.cm-lsp-hover pre code) {
    background: none;
    padding: 0;
  }

  .code-editor :global(.cm-lsp-hover :not(pre) > code) {
    background: rgba(0, 0, 0, 0.1);
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
  }

  .code-editor :global(.cm-lsp-hover hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0.4rem 0;
  }

  .code-editor :global(.cm-lsp-hover a) {
    color: var(--accent);
  }

  .code-editor :global(.cm-lsp-hover ul),
  .code-editor :global(.cm-lsp-hover ol) {
    margin: 0.3rem 0;
    padding-left: 1.2rem;
  }

  .code-editor :global(.cm-lsp-hover h1),
  .code-editor :global(.cm-lsp-hover h2),
  .code-editor :global(.cm-lsp-hover h3) {
    font-size: 0.78rem;
    margin: 0.4rem 0 0.2rem;
  }

  /* LSP diagnostic underlines */
  .code-editor :global(.cm-lsp-error) {
    text-decoration: wavy underline var(--status-error, #e55);
    text-decoration-skip-ink: none;
    text-underline-offset: 3px;
  }

  .code-editor :global(.cm-lsp-warning) {
    text-decoration: wavy underline var(--status-warning, #da3);
    text-decoration-skip-ink: none;
    text-underline-offset: 3px;
  }

  .code-editor :global(.cm-lsp-info) {
    text-decoration: wavy underline var(--text-muted, #888);
    text-decoration-skip-ink: none;
    text-underline-offset: 3px;
  }

  /* Cmd-hold: pointer cursor */
  .code-editor :global(.cm-lsp-cmd-held) {
    cursor: pointer;
  }

  /* Cmd-hover: underline the word under cursor */
  .code-editor :global(.cm-lsp-link) {
    text-decoration: underline;
    text-decoration-color: var(--accent);
    cursor: pointer;
  }
</style>

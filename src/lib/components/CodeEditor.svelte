<script lang="ts">
  import { untrack } from "svelte";
  import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter, drawSelection, rectangularSelection } from "@codemirror/view";
  import { EditorState, Compartment, type Extension } from "@codemirror/state";
  import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
  import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
  import { bracketMatching, foldGutter, foldKeymap, indentOnInput, syntaxHighlighting, HighlightStyle } from "@codemirror/language";
  import { tags } from "@lezer/highlight";

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

  interface Props {
    content: string;
    filename: string;
    readonly?: boolean;
    initialLine?: number | null;
    onchange?: (content: string) => void;
  }

  let { content, filename, readonly = false, initialLine = null, onchange }: Props = $props();

  let container: HTMLDivElement | undefined = $state();

  // NOT reactive — CM owns its own lifecycle, we just hold a reference
  let view: EditorView | undefined;
  let langCompartment = new Compartment();
  let readonlyCompartment = new Compartment();
  let editableCompartment = new Compartment();

  // Guard: skip content sync when the change came from the editor itself
  let suppressContentSync = false;

  // ── Korlap theme (warm amber palette) ──────────────────

  const korlapHighlight = HighlightStyle.define([
    { tag: tags.keyword, color: "#c8a97e" },
    { tag: tags.controlKeyword, color: "#c8a97e" },
    { tag: tags.operatorKeyword, color: "#c8a97e" },
    { tag: tags.definitionKeyword, color: "#c8a97e" },
    { tag: tags.moduleKeyword, color: "#c8a97e" },
    { tag: tags.operator, color: "#8a7e6a" },
    { tag: tags.punctuation, color: "#8a7e6a" },
    { tag: tags.string, color: "#7e9e6b" },
    { tag: tags.regexp, color: "#c87e7e" },
    { tag: tags.number, color: "#c87e7e" },
    { tag: tags.bool, color: "#c87e7e" },
    { tag: tags.null, color: "#c87e7e" },
    { tag: tags.comment, color: "#6a6050", fontStyle: "italic" },
    { tag: tags.lineComment, color: "#6a6050", fontStyle: "italic" },
    { tag: tags.blockComment, color: "#6a6050", fontStyle: "italic" },
    { tag: tags.docComment, color: "#7a7060", fontStyle: "italic" },
    { tag: tags.variableName, color: "#d4c5a9" },
    { tag: tags.definition(tags.variableName), color: "#d4c5a9" },
    { tag: tags.function(tags.variableName), color: "#d4c5a9" },
    { tag: tags.typeName, color: "#c8a97e" },
    { tag: tags.className, color: "#c8a97e" },
    { tag: tags.propertyName, color: "#b8a890" },
    { tag: tags.definition(tags.propertyName), color: "#b8a890" },
    { tag: tags.function(tags.propertyName), color: "#b8a890" },
    { tag: tags.attributeName, color: "#c8a97e" },
    { tag: tags.attributeValue, color: "#7e9e6b" },
    { tag: tags.tagName, color: "#c8a97e" },
    { tag: tags.angleBracket, color: "#8a7e6a" },
    { tag: tags.meta, color: "#8a7e6a" },
    { tag: tags.heading, color: "#c8a97e", fontWeight: "bold" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.strong, fontWeight: "bold" },
    { tag: tags.link, color: "#7e8ec8", textDecoration: "underline" },
    { tag: tags.escape, color: "#c87e7e" },
    { tag: tags.self, color: "#c8a97e" },
    { tag: tags.atom, color: "#c87e7e" },
    { tag: tags.labelName, color: "#b8a890" },
    { tag: tags.namespace, color: "#8a7e6a" },
    { tag: tags.macroName, color: "#c8a97e" },
    { tag: tags.special(tags.string), color: "#7e9e6b" },
  ]);

  const korlapTheme = EditorView.theme({
    "&": {
      color: "#d4c5a9",
      backgroundColor: "transparent",
      fontSize: "0.78rem",
      fontFamily: "var(--font-mono)",
    },
    ".cm-content": {
      caretColor: "#c8a97e",
      lineHeight: "1.6",
      padding: "0.5rem 0",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#c8a97e",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "rgba(200, 169, 126, 0.15)",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(255, 255, 255, 0.03)",
    },
    ".cm-gutters": {
      backgroundColor: "#12110e",
      borderRight: "1px solid var(--border)",
      color: "#4a4540",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "#12110e",
      color: "#8a7e6a",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 0.5rem 0 0.4rem",
      minWidth: "2.5rem",
      fontSize: "0.7rem",
    },
    ".cm-foldGutter .cm-gutterElement": {
      padding: "0 0.2rem",
      color: "#4a4540",
      cursor: "pointer",
    },
    ".cm-foldGutter .cm-gutterElement:hover": {
      color: "#8a7e6a",
    },
    ".cm-foldPlaceholder": {
      backgroundColor: "rgba(200, 169, 126, 0.1)",
      border: "1px solid rgba(200, 169, 126, 0.2)",
      color: "#8a7e6a",
      borderRadius: "3px",
      padding: "0 0.3rem",
      margin: "0 0.2rem",
    },
    ".cm-matchingBracket": {
      backgroundColor: "rgba(200, 169, 126, 0.2)",
      outline: "1px solid rgba(200, 169, 126, 0.3)",
    },
    ".cm-selectionMatch": {
      backgroundColor: "rgba(200, 169, 126, 0.1)",
    },
    ".cm-searchMatch": {
      backgroundColor: "rgba(200, 169, 126, 0.25)",
      outline: "1px solid rgba(200, 169, 126, 0.4)",
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: "rgba(200, 169, 126, 0.4)",
    },
    ".cm-panels": {
      backgroundColor: "#1a1814",
      color: "#d4c5a9",
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
      color: "#d4c5a9",
      borderRadius: "3px",
      padding: "0.15rem 0.35rem",
    },
    ".cm-panel.cm-search button": {
      backgroundColor: "transparent",
      border: "1px solid var(--border-light)",
      color: "#8a7e6a",
      borderRadius: "3px",
      padding: "0.15rem 0.4rem",
      cursor: "pointer",
    },
    ".cm-panel.cm-search button:hover": {
      backgroundColor: "var(--bg-hover)",
      color: "#d4c5a9",
    },
    ".cm-panel.cm-search label": {
      fontSize: "0.7rem",
      color: "#8a7e6a",
    },
    ".cm-tooltip": {
      backgroundColor: "#1a1814",
      border: "1px solid var(--border-light)",
      color: "#d4c5a9",
    },
    "&.cm-focused": {
      outline: "none",
    },
    ".cm-scroller": {
      overflow: "auto",
    },
  });

  // ── Light theme overrides ──────────────────────────────

  const korlapLightHighlight = HighlightStyle.define([
    { tag: tags.keyword, color: "#9a7a48" },
    { tag: tags.controlKeyword, color: "#9a7a48" },
    { tag: tags.operatorKeyword, color: "#9a7a48" },
    { tag: tags.definitionKeyword, color: "#9a7a48" },
    { tag: tags.moduleKeyword, color: "#9a7a48" },
    { tag: tags.operator, color: "#6a5d4e" },
    { tag: tags.punctuation, color: "#6a5d4e" },
    { tag: tags.string, color: "#4a7a3a" },
    { tag: tags.regexp, color: "#b04040" },
    { tag: tags.number, color: "#b04040" },
    { tag: tags.bool, color: "#b04040" },
    { tag: tags.null, color: "#b04040" },
    { tag: tags.comment, color: "#907f6d", fontStyle: "italic" },
    { tag: tags.lineComment, color: "#907f6d", fontStyle: "italic" },
    { tag: tags.blockComment, color: "#907f6d", fontStyle: "italic" },
    { tag: tags.variableName, color: "#33302a" },
    { tag: tags.typeName, color: "#9a7a48" },
    { tag: tags.className, color: "#9a7a48" },
    { tag: tags.propertyName, color: "#5a5040" },
    { tag: tags.tagName, color: "#9a7a48" },
    { tag: tags.attributeName, color: "#9a7a48" },
    { tag: tags.attributeValue, color: "#4a7a3a" },
    { tag: tags.heading, color: "#9a7a48", fontWeight: "bold" },
    { tag: tags.link, color: "#5a6a9a", textDecoration: "underline" },
    { tag: tags.escape, color: "#b04040" },
  ]);

  const korlapLightTheme = EditorView.theme({
    "&": {
      color: "#33302a",
      backgroundColor: "transparent",
    },
    ".cm-content": {
      caretColor: "#9a7a48",
    },
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: "#9a7a48",
    },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
      backgroundColor: "rgba(154, 122, 72, 0.15)",
    },
    ".cm-activeLine": {
      backgroundColor: "rgba(0, 0, 0, 0.03)",
    },
    ".cm-gutters": {
      backgroundColor: "#f7f4ef",
      color: "#b0a898",
    },
    ".cm-activeLineGutter": {
      backgroundColor: "#f7f4ef",
      color: "#6a5d4e",
    },
    ".cm-matchingBracket": {
      backgroundColor: "rgba(154, 122, 72, 0.2)",
      outline: "1px solid rgba(154, 122, 72, 0.3)",
    },
    ".cm-panels": {
      backgroundColor: "#eee9e0",
      color: "#33302a",
    },
    ".cm-panel.cm-search input": {
      backgroundColor: "rgba(255, 255, 255, 0.5)",
      border: "1px solid #c8bfb0",
      color: "#33302a",
    },
    ".cm-panel.cm-search button": {
      border: "1px solid #c8bfb0",
      color: "#6a5d4e",
    },
    ".cm-searchMatch": {
      backgroundColor: "rgba(154, 122, 72, 0.2)",
      outline: "1px solid rgba(154, 122, 72, 0.3)",
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: "rgba(154, 122, 72, 0.35)",
    },
    ".cm-tooltip": {
      backgroundColor: "#eee9e0",
      border: "1px solid #c8bfb0",
      color: "#33302a",
    },
  }, { dark: false });

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

    const dark = isDarkMode();

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
        // Theme (static — dark/light chosen at mount time)
        dark ? korlapTheme : EditorView.theme({}, { dark: false }),
        dark ? syntaxHighlighting(korlapHighlight) : syntaxHighlighting(korlapLightHighlight),
        ...(!dark ? [korlapLightTheme] : []),
        // Dynamic compartments
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

    return () => {
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
</style>

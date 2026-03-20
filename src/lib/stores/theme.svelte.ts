import { themes, defaultThemeId, type ThemeId, type ThemeDefinition, type TerminalColors, type EditorColors } from "$lib/themes";

const STORAGE_KEY = "korlap-theme";

// ── Reactive state ────────────────────────────────────────

let activeId = $state<ThemeId>(readStoredTheme());

function readStoredTheme(): ThemeId {
  if (typeof localStorage === "undefined") return defaultThemeId;
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored && stored in themes ? (stored as ThemeId) : defaultThemeId;
}

// ── Public API ────────────────────────────────────────────

export function getThemeId(): ThemeId {
  return activeId;
}

export function getTheme(): ThemeDefinition {
  return themes[activeId];
}

export function setTheme(id: ThemeId): void {
  activeId = id;
  localStorage.setItem(STORAGE_KEY, id);
  applyCssVars();
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("korlap-theme-change"));
  }
}

/** Terminal colors for the current theme + color scheme */
export function getTerminalTheme(): TerminalColors {
  const t = themes[activeId];
  return isDark() ? t.terminal.dark : t.terminal.light;
}

/** Editor colors for the current theme + color scheme */
export function getEditorColors(): EditorColors {
  const t = themes[activeId];
  return isDark() ? t.editor.dark : t.editor.light;
}

/** Light-mode editor colors (for CodeMirror light theme, always needed) */
export function getEditorColorsLight(): EditorColors {
  return themes[activeId].editor.light;
}

// ── CSS application ───────────────────────────────────────

function isDark(): boolean {
  return typeof window === "undefined" || window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function applyCssVars(): void {
  if (typeof document === "undefined") return;
  const t = themes[activeId];
  const vars = isDark() ? t.css.dark : t.css.light;
  const root = document.documentElement;
  for (const [key, value] of Object.entries(vars)) {
    root.style.setProperty(`--${key}`, value);
  }
}

// ── Initialization ────────────────────────────────────────

export function initTheme(): void {
  applyCssVars();
  // Re-apply when system color scheme changes
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    applyCssVars();
    // Dispatch event so Terminal/CodeEditor can update
    window.dispatchEvent(new CustomEvent("korlap-theme-change"));
  });
}

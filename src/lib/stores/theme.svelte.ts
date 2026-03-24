import { themes, defaultThemeId, type ThemeId, type ThemeDefinition, type TerminalColors, type EditorColors } from "$lib/themes";

const STORAGE_KEY = "korlap-theme";
const MODE_STORAGE_KEY = "korlap-color-mode";

export type ColorMode = "dark" | "light" | "system";

// ── Reactive state ────────────────────────────────────────

let activeId = $state<ThemeId>(readStoredTheme());
let colorMode = $state<ColorMode>(readStoredColorMode());

function readStoredTheme(): ThemeId {
  if (typeof localStorage === "undefined") return defaultThemeId;
  const stored = localStorage.getItem(STORAGE_KEY);
  return stored && stored in themes ? (stored as ThemeId) : defaultThemeId;
}

function readStoredColorMode(): ColorMode {
  if (typeof localStorage === "undefined") return "system";
  const stored = localStorage.getItem(MODE_STORAGE_KEY);
  if (stored === "dark" || stored === "light" || stored === "system") return stored;
  return "system";
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

export function getColorMode(): ColorMode {
  return colorMode;
}

export function setColorMode(mode: ColorMode): void {
  colorMode = mode;
  localStorage.setItem(MODE_STORAGE_KEY, mode);
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
  if (colorMode === "dark") return true;
  if (colorMode === "light") return false;
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

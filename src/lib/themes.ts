// ── Theme definitions ─────────────────────────────────────
// Each theme defines CSS custom property values (dark + light),
// plus terminal and editor color overrides.

export type ThemeId = "amber" | "indigo";

export interface TerminalColors {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export interface EditorColors {
  keyword: string;
  operator: string;
  string: string;
  number: string;
  comment: string;
  docComment: string;
  variable: string;
  property: string;
  link: string;
  text: string;
  textSecondary: string;
  textMuted: string;
  bg: string;
  bgCard: string;
  accentRgba: string; // "r, g, b" for rgba() usage
  borderColor: string; // for light theme search inputs etc.
}

export interface ThemeDefinition {
  id: ThemeId;
  name: string;
  css: { dark: Record<string, string>; light: Record<string, string> };
  terminal: { dark: TerminalColors; light: TerminalColors };
  editor: { dark: EditorColors; light: EditorColors };
}

/** Extract unique solid hex colors from a theme's dark CSS tokens (order-preserving). */
export function getPreviewColors(theme: ThemeDefinition): string[] {
  const seen = new Set<string>();
  const colors: string[] = [];
  for (const v of Object.values(theme.css.dark)) {
    const hex = v.toLowerCase();
    if (/^#[0-9a-f]{6}$/.test(hex) && !seen.has(hex)) {
      seen.add(hex);
      colors.push(hex);
    }
  }
  return colors;
}

// ── Amber (original warm palette) ─────────────────────────

const amber: ThemeDefinition = {
  id: "amber",
  name: "Amber",
  css: {
    dark: {
      "bg-sidebar": "#0f0d0a",
      "bg-base": "#12110e",
      "bg-titlebar": "#1a1611",
      "bg-card": "#1a1814",
      "bg-hover": "#1e1b17",
      "bg-active": "#2a2520",
      "border": "#1e1b18",
      "border-light": "#3a3530",
      "text-muted": "#4a4540",
      "text-dim": "#6a6050",
      "text-secondary": "#8a7e6a",
      "text-primary": "#d4c5a9",
      "text-bright": "#e8dcc8",
      "accent": "#c8a97e",
      "status-ok": "#7e9e6b",
      "status-pr-open": "#7e8ec8",
      "status-fail": "#b5564e",
      "diff-add": "#7e9e6b",
      "diff-add-bg": "#1a2a1a",
      "diff-del": "#c87e7e",
      "diff-del-bg": "#2a1a1a",
      "error": "#ee8888",
      "error-bg": "#3a1a1a",
      "toast-bg": "rgba(30, 27, 24, 0.78)",
      "toast-border": "rgba(255, 255, 255, 0.06)",
      "overlay-bg": "rgba(0, 0, 0, 0.5)",
      "input-inset-bg": "rgba(0, 0, 0, 0.25)",
      "input-inset-focus": "rgba(0, 0, 0, 0.3)",
      "btn-subtle-bg": "rgba(255, 255, 255, 0.06)",
      "btn-subtle-hover": "rgba(255, 255, 255, 0.1)",
      "pill-btn-hover": "rgba(255, 255, 255, 0.08)",
      "code-inline-bg": "rgba(255, 255, 255, 0.05)",
      "code-block-bg": "rgba(0, 0, 0, 0.3)",
      "img-remove-bg": "rgba(0, 0, 0, 0.65)",
      "img-remove-hover": "rgba(0, 0, 0, 0.85)",
      "bg-dev": "#191726",
      "border-dev": "#252238",
    },
    light: {
      "bg-sidebar": "#f0ebe3",
      "bg-base": "#f7f4ef",
      "bg-titlebar": "#ebe5dc",
      "bg-card": "#eee9e0",
      "bg-hover": "#e5dfd4",
      "bg-active": "#dbd4c7",
      "border": "#dbd4c7",
      "border-light": "#c8bfb0",
      "text-muted": "#b5ab9a",
      "text-dim": "#907f6d",
      "text-secondary": "#6a5d4e",
      "text-primary": "#33302a",
      "text-bright": "#1a1714",
      "accent": "#9a7a48",
      "status-ok": "#4e7a3a",
      "status-pr-open": "#4e6a9e",
      "status-fail": "#b54545",
      "diff-add": "#4e7a3a",
      "diff-add-bg": "#e4f0dc",
      "diff-del": "#b54545",
      "diff-del-bg": "#f5e2e2",
      "error": "#c04040",
      "error-bg": "#f5e2e2",
      "toast-bg": "rgba(255, 252, 248, 0.82)",
      "toast-border": "rgba(0, 0, 0, 0.06)",
      "overlay-bg": "rgba(0, 0, 0, 0.25)",
      "input-inset-bg": "rgba(0, 0, 0, 0.04)",
      "input-inset-focus": "rgba(0, 0, 0, 0.07)",
      "btn-subtle-bg": "rgba(0, 0, 0, 0.04)",
      "btn-subtle-hover": "rgba(0, 0, 0, 0.08)",
      "pill-btn-hover": "rgba(0, 0, 0, 0.06)",
      "code-inline-bg": "rgba(0, 0, 0, 0.04)",
      "code-block-bg": "rgba(0, 0, 0, 0.06)",
      "img-remove-bg": "rgba(0, 0, 0, 0.55)",
      "img-remove-hover": "rgba(0, 0, 0, 0.75)",
      "bg-dev": "#e8e4f0",
      "border-dev": "#cbc4d8",
    },
  },
  terminal: {
    dark: {
      background: "#12110e",
      foreground: "#d4c5a9",
      cursor: "#c8a97e",
      cursorAccent: "#12110e",
      selectionBackground: "#c8a97e44",
      black: "#1e1b17",
      red: "#c87e7e",
      green: "#7e9e6b",
      yellow: "#c8a97e",
      blue: "#7e8e9e",
      magenta: "#9e7e8e",
      cyan: "#7e9e9e",
      white: "#d4c5a9",
      brightBlack: "#6a6050",
      brightRed: "#e8a0a0",
      brightGreen: "#a0c890",
      brightYellow: "#e8c8a0",
      brightBlue: "#a0b0c8",
      brightMagenta: "#c8a0b8",
      brightCyan: "#a0c8c0",
      brightWhite: "#e8dcc8",
    },
    light: {
      background: "#f7f4ef",
      foreground: "#33302a",
      cursor: "#9a7a48",
      cursorAccent: "#f7f4ef",
      selectionBackground: "#9a7a4844",
      black: "#dbd4c7",
      red: "#b54545",
      green: "#4e7a3a",
      yellow: "#9a7a48",
      blue: "#4e6a8e",
      magenta: "#7e4e6e",
      cyan: "#3a7a6e",
      white: "#33302a",
      brightBlack: "#907f6d",
      brightRed: "#c85050",
      brightGreen: "#5a8e45",
      brightYellow: "#b08a50",
      brightBlue: "#5a7aa0",
      brightMagenta: "#905a80",
      brightCyan: "#4a8e80",
      brightWhite: "#1a1714",
    },
  },
  editor: {
    dark: {
      keyword: "#c8a97e",
      operator: "#8a7e6a",
      string: "#7e9e6b",
      number: "#c87e7e",
      comment: "#6a6050",
      docComment: "#7a7060",
      variable: "#d4c5a9",
      property: "#b8a890",
      link: "#7e8ec8",
      text: "#d4c5a9",
      textSecondary: "#8a7e6a",
      textMuted: "#4a4540",
      bg: "#12110e",
      bgCard: "#1a1814",
      accentRgba: "200, 169, 126",
      borderColor: "#3a3530",
    },
    light: {
      keyword: "#9a7a48",
      operator: "#6a5d4e",
      string: "#4a7a3a",
      number: "#b04040",
      comment: "#907f6d",
      docComment: "#907f6d",
      variable: "#33302a",
      property: "#5a5040",
      link: "#5a6a9a",
      text: "#33302a",
      textSecondary: "#6a5d4e",
      textMuted: "#b0a898",
      bg: "#f7f4ef",
      bgCard: "#eee9e0",
      accentRgba: "154, 122, 72",
      borderColor: "#c8bfb0",
    },
  },
};

// ── Indigo (purple-lavender-ice palette) ──────────────────

const indigo: ThemeDefinition = {
  id: "indigo",
  name: "Indigo",
  css: {
    dark: {
      "bg-sidebar": "#111014",
      "bg-base": "#141318",
      "bg-titlebar": "#1b1a1f",
      "bg-card": "#19181d",
      "bg-hover": "#1f1e24",
      "bg-active": "#27262c",
      "border": "#222128",
      "border-light": "#32313a",
      "text-muted": "#48475a",
      "text-dim": "#62607a",
      "text-secondary": "#9088a8",
      "text-primary": "#d8d2e4",
      "text-bright": "#eee8f4",
      "accent": "#A3C7D6",
      "status-ok": "#7e9e6b",
      "status-pr-open": "#7e8ec8",
      "status-fail": "#c05858",
      "diff-add": "#7e9e6b",
      "diff-add-bg": "#141e16",
      "diff-del": "#c87878",
      "diff-del-bg": "#201418",
      "error": "#d06060",
      "error-bg": "#201418",
      "toast-bg": "rgba(20, 19, 24, 0.85)",
      "toast-border": "rgba(255, 255, 255, 0.06)",
      "overlay-bg": "rgba(0, 0, 0, 0.5)",
      "input-inset-bg": "rgba(0, 0, 0, 0.25)",
      "input-inset-focus": "rgba(0, 0, 0, 0.3)",
      "btn-subtle-bg": "rgba(255, 255, 255, 0.06)",
      "btn-subtle-hover": "rgba(255, 255, 255, 0.1)",
      "pill-btn-hover": "rgba(255, 255, 255, 0.08)",
      "code-inline-bg": "rgba(255, 255, 255, 0.05)",
      "code-block-bg": "rgba(0, 0, 0, 0.3)",
      "img-remove-bg": "rgba(0, 0, 0, 0.65)",
      "img-remove-hover": "rgba(0, 0, 0, 0.85)",
      "bg-dev": "#171620",
      "border-dev": "#2a2932",
    },
    light: {
      "bg-sidebar": "#e8e4f0",
      "bg-base": "#f2eff8",
      "bg-titlebar": "#ddd8ea",
      "bg-card": "#e2ddef",
      "bg-hover": "#d8d2e4",
      "bg-active": "#ccc4da",
      "border": "#ccc4da",
      "border-light": "#b8b0c8",
      "text-muted": "#b0a8c0",
      "text-dim": "#887ea0",
      "text-secondary": "#5e5478",
      "text-primary": "#28243a",
      "text-bright": "#181430",
      "accent": "#4a7a8a",
      "status-ok": "#4e7a3a",
      "status-pr-open": "#4e6a9e",
      "status-fail": "#b54545",
      "diff-add": "#4e7a3a",
      "diff-add-bg": "#e4f0dc",
      "diff-del": "#b54545",
      "diff-del-bg": "#f2e0e0",
      "error": "#b04040",
      "error-bg": "#f2e0e0",
      "toast-bg": "rgba(242, 239, 248, 0.82)",
      "toast-border": "rgba(0, 0, 0, 0.06)",
      "overlay-bg": "rgba(0, 0, 0, 0.25)",
      "input-inset-bg": "rgba(0, 0, 0, 0.04)",
      "input-inset-focus": "rgba(0, 0, 0, 0.07)",
      "btn-subtle-bg": "rgba(0, 0, 0, 0.04)",
      "btn-subtle-hover": "rgba(0, 0, 0, 0.08)",
      "pill-btn-hover": "rgba(0, 0, 0, 0.06)",
      "code-inline-bg": "rgba(0, 0, 0, 0.04)",
      "code-block-bg": "rgba(0, 0, 0, 0.06)",
      "img-remove-bg": "rgba(0, 0, 0, 0.55)",
      "img-remove-hover": "rgba(0, 0, 0, 0.75)",
      "bg-dev": "#e0dcf0",
      "border-dev": "#c8c0e0",
    },
  },
  terminal: {
    dark: {
      background: "#141318",
      foreground: "#d8d2e4",
      cursor: "#A3C7D6",
      cursorAccent: "#141318",
      selectionBackground: "#A3C7D644",
      black: "#1f1e24",
      red: "#c87878",
      green: "#7e9e6b",
      yellow: "#d4b878",
      blue: "#A3C7D6",
      magenta: "#9F73AB",
      cyan: "#78b8b8",
      white: "#d8d2e4",
      brightBlack: "#625e78",
      brightRed: "#e09090",
      brightGreen: "#a0c890",
      brightYellow: "#e8d098",
      brightBlue: "#b8d8e8",
      brightMagenta: "#b898c0",
      brightCyan: "#98d0d0",
      brightWhite: "#eee8f4",
    },
    light: {
      background: "#f2eff8",
      foreground: "#28243a",
      cursor: "#4a7a8a",
      cursorAccent: "#f2eff8",
      selectionBackground: "#4a7a8a44",
      black: "#ccc4da",
      red: "#b54545",
      green: "#4e7a3a",
      yellow: "#8a7040",
      blue: "#4a6a8a",
      magenta: "#7a5488",
      cyan: "#3a7878",
      white: "#28243a",
      brightBlack: "#887ea0",
      brightRed: "#c85050",
      brightGreen: "#5a8e45",
      brightYellow: "#a08848",
      brightBlue: "#5a7a9a",
      brightMagenta: "#8a6498",
      brightCyan: "#4a8a88",
      brightWhite: "#181430",
    },
  },
  editor: {
    dark: {
      keyword: "#A3C7D6",
      operator: "#9088a8",
      string: "#7e9e6b",
      number: "#9F73AB",
      comment: "#625e78",
      docComment: "#706890",
      variable: "#d8d2e4",
      property: "#b0a8c8",
      link: "#A3C7D6",
      text: "#d8d2e4",
      textSecondary: "#9088a8",
      textMuted: "#48475a",
      bg: "#141318",
      bgCard: "#19181d",
      accentRgba: "163, 199, 214",
      borderColor: "#32313a",
    },
    light: {
      keyword: "#4a7a8a",
      operator: "#5e5478",
      string: "#4a7a3a",
      number: "#7a5488",
      comment: "#887ea0",
      docComment: "#887ea0",
      variable: "#28243a",
      property: "#5a5070",
      link: "#4a7a8a",
      text: "#28243a",
      textSecondary: "#5e5478",
      textMuted: "#b0a8c0",
      bg: "#f2eff8",
      bgCard: "#e2ddef",
      accentRgba: "74, 122, 138",
      borderColor: "#b8b0c8",
    },
  },
};

// ── Registry ──────────────────────────────────────────────

export const themes: Record<ThemeId, ThemeDefinition> = { amber, indigo };
export const themeList: ThemeDefinition[] = [amber, indigo];
export const defaultThemeId: ThemeId = "amber";

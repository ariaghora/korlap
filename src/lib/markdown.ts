import { Marked } from "marked";
import { markedHighlight } from "marked-highlight";
import hljs from "highlight.js/lib/core";
import DOMPurify from "dompurify";
import "./hljs-korlap.css";
import type { MessageMention } from "$lib/stores/messages.svelte.js";

// Register common languages individually to keep bundle small
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import bash from "highlight.js/lib/languages/bash";
import json from "highlight.js/lib/languages/json";
import css from "highlight.js/lib/languages/css";
import xml from "highlight.js/lib/languages/xml";
import yaml from "highlight.js/lib/languages/yaml";
import markdown from "highlight.js/lib/languages/markdown";
import sql from "highlight.js/lib/languages/sql";
import go from "highlight.js/lib/languages/go";
import java from "highlight.js/lib/languages/java";
import swift from "highlight.js/lib/languages/swift";
import diff from "highlight.js/lib/languages/diff";
import toml from "highlight.js/lib/languages/ini";
import kotlin from "highlight.js/lib/languages/kotlin";
import ruby from "highlight.js/lib/languages/ruby";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";

hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("jsx", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("tsx", typescript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("py", python);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("sh", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("zsh", bash);
hljs.registerLanguage("json", json);
hljs.registerLanguage("css", css);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("svg", xml);
hljs.registerLanguage("svelte", xml);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("yml", yaml);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("md", markdown);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("go", go);
hljs.registerLanguage("java", java);
hljs.registerLanguage("swift", swift);
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("toml", toml);
hljs.registerLanguage("ini", toml);
hljs.registerLanguage("kotlin", kotlin);
hljs.registerLanguage("kt", kotlin);
hljs.registerLanguage("ruby", ruby);
hljs.registerLanguage("rb", ruby);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);

const marked = new Marked(
  markedHighlight({
    langPrefix: "hljs language-",
    highlight(code: string, lang: string) {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(code, { language: lang }).value;
      }
      return hljs.highlightAuto(code).value;
    },
  }),
);

marked.setOptions({
  gfm: true,
  breaks: false,
});

// Cache rendered HTML keyed by raw markdown string.
// Messages are immutable once added, so cache entries never go stale.
const renderCache = new Map<string, string>();

/**
 * Render a markdown string to sanitized HTML.
 * Synchronous — safe to call inline in Svelte templates.
 * Results are cached so re-renders skip parsing/highlighting entirely.
 */
export function renderMarkdown(raw: string): string {
  const cached = renderCache.get(raw);
  if (cached !== undefined) return cached;
  const html = marked.parse(raw) as string;
  const sanitized = DOMPurify.sanitize(html);
  renderCache.set(raw, sanitized);
  return sanitized;
}

function escapeHtmlAttr(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

const userRenderCache = new Map<string, string>();

/**
 * Render a user message's markdown to sanitized HTML.
 * Mention references (@filename) are converted to clickable chips
 * (span.msg-mention-chip with data-mention-path) before markdown parsing.
 */
export function renderUserMarkdown(raw: string, mentions?: MessageMention[]): string {
  let processed = raw;

  if (mentions && mentions.length > 0) {
    // Sort by displayName length descending to avoid partial matches
    const sorted = [...mentions].sort((a, b) => b.displayName.length - a.displayName.length);
    for (const m of sorted) {
      const escaped = m.displayName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const regex = new RegExp(`@${escaped}`, "g");
      const chipHtml = `<span class="msg-mention-chip" data-mention-path="${escapeHtmlAttr(m.path)}">@${escapeHtml(m.displayName)}</span>`;
      processed = processed.replace(regex, chipHtml);
    }
  }

  const cached = userRenderCache.get(processed);
  if (cached !== undefined) return cached;

  const html = marked.parse(processed) as string;
  const sanitized = DOMPurify.sanitize(html, {
    ADD_ATTR: ["data-mention-path"],
  });

  userRenderCache.set(processed, sanitized);
  return sanitized;
}

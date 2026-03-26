#!/usr/bin/env bun
/**
 * Korlap MCP Server
 *
 * Exposes Korlap workspace tools to Claude agents via MCP protocol.
 * Communicates with Korlap's Tauri backend via a localhost HTTP API.
 *
 * Environment variables (set by Korlap when spawning claude):
 *   KORLAP_API_PORT  — port of the Korlap HTTP API
 *   KORLAP_WORKSPACE_ID — current workspace ID
 */

import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

const API_PORT = process.env.KORLAP_API_PORT;
const WORKSPACE_ID = process.env.KORLAP_WORKSPACE_ID;

if (!API_PORT || !WORKSPACE_ID) {
  console.error(
    "KORLAP_API_PORT and KORLAP_WORKSPACE_ID must be set",
  );
  process.exit(1);
}

const API_BASE = `http://127.0.0.1:${API_PORT}`;

async function apiCall(
  method: string,
  path: string,
  body?: Record<string, unknown>,
): Promise<{ ok: boolean; data: unknown; status: number }> {
  const url = `${API_BASE}${path}`;
  const opts: RequestInit = {
    method,
    headers: { "Content-Type": "application/json" },
  };
  if (body) {
    opts.body = JSON.stringify(body);
  }
  const res = await fetch(url, opts);
  const data = await res.json();
  return { ok: res.ok, data, status: res.status };
}

// ── Server setup ────────────────────────────────────────────────────

const server = new McpServer({
  name: "korlap",
  version: "0.1.0",
});

// ── Tools ───────────────────────────────────────────────────────────

server.tool(
  "rename_branch",
  "Rename the current workspace branch. Use this to give the branch a meaningful name based on the task. Call this early in the conversation once you understand what the user wants. Use conventional naming: feat/..., fix/..., refactor/..., chore/..., etc. Keep it concise (<30 chars).",
  {
    new_name: z.string().describe(
      "The new branch name, e.g. 'feat/add-auth-middleware' or 'fix/login-redirect'"
    ),
  },
  async ({ new_name }) => {
    const { ok, data } = await apiCall("POST", "/rename-branch", {
      workspace_id: WORKSPACE_ID,
      new_name,
    });

    if (!ok) {
      return {
        content: [
          {
            type: "text" as const,
            text: `Failed to rename branch: ${JSON.stringify(data)}`,
          },
        ],
        isError: true,
      };
    }

    return {
      content: [
        {
          type: "text" as const,
          text: `Branch renamed to: ${new_name}`,
        },
      ],
    };
  },
);

server.tool(
  "get_workspace_info",
  "Get information about the current Korlap workspace: branch name, status, worktree path, etc.",
  {},
  async () => {
    const { ok, data } = await apiCall(
      "GET",
      `/workspace-info?workspace_id=${WORKSPACE_ID}`,
    );

    if (!ok) {
      return {
        content: [
          {
            type: "text" as const,
            text: `Failed to get workspace info: ${JSON.stringify(data)}`,
          },
        ],
        isError: true,
      };
    }

    return {
      content: [
        {
          type: "text" as const,
          text: JSON.stringify(data, null, 2),
        },
      ],
    };
  },
);

server.tool(
  "notify",
  "Send a notification to the Korlap UI. Use this to communicate status updates, warnings, or completion messages to the user.",
  {
    message: z.string().describe("The notification message"),
    level: z
      .enum(["info", "warn", "error"])
      .optional()
      .describe("Notification level (default: info)"),
  },
  async ({ message, level }) => {
    const { ok } = await apiCall("POST", "/notify", {
      workspace_id: WORKSPACE_ID,
      message,
      level: level ?? "info",
    });

    return {
      content: [
        {
          type: "text" as const,
          text: ok ? "Notification sent" : "Failed to send notification",
        },
      ],
    };
  },
);

// ── LSP Tools ───────────────────────────────────────────────────────

server.tool(
  "lsp_goto_definition",
  "Find where a symbol is defined. Provide the file path (relative to workspace root), line number (1-based), and character position (1-based).",
  {
    file_path: z
      .string()
      .describe("File path relative to workspace root, e.g. 'src/main.rs'"),
    line: z.number().int().positive().describe("Line number (1-based)"),
    character: z
      .number()
      .int()
      .positive()
      .describe("Character position (1-based)"),
  },
  async ({ file_path, line, character }) => {
    const { ok, data } = await apiCall("POST", "/lsp/goto-definition", {
      workspace_id: WORKSPACE_ID,
      file_path,
      line,
      character,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

server.tool(
  "lsp_find_references",
  "Find all references to a symbol across the workspace. Use this to understand how a function/type/variable is used before modifying it.",
  {
    file_path: z
      .string()
      .describe("File path relative to workspace root"),
    line: z.number().int().positive().describe("Line number (1-based)"),
    character: z
      .number()
      .int()
      .positive()
      .describe("Character position (1-based)"),
  },
  async ({ file_path, line, character }) => {
    const { ok, data } = await apiCall("POST", "/lsp/references", {
      workspace_id: WORKSPACE_ID,
      file_path,
      line,
      character,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

server.tool(
  "lsp_hover",
  "Get type information and documentation for a symbol at a position. Use this to check a function's signature or a variable's type without opening the file.",
  {
    file_path: z
      .string()
      .describe("File path relative to workspace root"),
    line: z.number().int().positive().describe("Line number (1-based)"),
    character: z
      .number()
      .int()
      .positive()
      .describe("Character position (1-based)"),
  },
  async ({ file_path, line, character }) => {
    const { ok, data } = await apiCall("POST", "/lsp/hover", {
      workspace_id: WORKSPACE_ID,
      file_path,
      line,
      character,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

server.tool(
  "lsp_workspace_symbols",
  "Search for symbols (functions, classes, types, variables) across the workspace by name. Use this to find where something is defined when you know its name but not its file.",
  {
    query: z
      .string()
      .describe("Symbol name or partial name to search for"),
  },
  async ({ query }) => {
    const { ok, data } = await apiCall("POST", "/lsp/workspace-symbols", {
      workspace_id: WORKSPACE_ID,
      query,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

server.tool(
  "lsp_diagnostics",
  "Get compiler errors and warnings for a file. Use this after making changes to check if the code compiles correctly.",
  {
    file_path: z
      .string()
      .describe("File path relative to workspace root"),
  },
  async ({ file_path }) => {
    const { ok, data } = await apiCall("POST", "/lsp/diagnostics", {
      workspace_id: WORKSPACE_ID,
      file_path,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

server.tool(
  "lsp_rename",
  "Rename a symbol across the entire workspace. Applies changes to all files automatically. Use this for safe, compiler-accurate renames of functions, variables, types, etc.",
  {
    file_path: z
      .string()
      .describe("File path relative to workspace root"),
    line: z.number().int().positive().describe("Line number (1-based)"),
    character: z
      .number()
      .int()
      .positive()
      .describe("Character position (1-based)"),
    new_name: z
      .string()
      .describe("The new name for the symbol"),
  },
  async ({ file_path, line, character, new_name }) => {
    const { ok, data } = await apiCall("POST", "/lsp/rename", {
      workspace_id: WORKSPACE_ID,
      file_path,
      line,
      character,
      new_name,
    });

    const text = ok
      ? (data as { text: string }).text
      : `LSP error: ${JSON.stringify(data)}`;
    return {
      content: [{ type: "text" as const, text }],
      isError: !ok,
    };
  },
);

// ── Start ───────────────────────────────────────────────────────────

async function main() {
  const transport = new StdioServerTransport();
  await server.connect(transport);
  console.error(`Korlap MCP server running (workspace: ${WORKSPACE_ID})`);
}

main().catch((error) => {
  console.error("Fatal error:", error);
  process.exit(1);
});

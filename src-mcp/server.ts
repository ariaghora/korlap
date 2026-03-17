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

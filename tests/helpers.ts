import { type Page } from "@playwright/test";

/**
 * Injects a mock __TAURI_INTERNALS__ into the page before the app loads.
 * This intercepts all invoke() calls and lets tests control responses.
 */
export async function mockTauri(
  page: Page,
  handlers: Record<string, (...args: any[]) => any> = {},
) {
  await page.addInitScript((serializedHandlers) => {
    // Channel callback store
    const channelCallbacks = new Map<number, (msg: any) => void>();
    let nextChannelId = 1;

    // Mock __TAURI_INTERNALS__ which @tauri-apps/api checks for
    (window as any).__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args: any = {}) => {
        // Check if there's a handler for this command
        const handlerNames = Object.keys(serializedHandlers);
        if (handlerNames.includes(cmd)) {
          // We can't pass functions through addInitScript serialization,
          // so handlers are stored as return values
          return serializedHandlers[cmd];
        }

        // Default responses for known commands
        switch (cmd) {
          case "list_repos":
            return [];
          case "list_workspaces":
            return [];
          case "plugin:dialog|open":
            return null;
          case "plugin:event|listen":
            return 0;
          case "plugin:event|unlisten":
            return null;
          default:
            console.warn(`[mock] unhandled invoke: ${cmd}`, args);
            return null;
        }
      },
      convertFileSrc: (path: string) => path,
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
        windows: [{ label: "main" }],
        webviews: [{ label: "main" }],
      },
      // Event listener support
      transformCallback: (callback: Function) => {
        const id = nextChannelId++;
        channelCallbacks.set(id, callback as any);
        return id;
      },
    };
  }, handlers);
}

/**
 * Shorthand: mock with repos pre-loaded
 */
export async function mockTauriWithRepo(
  page: Page,
  repoName = "test-repo",
  repoPath = "/tmp/test-repo",
) {
  const repo = {
    id: "repo-1",
    path: repoPath,
    gh_profile: null,
    display_name: repoName,
    default_branch: "main",
  };

  await page.addInitScript((repoData) => {
    const channelCallbacks = new Map<number, (msg: any) => void>();
    let nextChannelId = 1;

    (window as any).__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args: any = {}) => {
        switch (cmd) {
          case "list_repos":
            return [repoData];
          case "list_workspaces":
            return [];
          case "add_repo":
            return repoData;
          case "create_workspace":
            return {
              id: "ws-" + Math.random().toString(36).slice(2, 8),
              name: "test-workspace",
              branch: "conductor/test-workspace",
              worktree_path: "/tmp/test-repo/.korlap/worktrees/ws-1",
              repo_id: repoData.id,
              gh_profile: null,
              status: "waiting",
              created_at: Math.floor(Date.now() / 1000),
            };
          case "archive_workspace":
            return null;
          case "save_messages":
            return null;
          case "load_messages":
            return [];
          case "rename_branch":
            return {
              ...repoData,
              id: args?.workspaceId ?? "ws-1",
              name: args?.newName ?? "renamed",
              branch: `conductor/${args?.newName ?? "renamed"}`,
            };
          case "get_changed_files":
            return [];
          case "get_diff":
            return "";
          case "run_script":
            return null;
          case "open_terminal":
            return null;
          case "write_terminal":
            return null;
          case "resize_terminal":
            return null;
          case "close_terminal":
            return null;
          case "send_message": {
            // Channel serializes as "__CHANNEL__:<id>" string
            const onEvent = args?.onEvent;
            let channelId: number | null = null;
            if (typeof onEvent === "string" && onEvent.startsWith("__CHANNEL__:")) {
              channelId = parseInt(onEvent.split(":")[1], 10);
            } else if (typeof onEvent === "object" && onEvent != null) {
              channelId = onEvent.__tauriChannel ?? onEvent.id;
            }
            if (channelId != null) {
              const cb = channelCallbacks.get(channelId);
              if (cb) {
                setTimeout(() => {
                  cb({
                    type: "assistant_message",
                    text: "Mock response from Claude",
                    tool_uses: [],
                  });
                  cb({ type: "done" });
                }, 100);
              }
            }
            return null;
          }
          case "plugin:dialog|open":
            return null;
          case "plugin:event|listen":
            return 0;
          case "plugin:event|unlisten":
            return null;
          default:
            console.warn(`[mock] unhandled invoke: ${cmd}`, args);
            return null;
        }
      },
      convertFileSrc: (path: string) => path,
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
        windows: [{ label: "main" }],
        webviews: [{ label: "main" }],
      },
      transformCallback: (callback: Function) => {
        const id = nextChannelId++;
        channelCallbacks.set(id, callback as any);
        return id;
      },
    };
  }, repo);
}

import { test, expect } from "@playwright/test";
import { mockTauri, mockTauriWithRepo } from "./helpers";

test.describe("empty state", () => {
  test("shows open repository button when no repos", async ({ page }) => {
    await mockTauri(page);
    await page.goto("/");
    await expect(page.locator("text=Open Repository")).toBeVisible();
    await expect(page.locator("text=Korlap")).toBeVisible();
  });
});

test.describe("with repo", () => {
  test("shows sidebar and workspace list", async ({ page }) => {
    await mockTauriWithRepo(page, "my-app");
    await page.goto("/");

    // Repo tab should be visible
    await expect(page.locator(".repo-tab").first()).toContainText("my-app");

    // Sidebar should show workspaces label
    await expect(page.locator("text=Workspaces")).toBeVisible();

    // New workspace button should exist
    await expect(page.locator("text=+ New workspace")).toBeVisible();

    // Empty panel message
    await expect(
      page.locator("text=Create a workspace to start an agent."),
    ).toBeVisible();
  });

  test("can create a workspace", async ({ page }) => {
    await mockTauriWithRepo(page, "my-app");
    await page.goto("/");

    // Click new workspace
    await page.click("text=+ New workspace");

    // Workspace should appear in sidebar
    await expect(page.locator(".ws-name")).toContainText("test-workspace");

    // Tab bar should be visible
    await expect(page.locator(".tab.active")).toContainText("Chat");

    // Chat empty state
    await expect(
      page.locator("text=Send a message to start the agent."),
    ).toBeVisible();
  });

  test("sent message appears immediately in chat", async ({ page }) => {
    await mockTauriWithRepo(page, "my-app");
    await page.goto("/");

    // Create workspace
    await page.click("text=+ New workspace");
    await expect(page.locator(".ws-name")).toContainText("test-workspace");

    // Type and send a message
    await page.fill(
      'input[placeholder="Ask to make changes, @mention files, run /commands"]',
      "hello world",
    );
    await page.click("button:text('Send')");

    // User message should appear immediately
    await expect(page.locator(".user-bubble")).toContainText("hello world");
  });

  // NOTE: Channel-based tests (agent response, thinking indicator) require
  // real Tauri runtime — the mock Channel wiring doesn't deliver messages
  // through the same path. These are tested manually in the real app.
});

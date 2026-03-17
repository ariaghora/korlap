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

    // Panel should show workspace header
    await expect(page.locator(".panel-title strong")).toContainText(
      "test-workspace",
    );

    // Chat empty state
    await expect(
      page.locator("text=Send a message to start the agent."),
    ).toBeVisible();
  });
});

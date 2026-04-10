import { test, expect } from "@playwright/test";

test.describe("Barca UI", () => {
  test("dashboard page loads", async ({ page }) => {
    await page.goto("/ui/");
    await page.locator("aside").waitFor({ state: "visible" });
    // Sidebar nav should exist
    const nav = page.locator("nav");
    await expect(nav).toHaveCount(1);
    // Barca title in sidebar
    await expect(page.getByText("Barca", { exact: true })).toBeVisible();
  });

  test("assets page loads and shows table", async ({ page }) => {
    await page.goto("/ui/assets");
    await page.locator("aside").waitFor({ state: "visible" });
    // Table should render (even if empty)
    const table = page.locator("table");
    await expect(table).toHaveCount(1);
  });

  test("assets page has filter tabs", async ({ page }) => {
    await page.goto("/ui/assets");
    await page.locator("aside").waitFor({ state: "visible" });
    // Tabs should have All, Assets, Sensors, Effects
    await expect(page.getByRole("tab", { name: "All" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Assets" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Sensors" })).toBeVisible();
    await expect(page.getByRole("tab", { name: "Effects" })).toBeVisible();
  });

  test("asset detail page loads (if assets exist)", async ({ page }) => {
    await page.goto("/ui/assets");
    await page.locator("aside").waitFor({ state: "visible" });
    const assetLinks = page.locator("tbody a");
    if ((await assetLinks.count()) > 0) {
      await assetLinks.first().click();
      await page.locator("aside").waitFor({ state: "visible" });
      // Should see "Definition" and "Latest Run" cards
      await expect(page.getByText("Definition", { exact: true })).toBeVisible();
      await expect(page.getByText("Latest Run", { exact: true })).toBeVisible();
    }
  });

  test("asset detail has refresh button (if assets exist)", async ({ page }) => {
    await page.goto("/ui/assets");
    await page.locator("aside").waitFor({ state: "visible" });
    const assetLinks = page.locator("tbody a");
    if ((await assetLinks.count()) > 0) {
      await assetLinks.first().click();
      await page.locator("aside").waitFor({ state: "visible" });
      const refreshButton = page.getByRole("button", { name: /Refresh/ });
      await expect(refreshButton).toBeVisible();
    }
  });

  test("jobs page loads and shows table", async ({ page }) => {
    await page.goto("/ui/jobs");
    await page.locator("aside").waitFor({ state: "visible" });
    const table = page.locator("table");
    await expect(table).toHaveCount(1);
    // Filter tabs
    await expect(page.getByRole("tab", { name: "All" })).toBeVisible();
  });

  test("sensors page loads and shows table", async ({ page }) => {
    await page.goto("/ui/sensors");
    await page.locator("aside").waitFor({ state: "visible" });
    const table = page.locator("table");
    await expect(table).toHaveCount(1);
  });

  test("pages do not return 500 errors", async ({ page }) => {
    const pagesToTest = ["/ui/", "/ui/assets", "/ui/jobs", "/ui/sensors"];
    for (const path of pagesToTest) {
      const response = await page.request.get(path);
      expect(response.status()).toBeLessThan(500);
    }
  });

  test("reconcile button exists on dashboard", async ({ page }) => {
    await page.goto("/ui/");
    await page.locator("aside").waitFor({ state: "visible" });
    const reconcileButton = page.getByRole("button", { name: /Reconcile/ });
    await expect(reconcileButton).toBeVisible();
  });

  test("sidebar navigation links exist", async ({ page }) => {
    await page.goto("/ui/");
    await page.locator("aside").waitFor({ state: "visible" });
    // All four nav links should be in the sidebar
    const nav = page.locator("nav");
    await expect(nav.getByText("Dashboard")).toBeVisible();
    await expect(nav.getByText("Assets")).toBeVisible();
    await expect(nav.getByText("Jobs")).toBeVisible();
    await expect(nav.getByText("Sensors")).toBeVisible();
  });

  test("clicking sidebar nav links navigates correctly", async ({ page }) => {
    await page.goto("/ui/");
    await page.locator("aside").waitFor({ state: "visible" });
    const nav = page.locator("nav");
    await nav.getByText("Assets").click();
    await expect(page).toHaveURL(/\/ui\/assets/);
    await nav.getByText("Jobs").click();
    await expect(page).toHaveURL(/\/ui\/jobs/);
    await nav.getByText("Sensors").click();
    await expect(page).toHaveURL(/\/ui\/sensors/);
    await nav.getByText("Dashboard").click();
    await expect(page).toHaveURL(/\/ui\/?$/);
  });

  test("kind filter tab click updates view", async ({ page }) => {
    await page.goto("/ui/assets");
    await page.locator("aside").waitFor({ state: "visible" });
    const assetsTab = page.getByRole("tab", { name: "Assets" });
    await assetsTab.click();
    // Should still render table after filter
    await expect(page.locator("table")).toHaveCount(1);
  });
});

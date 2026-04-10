import { test, expect, Page } from "@playwright/test";

/**
 * Visual regression tests for Barca UI layouts
 *
 * These tests capture screenshots at different viewport sizes to catch
 * CSS layout breaks. Run with:
 *   npm run test:e2e:visual
 *
 * To update baselines after intentional changes:
 *   npm run test:e2e:visual -- --update-snapshots
 *
 * Note: pages open SSE connections that stay alive, so we can't use
 * waitForLoadState("networkidle"). Instead we wait for a known element to
 * be visible and then a short delay for React to finish rendering.
 */

async function waitForPage(page: Page) {
  // Wait for the sidebar to be rendered (proves React has mounted)
  await page.locator("aside").waitFor({ state: "visible", timeout: 10_000 });
  // Let React finish rendering data-dependent content
  await page.waitForTimeout(500);
}

test.describe("Visual Regression Tests — Mobile (375px)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 });
  });

  test("dashboard page layout @visual", async ({ page }) => {
    await page.goto("/ui/");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("dashboard-mobile.png");
  });

  test("assets page layout @visual", async ({ page }) => {
    await page.goto("/ui/assets");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("assets-mobile.png");
  });

  test("jobs page layout @visual", async ({ page }) => {
    await page.goto("/ui/jobs");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("jobs-mobile.png");
  });

  test("sensors page layout @visual", async ({ page }) => {
    await page.goto("/ui/sensors");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("sensors-mobile.png");
  });
});

test.describe("Visual Regression Tests — Tablet (768px)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 });
  });

  test("dashboard page layout @visual", async ({ page }) => {
    await page.goto("/ui/");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("dashboard-tablet.png");
  });

  test("assets page layout @visual", async ({ page }) => {
    await page.goto("/ui/assets");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("assets-tablet.png");
  });

  test("jobs page layout @visual", async ({ page }) => {
    await page.goto("/ui/jobs");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("jobs-tablet.png");
  });

  test("sensors page layout @visual", async ({ page }) => {
    await page.goto("/ui/sensors");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("sensors-tablet.png");
  });
});

test.describe("Visual Regression Tests — Desktop (1200px)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1200, height: 800 });
  });

  test("dashboard page layout @visual", async ({ page }) => {
    await page.goto("/ui/");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("dashboard-desktop.png");
  });

  test("assets page layout @visual", async ({ page }) => {
    await page.goto("/ui/assets");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("assets-desktop.png");
  });

  test("jobs page layout @visual", async ({ page }) => {
    await page.goto("/ui/jobs");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("jobs-desktop.png");
  });

  test("sensors page layout @visual", async ({ page }) => {
    await page.goto("/ui/sensors");
    await waitForPage(page);
    await expect(page).toHaveScreenshot("sensors-desktop.png");
  });
});

test.describe("Component Visual Tests — Desktop (1200px)", () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1200, height: 800 });
  });

  test("topbar on dashboard @visual", async ({ page }) => {
    await page.goto("/ui/");
    await waitForPage(page);
    const topbar = page.locator("header");
    await expect(topbar).toHaveScreenshot("topbar.png");
  });

  test("assets table @visual", async ({ page }) => {
    await page.goto("/ui/assets");
    await waitForPage(page);
    const table = page.locator("table").first();
    await expect(table).toHaveScreenshot("assets-table.png");
  });

  test("sidebar navigation @visual", async ({ page }) => {
    await page.goto("/ui/");
    await waitForPage(page);
    const sidebar = page.locator("aside");
    await expect(sidebar).toHaveScreenshot("sidebar.png");
  });
});

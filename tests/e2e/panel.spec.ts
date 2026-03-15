import { test, expect } from "@playwright/test";

// Helper: wait for the server to be ready
async function waitForServer(baseURL: string) {
  for (let i = 0; i < 30; i++) {
    try {
      const res = await fetch(baseURL);
      if (res.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 500));
  }
  throw new Error("Server did not start in time");
}

// Helper: find Refresh buttons by their data-on:click__stop attribute content
function refreshButtonWithAttr(page: any, attrContains: string) {
  // Use XPath since CSS selectors can't handle colons in attribute names
  return page.locator(
    `xpath=//button[contains(., 'Refresh') and contains(@data-on\\:click__stop, '${attrContains}')]`
  );
}

test.beforeEach(async ({ baseURL }) => {
  await waitForServer(baseURL!);
});

test.describe("Asset panel", () => {
  test("clicking a stale asset refresh immediately opens panel and streams logs", async ({
    page,
  }) => {
    await page.goto("/");

    // There should be at least one asset card visible
    const cards = page.locator("article[id^='asset-card-']");
    await expect(cards.first()).toBeVisible();

    // Find stale Refresh buttons via page.evaluate (attribute has colon, tricky for selectors)
    const staleCount = await page.evaluate(() => {
      const buttons = Array.from(document.querySelectorAll("button"));
      return buttons.filter(
        (b) =>
          b.textContent?.includes("Refresh") &&
          b.getAttribute("data-on:click__stop")?.includes("refresh=true")
      ).length;
    });

    if (staleCount === 0) {
      test.skip(true, "No stale assets available — all are fresh");
      return;
    }

    // Click the first stale Refresh button
    await page.evaluate(() => {
      const buttons = Array.from(document.querySelectorAll("button"));
      const btn = buttons.find(
        (b) =>
          b.textContent?.includes("Refresh") &&
          b.getAttribute("data-on:click__stop")?.includes("refresh=true")
      );
      btn?.click();
    });

    // Panel should be closed initially — but we just clicked, so check it opens
    const panelWrapper = page.locator("#panel-wrapper");
    await expect(panelWrapper).toHaveClass(/open/, { timeout: 2000 });

    // Panel content should be populated with asset details
    const panelContent = page.locator("#panel-content");
    await expect(panelContent).not.toBeEmpty({ timeout: 5000 });

    // The panel should contain a "Live logs" section
    await expect(panelContent.locator("text=Live logs")).toBeVisible({
      timeout: 5000,
    });

    // Wait for log entries to start appearing (the job runs and produces logs)
    const logEntries = page.locator("#job-log-entries > div");
    await expect(logEntries.first()).toBeVisible({ timeout: 15000 });
  });

  test("clicking a fresh asset refresh shows confirm modal, then opens panel", async ({
    page,
  }) => {
    await page.goto("/");

    // Check for fresh assets (their Refresh buttons trigger the confirm modal)
    let freshCount = await page.evaluate(() => {
      const buttons = Array.from(document.querySelectorAll("button"));
      return buttons.filter(
        (b) =>
          b.textContent?.includes("Refresh") &&
          b.getAttribute("data-on:click__stop")?.includes("confirmModalOpen")
      ).length;
    });

    if (freshCount === 0) {
      // Materialize an asset to make it fresh
      const staleCount = await page.evaluate(() => {
        const buttons = Array.from(document.querySelectorAll("button"));
        return buttons.filter(
          (b) =>
            b.textContent?.includes("Refresh") &&
            b.getAttribute("data-on:click__stop")?.includes("refresh=true")
        ).length;
      });

      if (staleCount === 0) {
        test.skip(true, "No assets available to test");
        return;
      }

      // Click refresh on stale asset
      await page.evaluate(() => {
        const buttons = Array.from(document.querySelectorAll("button"));
        const btn = buttons.find(
          (b) =>
            b.textContent?.includes("Refresh") &&
            b.getAttribute("data-on:click__stop")?.includes("refresh=true")
        );
        btn?.click();
      });

      // Wait for panel to show completion (status badge changes to "Fresh")
      await expect(
        page.locator("#panel-content [data-tone='fresh']")
      ).toBeVisible({ timeout: 20000 });

      // Close the panel by clicking the backdrop
      await page.locator("#panel-backdrop").click();
      await expect(page.locator("#panel-wrapper")).not.toHaveClass(/open/, {
        timeout: 2000,
      });

      // Now the asset should be fresh — reload to get updated cards
      await page.goto("/");
      freshCount = await page.evaluate(() => {
        const buttons = Array.from(document.querySelectorAll("button"));
        return buttons.filter(
          (b) =>
            b.textContent?.includes("Refresh") &&
            b.getAttribute("data-on:click__stop")?.includes("confirmModalOpen")
        ).length;
      });
      if (freshCount === 0) {
        test.skip(true, "Asset did not become fresh after materialization");
        return;
      }
    }

    // Panel should be closed
    const panelWrapper = page.locator("#panel-wrapper");
    await expect(panelWrapper).not.toHaveClass(/open/);

    // Click Refresh on the fresh asset — should open confirm modal
    await page.evaluate(() => {
      const buttons = Array.from(document.querySelectorAll("button"));
      const btn = buttons.find(
        (b) =>
          b.textContent?.includes("Refresh") &&
          b.getAttribute("data-on:click__stop")?.includes("confirmModalOpen")
      );
      btn?.click();
    });

    // Confirm modal should be visible
    const modal = page.locator("text=Re-materialize?");
    await expect(modal).toBeVisible({ timeout: 2000 });

    // Panel should still be closed (modal is open, not panel)
    await expect(panelWrapper).not.toHaveClass(/open/);

    // Click "Refresh anyway"
    await page.locator("button:has-text('Refresh anyway')").click();

    // Modal should close
    await expect(modal).not.toBeVisible({ timeout: 2000 });

    // Panel should open
    await expect(panelWrapper).toHaveClass(/open/, { timeout: 2000 });

    // Panel content should be populated
    const panelContent = page.locator("#panel-content");
    await expect(panelContent).not.toBeEmpty({ timeout: 5000 });

    // Live logs should appear
    await expect(panelContent.locator("text=Live logs")).toBeVisible({
      timeout: 5000,
    });

    // Wait for actual log lines to stream in
    const logEntries = page.locator("#job-log-entries > div");
    await expect(logEntries.first()).toBeVisible({ timeout: 15000 });
  });

  test("clicking an asset card opens its panel", async ({ page }) => {
    await page.goto("/");

    const card = page.locator("article[id^='asset-card-']").first();
    await expect(card).toBeVisible();

    // Panel closed initially
    const panelWrapper = page.locator("#panel-wrapper");
    await expect(panelWrapper).not.toHaveClass(/open/);

    // Click the card body (not the refresh button)
    await card.locator("h2").first().click();

    // Panel should open
    await expect(panelWrapper).toHaveClass(/open/, { timeout: 2000 });

    // Panel should have content
    const panelContent = page.locator("#panel-content");
    await expect(panelContent).not.toBeEmpty({ timeout: 5000 });
  });

  test("Escape key closes the panel", async ({ page }) => {
    await page.goto("/");

    // Open panel by clicking a card
    await page.locator("article[id^='asset-card-'] h2").first().click();
    const panelWrapper = page.locator("#panel-wrapper");
    await expect(panelWrapper).toHaveClass(/open/, { timeout: 2000 });

    // Press Escape
    await page.keyboard.press("Escape");

    // Panel should close
    await expect(panelWrapper).not.toHaveClass(/open/, { timeout: 2000 });
  });
});

import { test, expect } from "@playwright/test";

test("page loads with no console errors", async ({ page }) => {
  const errors: string[] = [];

  // Collect console errors and warnings (ignore info/log)
  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(`[console.error] ${msg.text()}`);
    }
  });

  // Collect uncaught page errors
  page.on("pageerror", (err) => {
    errors.push(`[pageerror] ${err.message}`);
  });

  await page.goto("/");

  // Wait for Datastar to initialize and process all attributes
  await page.waitForTimeout(2000);

  // Should have at least the page shell rendered
  await expect(page.locator("body")).toBeVisible();

  // Assert zero errors
  if (errors.length > 0) {
    console.log("Console errors found:");
    for (const e of errors) console.log("  ", e);
  }
  expect(errors).toEqual([]);
});

test("navigating to jobs view produces no console errors", async ({
  page,
}) => {
  const errors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(`[console.error] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => {
    errors.push(`[pageerror] ${err.message}`);
  });

  await page.goto("/?view=jobs");
  await page.waitForTimeout(2000);

  await expect(page.locator("body")).toBeVisible();
  expect(errors).toEqual([]);
});

test("opening asset panel produces no console errors", async ({ page }) => {
  const errors: string[] = [];

  page.on("console", (msg) => {
    if (msg.type() === "error") {
      errors.push(`[console.error] ${msg.text()}`);
    }
  });
  page.on("pageerror", (err) => {
    errors.push(`[pageerror] ${err.message}`);
  });

  await page.goto("/");
  await page.waitForTimeout(1000);

  // Click the first asset card to open the panel
  const card = page.locator("article[id^='asset-card-']").first();
  const hasCards = (await card.count()) > 0;
  if (!hasCards) {
    test.skip(true, "No asset cards on page");
    return;
  }

  await card.locator("h2").first().click();

  // Wait for panel to open and SSE to deliver content
  await page.waitForTimeout(3000);

  if (errors.length > 0) {
    console.log("Console errors found after opening panel:");
    for (const e of errors) console.log("  ", e);
  }
  expect(errors).toEqual([]);
});

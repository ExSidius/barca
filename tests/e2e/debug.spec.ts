import { test, expect } from "@playwright/test";

test("debug: check datastar loading", async ({ page }) => {
  const consoleMessages: string[] = [];
  const pageErrors: string[] = [];
  const networkRequests: string[] = [];

  page.on("console", (msg) => {
    consoleMessages.push(`[${msg.type()}] ${msg.text()}`);
  });
  page.on("pageerror", (err) => pageErrors.push(err.message));
  page.on("request", (req) => {
    if (req.url().includes("datastar")) {
      networkRequests.push(`${req.method()} ${req.url()}`);
    }
  });
  page.on("response", (res) => {
    if (res.url().includes("datastar")) {
      networkRequests.push(`  -> ${res.status()} ${res.url()}`);
    }
  });
  page.on("requestfailed", (req) => {
    if (req.url().includes("datastar")) {
      networkRequests.push(`  FAILED: ${req.url()} ${req.failure()?.errorText}`);
    }
  });

  await page.goto("/");
  await page.waitForTimeout(3000);

  console.log("=== Network (datastar) ===");
  for (const n of networkRequests) console.log(n);

  console.log("=== Console ===");
  for (const m of consoleMessages) console.log(m);

  console.log("=== Page errors ===");
  for (const e of pageErrors) console.log(e);

  // Check script tags
  const scripts = await page.evaluate(() => {
    return Array.from(document.querySelectorAll("script")).map((s) => ({
      src: s.src,
      type: s.type,
      loaded: s.src ? "external" : "inline",
    }));
  });
  console.log("=== Scripts ===", JSON.stringify(scripts, null, 2));

  // Try clicking and check for SSE request
  const sseRequests: string[] = [];
  page.on("request", (req) => {
    if (req.url().includes("/panel/stream")) {
      sseRequests.push(`${req.method()} ${req.url()}`);
    }
  });

  await page.locator("article[id^='asset-card-'] h2").first().click();
  await page.waitForTimeout(3000);

  console.log("=== SSE requests after click ===");
  for (const s of sseRequests) console.log(s);

  // Check if any fetch requests were made
  const allAfterClick = await page.evaluate(() => {
    const pw = document.getElementById("panel-wrapper");
    return {
      pwStyle: pw?.style.cssText || "",
      pwClass: pw?.className || "(empty)",
      bodyClass: document.body.className,
    };
  });
  console.log("=== DOM state ===", JSON.stringify(allAfterClick));
});

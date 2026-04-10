import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: "html",
  use: {
    baseURL: "http://localhost:8401",
    trace: "on-first-retry",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  webServer: {
    // Build React UI first, then start the FastAPI server that serves it at /ui/
    command:
      "cd packages/barca/src/barca/server/ui && npm run build && cd /Users/mukulram/recursia/barca/examples/basic_app && uv run barca serve --port 8401",
    url: "http://localhost:8401/health",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
});

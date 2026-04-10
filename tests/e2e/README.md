# Barca E2E Tests

End-to-end browser tests for Barca using Playwright.

## Setup

```bash
# Install Playwright dependencies
npm install
```

## Running Tests

```bash
# Run all E2E tests (headless)
npm run test:e2e

# Run with UI (interactive test runner)
npm run test:e2e:ui

# Run with visible browser
npm run test:e2e:headed

# Run in debug mode (step through)
npm run test:e2e:debug
```

## How It Works

The `playwright.config.ts` configures:
- Base URL: `http://localhost:8401`
- Auto-starts the Barca server before running tests
- Prepares the example project with clean database

Tests are located in `ui.spec.ts` and cover:
- Dashboard page
- Assets list and filtering
- Asset detail page
- Jobs page
- Sensors list and detail
- Error handling (no 500s)
- Navigation and UI elements

## Notes

- Tests automatically start a clean Barca server for the example project
- The example database is reset before each test run
- Tests are resilient to missing data (no assets/sensors) and just verify pages load

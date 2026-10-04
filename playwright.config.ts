import { defineConfig, devices } from '@playwright/test';

// UI tests run the Svelte app in a normal browser, where the in-memory mock
// backend answers instead of Rust. Native behaviour is covered by Rust tests
// and the desktop smoke test (scripts/desktop-smoke.mjs).
export default defineConfig({
  testDir: 'tests/e2e',
  timeout: 30_000,
  fullyParallel: true,
  reporter: [['list']],
  outputDir: 'target/playwright',
  use: {
    baseURL: 'http://127.0.0.1:1420',
    trace: 'retain-on-failure',
    viewport: { width: 1280, height: 800 },
  },
  // Locally use the installed Chrome (no browser download); CI uses Playwright's Chromium.
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1280, height: 800 }, channel: process.env.CI ? undefined : 'chrome' } }],
  webServer: {
    command: 'npm run dev',
    url: 'http://127.0.0.1:1420',
    reuseExistingServer: true,
    timeout: 60_000,
  },
});

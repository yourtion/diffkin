import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests",
  fullyParallel: true,
  timeout: 30_000,
  use: {
    baseURL: "http://127.0.0.1:1421",
    viewport: { width: 1440, height: 950 },
    headless: true,
    ...(process.env.CI ? {} : { channel: "chrome" }),
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm exec rsbuild dev --port 1421",
    url: "http://127.0.0.1:1421",
    reuseExistingServer: false,
    timeout: 60_000,
  },
});

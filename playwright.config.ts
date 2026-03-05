import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests",
  timeout: 60_000,
  retries: 1,
  use: {
    baseURL: "http://127.0.0.1:4173",
    headless: true,
    viewport: { width: 1440, height: 900 },
  },
  webServer: {
    command: "python3 -m http.server 4173 --directory src/web/static",
    port: 4173,
    reuseExistingServer: true,
  },
  reporter: [["list"], ["html", { outputFolder: "playwright-report" }]],
});

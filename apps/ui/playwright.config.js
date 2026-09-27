import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  workers: 1,
  retries: 0,
  use: {
    baseURL: "http://127.0.0.1:1427",
    viewport: { width: 1180, height: 820 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    launchOptions: { args: ["--disable-gpu"] },
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1 --port 1427",
    url: "http://127.0.0.1:1427",
    reuseExistingServer: false,
    timeout: 30000,
  },
});

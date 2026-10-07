import { defineConfig } from "@playwright/test";

const http = process.env.PARLEY_E2E_HTTP ?? "http://127.0.0.1:18080";

export default defineConfig({
  testDir: "./e2e",
  timeout: 120_000,
  fullyParallel: false,
  workers: 1,
  use: {
    baseURL: http,
    headless: true,
    channel: (process.env.CI || process.env.PARLEY_BROWSER_CHANNEL === "chromium") ? "chromium" : "chrome",
  },
  webServer: {
    command: "./target/debug/parley",
    url: `${http}/healthz`,
    reuseExistingServer: false,
    timeout: 120_000,
    env: {
      PARLEY_VAPI_CHAT: "fake",
      PARLEY_TUNNEL: "0",
      PARLEY_PROVISION: "0",
      PARLEY_API_SECRET: "dev-only",
      PARLEY_OPERATOR_BOOTSTRAP: "bootstrap",
      PARLEY_BIND_HTTP: "127.0.0.1:18080",
      PARLEY_BIND_UCTP_WS: "127.0.0.1:17443",
      PARLEY_BIND_SIP: "127.0.0.1:15060",
      PARLEY_SQLITE_PATH: "/tmp/parley-e2e.sqlite",
      PARLEY_BLOB_DIR: "/tmp/parley-e2e-blobs",
    },
  },
});

import { defineConfig } from "@playwright/test";
const baseURL = "http://127.0.0.1:19439";
export default defineConfig({
  testDir: "./specs",
  workers: 1,
  use: {
    baseURL,
    viewport: { width: 1100, height: 660 },
    trace: "retain-on-failure",
  },
  webServer: {
    command:
      "cargo run --manifest-path ../Cargo.toml --locked --features dev-ui -- dev",
    env: { LUM_DEV_PORT: "19439", LUM_DEV_INFERENCE_PORT: "0" },
    wait: { stderr: /Lum UI preview:/ },
    gracefulShutdown: { signal: "SIGTERM", timeout: 5000 },
    timeout: 120_000,
    reuseExistingServer: false,
  },
});

import { test, expect } from "@playwright/test";
test("chat, add a model in the background, switch models and manage a key", async ({
  page,
}) => {
  await page.goto("/");
  await expect(page.getByRole("img", { name: "Loaded" })).toHaveCount(1);
  const prompt = page.getByLabel("Message", { exact: true });
  await prompt.fill("hello");
  await prompt.press("Enter");
  await expect(page.getByText("Mock reply from code: hello")).toBeVisible();
  await prompt.fill("unsent draft");
  await page.route("**/admin/catalog", (r) =>
    r.fulfill({ json: ["loom-fun/browser-fixture"] }),
  );
  await page.route("**/admin/presets?*", (r) =>
    r.fulfill({ json: ["chat", "chat-16gb"] }),
  );
  await page.getByRole("button", { name: "+ Add model" }).click();
  await page.getByLabel("Repository", { exact: true }).fill("fixture");
  await page.getByRole("button", { name: "loom-fun/browser-fixture" }).click();
  const download = page.getByRole("button", { name: "Download", exact: true });
  await expect(download).toBeDisabled();
  await page
    .getByRole("combobox", { name: "Preset", exact: true })
    .selectOption("chat-16gb");
  await download.click();
  await expect(page.locator("#downloads")).toBeVisible();
  await expect(page.getByRole("progressbar")).toBeVisible();
  const model = page.getByRole("link", { name: "browser-fixture" });
  await expect(model).toBeVisible({ timeout: 10000 });
  await expect(page.getByText("No active downloads.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(prompt).toHaveValue("unsent draft");
  await model.click();
  await expect(page.locator(".chat-head")).toContainText("chat-16gb");
  await page.getByRole("button", { name: "Load model" }).click();
  await expect(page.getByRole("button", { name: "Unload" })).toBeVisible();
  await page.getByRole("link", { name: "code Loaded" }).click();
  await expect(prompt).toHaveValue("unsent draft");
  await expect(page.getByText("Mock reply from code: hello")).toBeVisible();
  await page.getByRole("link", { name: "API", exact: true }).click();
  await page.getByLabel("Key name").fill("browser test");
  await page.getByRole("button", { name: "Create key" }).click();
  await expect(page.locator("#new-key")).toHaveValue(/^lum_/);
  await page.reload();
  await expect(page.getByText("browser test", { exact: true })).toBeVisible();
  await expect(page.locator("#new-key")).toHaveCount(0);
  await page.getByRole("button", { name: "Revoke" }).click();
  await expect(page.getByText("No active keys.")).toBeVisible();
});

test("a single preset is preselected but installation requires explicit Download", async ({
  page,
}) => {
  let installs = 0;
  await page.route("**/admin/downloads", (r) => {
    installs++;
    return r.fulfill({ status: 202, body: "" });
  });
  await page.route("**/admin/catalog", (r) =>
    r.fulfill({ json: ["loom-fun/solo"] }),
  );
  await page.route("**/admin/presets?*", (r) => r.fulfill({ json: ["chat"] }));
  await page.goto("/");
  await page.getByRole("button", { name: "+ Add model" }).click();
  await page.getByRole("button", { name: "loom-fun/solo" }).click();
  await expect(page.getByText("Preset: chat", { exact: true })).toBeVisible();
  const download = page.getByRole("button", { name: "Download", exact: true });
  await expect(download).toBeEnabled();
  await page.getByLabel("Repository", { exact: true }).press("Enter");
  expect(installs).toBe(0);
  await download.click();
  await expect.poll(() => installs).toBe(1);
});

test("cancelling Add model clears the form without downloading", async ({
  page,
}) => {
  await page.route("**/admin/catalog", (r) =>
    r.fulfill({ json: ["loom-fun/example"] }),
  );
  await page.route("**/admin/presets?*", (r) =>
    r.fulfill({ json: ["chat", "large"] }),
  );
  await page.goto("/");
  const open = page.getByRole("button", { name: "+ Add model" });
  await open.click();
  for (const cancel of ["Close", "Escape"]) {
    await page.getByRole("button", { name: "loom-fun/example" }).click();
    const preset = page.getByRole("combobox", { name: "Preset" });
    await preset.selectOption("large");
    if (cancel === "Close")
      await page.getByRole("button", { name: "Close", exact: true }).click();
    else await page.keyboard.press("Escape");
    await open.click();
    await expect(page.getByLabel("Repository", { exact: true })).toHaveValue(
      "",
    );
    await expect(preset).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "loom-fun/example" }),
    ).toBeVisible();
  }
});

test("removal is confirmed, not triggered by navigation", async ({ page }) => {
  await page.goto("/remove/instruct");
  await expect(
    page.getByRole("heading", { name: "Remove instruct?" }),
  ).toBeVisible();
  await page.getByRole("link", { name: "Cancel" }).click();
  await page.getByRole("link", { name: "instruct" }).click();
  await page.getByRole("link", { name: "Remove…" }).click();
  await page.getByRole("button", { name: "Remove files" }).click();
  await expect(
    page.getByRole("navigation", { name: "Downloaded models" }),
  ).toBeVisible();
  await expect(page.getByRole("link", { name: "code Loaded" })).toBeVisible();
  await expect(page.getByRole("link", { name: "instruct" })).toHaveCount(0);
});

test("setup retries and continues to the requested screen", async ({
  page,
}) => {
  let retried = false;
  await page.route("**/admin/runtime", (r) =>
    r.fulfill({
      json: {
        runtime: "Test SDK",
        setup: retried
          ? { phase: "ready" }
          : { phase: "failed", error: "SDK unavailable" },
      },
    }),
  );
  await page.route("**/admin/runtime/retry", (r) => {
    retried = r.request().method() === "POST";
    return r.fulfill({ status: 202, body: "" });
  });
  await page.goto("/api");
  await expect(page.getByText("SDK unavailable")).toBeVisible();
  await page.getByRole("button", { name: "Retry setup" }).click();
  await expect(
    page.getByRole("heading", { name: "API management" }),
  ).toBeVisible();
});

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
  const downloads = page.getByRole("button", {
    name: "Downloads: 0 queued or active",
  });
  await downloads.click();
  await expect(page.getByText("No active downloads.")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("link", { name: "API keys", exact: true }).click();
  const base = page.locator("code").filter({ hasText: /^https?:\/\/.+\/v1$/ });
  await expect(base).toBeVisible();
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.getByRole("button", { name: "Copy client base URL" }).click();
  await expect(page.getByRole("status")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    await base.textContent(),
  );
  const keyName = page.getByLabel("Key name");
  const createKey = page.getByRole("button", { name: "Create key" });
  await keyName.fill("browser test");
  expect((await createKey.boundingBox())?.height).toBe(
    (await keyName.boundingBox())?.height,
  );
  const beforeCreate = await createKey.boundingBox();
  await createKey.click();
  const generatedKey = page.locator("#new-key code");
  await expect(generatedKey).toHaveText(/^lum_/);
  expect(await createKey.boundingBox()).toEqual(beforeCreate);
  await page.getByRole("button", { name: "Copy generated API key" }).click();
  await expect(page.locator("#new-key").getByRole("status")).toHaveText(
    "Copied",
  );
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(
    await generatedKey.textContent(),
  );
  await page.reload();
  await expect(page.getByText("browser test", { exact: true })).toBeVisible();
  await expect(page.locator("#new-key")).toHaveCount(0);
  await page.getByRole("button", { name: "Revoke" }).click();
  await expect(page.getByText("No active keys.")).toBeVisible();
});

test("submitting clears the composer before the reply and restores it on error", async ({
  page,
}) => {
  let release!: () => void;
  const pending = new Promise<void>((resolve) => (release = resolve));
  await page.route("**/admin/chat/code", async (route) => {
    await pending;
    await route.fulfill({ status: 400, body: "No displayable answer" });
  });
  await page.goto("/chat/code");
  const prompt = page.getByLabel("Message", { exact: true });
  await prompt.fill("retry this");
  await prompt.press("Enter");
  try {
    await expect(prompt).toHaveValue("");
    await expect(page.locator('[data-role="user"]')).toContainText(
      "retry this",
    );
  } finally {
    release();
  }
  await expect(prompt).toHaveValue("retry this");
  await expect(page.locator('[data-role="assistant"]')).toHaveCount(0);
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

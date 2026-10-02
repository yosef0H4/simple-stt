const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const f = require("./settings-fixture.cjs");
(async () => {
  await new Promise((r) => f.server.listen(0, "127.0.0.1", r));
  const browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({
    viewport: { width: 1280, height: 900 },
    permissions: ["clipboard-read", "clipboard-write"],
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("dialog", (dialog) => dialog.dismiss());
  const url = `http://127.0.0.1:${f.server.address().port}/#token=${f.token}`;
  const go = async (name) => {
    await page.locator(`nav button[data-page="${name}"]`).click();
    await page.locator(`section[data-section="${name}"]`).waitFor();
  };
  const save = async () => {
    await page.locator("#savebar button[type=submit]").click();
    await page.getByText("Saved and applied.", { exact: true }).waitFor();
  };
  const draft = async () => {
    const active = await page
      .locator('nav button[aria-current="page"]')
      .getAttribute("data-page");
    await go("config");
    const value = JSON.parse(await page.locator("#json").inputValue());
    await go(active);
    return value;
  };
  await page.goto(url);
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  const initial = f.getConfig();
  for (const name of [
    "general",
    "audio",
    "models",
    "output",
    "cleanup",
    "advanced",
    "config",
  ]) {
    await go(name);
    assert.equal(await page.locator("section[data-section]").count(), 1);
  }
  await go("general");
  await page
    .locator('[data-setting-path="general.start_at_login"] input')
    .check();
  await go("output");
  await page.locator('[data-setting-path="output.lowercase"] input').check();
  await go("general");
  assert(
    await page
      .locator('[data-setting-path="general.start_at_login"] input')
      .isChecked(),
  );
  assert.equal(
    f.getConfig().general.start_at_login,
    initial.general.start_at_login,
    "draft did not save automatically",
  );
  await save();
  assert.equal(f.getConfig().output.lowercase, true);
  await go("audio");
  await page.locator('[data-setting-path="audio.gain"] .help button').focus();
  assert(
    await page
      .locator('[data-setting-path="audio.gain"] .help [role=tooltip]')
      .isVisible(),
  );
  await go("general");
  await page
    .getByRole("button", { name: "Record Record shortcut", exact: true })
    .click();
  await page.waitForFunction(
    () =>
      document.getElementById("general.record_hotkey").value === "Ctrl+Alt+R",
  );
  await save();
  await go("output");
  await page
    .locator('[data-setting-path="output.paced_typing_enabled"] input')
    .uncheck();
  assert(
    await page
      .locator('[data-setting-path="output.typing_speed_wpm"] input')
      .isDisabled(),
  );
  await page.getByRole("button", { name: "Reset Typing to defaults" }).click();
  await page.waitForFunction(
    () =>
      document.querySelector(
        '[data-setting-path="output.paced_typing_enabled"] input',
      ).checked,
  );
  assert(
    await page
      .locator('[data-setting-path="output.paced_typing_enabled"] input')
      .isChecked(),
  );
  await page.getByRole("button", { name: "Add manually" }).click();
  await page
    .getByRole("textbox", { name: "Application identity" })
    .fill("terminal-test");
  await page
    .getByRole("combobox", { name: "Delivery mode for terminal-test" })
    .selectOption("paste_ctrl_shift_v");
  await save();
  assert.equal(
    f.getConfig().output.app_overrides[0].mode,
    "paste_ctrl_shift_v",
  );
  await page.getByRole("button", { name: "Remove app override" }).click();
  await page
    .getByRole("button", { name: "Add current app", exact: true })
    .click();
  await page.getByRole("textbox", { name: "Application identity" }).waitFor();
  assert.equal((await draft()).output.app_overrides[0].app_id, "test-app");
  await go("cleanup");
  assert(
    await page
      .locator('[data-setting-path="cleanup.screenshot.enabled"] input')
      .isDisabled(),
  );
  await page.locator('[data-setting-path="cleanup.enabled"] input').check();
  await page
    .locator('[data-setting-path="cleanup.screenshot.enabled"] input')
    .check();
  await page
    .getByRole("textbox", { name: "Never capture" })
    .fill("private-app\nsecret-app");
  await page
    .getByRole("textbox", { name: "Provider API key" })
    .fill("fixture-key");
  await page.getByRole("button", { name: "Save API key" }).click();
  await page.getByRole("button", { name: "Remove saved API key" }).waitFor();
  assert.equal(
    await page.getByRole("textbox", { name: "Provider API key" }).inputValue(),
    "",
  );
  await page.getByRole("button", { name: "Fetch models", exact: true }).click();
  await page
    .getByRole("combobox", { name: "Cleanup model" })
    .fill("provider-model");
  await page
    .getByRole("combobox", { name: "Cleanup model" })
    .press("ArrowDown");
  await page.getByRole("combobox", { name: "Cleanup model" }).press("Enter");
  await page.getByRole("button", { name: "Test cleanup", exact: true }).click();
  await page.getByText("مرحبا · Hello Jayson", { exact: true }).waitFor();
  await page
    .locator('[data-setting-path="cleanup.provider"] select')
    .selectOption("chat_gpt");
  await page
    .getByRole("button", { name: "Connect ChatGPT", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Disconnect ChatGPT", exact: true })
    .waitFor({ timeout: 8000 });
  await page
    .getByRole("button", { name: "Disconnect ChatGPT", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Connect ChatGPT", exact: true })
    .waitFor();
  await page.locator('[data-setting-path="cleanup.enabled"] input').uncheck();
  assert.equal((await draft()).cleanup.screenshot.enabled, false);
  await save();
  await page
    .locator('[data-setting-path="cleanup.provider"] select')
    .selectOption("open_ai_compatible");
  await page.getByRole("button", { name: "Remove saved API key" }).click();
  await page
    .locator('[data-setting-path="cleanup.provider"] select')
    .selectOption("chat_gpt");
  assert.equal(f.options.keySaved, false);
  f.options.history = [
    {
      raw: "مرحبا الأصل",
      cleaned: "مرحبا العالم",
      model: "test",
      latency_ms: 10,
      outcome: "cleaned",
    },
  ];
  await page
    .getByRole("button", { name: "Connect using a code", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Disconnect ChatGPT", exact: true })
    .waitFor();
  assert.equal(
    await page.evaluate(() => navigator.clipboard.readText()),
    "TEST-CODE",
  );
  await page
    .getByRole("button", { name: "Copy cleaned text", exact: true })
    .click();
  assert.equal(
    await page.evaluate(() => navigator.clipboard.readText()),
    "مرحبا العالم",
  );
  await page
    .getByRole("button", { name: "Clear cleanup history", exact: true })
    .click();
  await page.getByText("No cleaned dictation yet", { exact: true }).waitFor();
  f.options.cleanupDelay = 5000;
  await page.getByRole("button", { name: "Test cleanup", exact: true }).click();
  for (let i = 0; i < 50 && !f.options.pendingCleanup; i++)
    await new Promise((r) => setTimeout(r, 10));
  assert.equal(f.options.pendingCleanup, 1);
  await go("general");
  for (let i = 0; i < 50 && f.options.pendingCleanup; i++)
    await new Promise((r) => setTimeout(r, 10));
  assert.equal(
    f.options.pendingCleanup,
    0,
    "page-owned request must abort on navigation",
  );
  f.options.cleanupDelay = 0;
  await go("advanced");
  await page
    .locator('[data-setting-path="diagnostics.log_transcripts"] input')
    .check();
  await save();
  await page
    .getByRole("combobox", { name: "Search all settings" })
    .fill("gain");
  await page
    .getByRole("combobox", { name: "Search all settings" })
    .press("Enter");
  await page.locator('[data-setting-path="audio.gain"] input').waitFor();
  assert.equal(
    await page
      .locator('[data-setting-path="audio.gain"] input')
      .evaluate((e) => e === document.activeElement),
    true,
  );
  await go("config");
  const exported = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Export", exact: true }).click(),
  ]);
  const exportPath = await exported[0].path();
  assert.equal(JSON.parse(fs.readFileSync(exportPath)).schema_version, 9);
  await page.getByRole("button", { name: "Copy JSON", exact: true }).click();
  assert.equal(
    JSON.parse(await page.evaluate(() => navigator.clipboard.readText()))
      .schema_version,
    9,
  );
  await page.locator("#json").fill("{broken");
  await page.locator("#savebar button[type=submit]").click();
  await page.locator("#notice").waitFor();
  assert.match(await page.locator("#notice").innerText(), /JSON|SyntaxError/);
  await page.getByRole("button", { name: "Reload", exact: true }).click();
  await page.waitForFunction(() =>
    document.querySelector("#json").value.startsWith("{\n"),
  );
  assert.equal(
    JSON.parse(await page.locator("#json").inputValue()).schema_version,
    9,
  );
  const edited = JSON.parse(await page.locator("#json").inputValue());
  edited.audio.gain = 1.4;
  await page.locator("#json").fill(JSON.stringify(edited));
  await go("general");
  await go("config");
  assert.equal(
    JSON.parse(await page.locator("#json").inputValue()).audio.gain,
    1.4,
  );
  await save();
  assert.equal(f.getConfig().audio.gain, 1.4);
  await go("audio");
  await page.locator('[data-setting-path="audio.gain"] input').fill("1.5");
  f.options.conflict = true;
  await page.locator("#savebar button[type=submit]").click();
  await page
    .getByText("config changed outside Settings", { exact: true })
    .waitFor();
  assert.equal((await draft()).audio.gain, 1.5);
  f.options.conflict = false;
  await save();
  f.options.online = false;
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  await go("audio");
  assert(await page.locator("[data-model-test]").isDisabled());
  await page.locator('[data-setting-path="audio.gain"] input').fill("1.6");
  await page.locator("#savebar button[type=submit]").click();
  await page.locator("#savebar").waitFor({ state: "detached" });
  f.options.online = true;
  f.options.platform = "linux";
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  await page
    .getByRole("button", { name: "Configure system shortcuts" })
    .click();
  await go("output");
  await page
    .getByRole("combobox", { name: "Current Linux delivery method" })
    .selectOption("native|smart_paste");
  await page.getByText("Cycle between", { exact: true }).click();
  await page
    .getByRole("checkbox", {
      name: "Include Clipboard only wl-clipboard in delivery cycle",
      exact: true,
    })
    .check();
  await save();
  assert.equal(f.getConfig().output.linux_automation_backend, "native");
  assert.equal(
    f.getConfig().output.linux_delivery_cycle[0].backend,
    "clipboard_only",
  );
  const folder = path.resolve("artifacts/settings-overhaul");
  fs.mkdirSync(folder, { recursive: true });
  for (const theme of ["light", "dark"])
    for (const width of [360, 768, 1280]) {
      f.getConfig().general.ui_theme = theme;
      await page.setViewportSize({ width, height: 900 });
      await page.reload();
      await page
        .locator('[data-setting-path="general.enabled"] input')
        .waitFor();
      for (const name of [
        "general",
        "audio",
        "models",
        "output",
        "cleanup",
        "advanced",
        "config",
      ]) {
        await go(name);
        assert.equal(
          await page.evaluate(
            () => document.documentElement.scrollWidth > innerWidth,
          ),
          false,
          `${name} ${theme} ${width} overflow`,
        );
        if (process.env.SETTINGS_VISUAL_REVIEW)
          await page.screenshot({
            path: path.join(folder, `${name}-${theme}-${width}.png`),
          });
      }
    }
  f.getConfig().general.ui_theme = "auto";
  await page.emulateMedia({ colorScheme: "dark" });
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  assert.equal(
    await page.evaluate(() =>
      getComputedStyle(document.documentElement)
        .getPropertyValue("--paper")
        .trim(),
    ),
    "#151a23",
  );
  f.getConfig().speech.single_model_filename =
    "long-unavailable-model-".repeat(10) + ".gguf";
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  await go("audio");
  assert.match(
    await page
      .getByRole("combobox", { name: "Speech model", exact: true })
      .inputValue(),
    /Unavailable/,
  );
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
    "long filename overflow",
  );
  await page
    .getByRole("combobox", { name: "Speech model", exact: true })
    .fill("no-such-model");
  assert(await page.getByText("No matches", { exact: true }).isVisible());
  await page
    .getByRole("combobox", { name: "Speech model", exact: true })
    .press("Escape");
  await page.setViewportSize({ width: 360, height: 900 });
  await page.locator('nav button[data-page="output"]').focus();
  assert(
    await page.locator('nav button[data-page="output"] span').isVisible(),
    "icon navigation focus tooltip",
  );
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.evaluate(() => (document.body.style.zoom = "2"));
  await go("audio");
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > innerWidth,
    ),
    false,
    "200% zoom overflow",
  );
  assert.deepEqual(errors, []);
  console.log(
    "PASS all Settings pages: drafts, reset/save, shortcuts, delivery, cleanup, credentials, OAuth, search, JSON, conflicts, offline, Linux, themes and layouts",
  );
  await browser.close();
  f.closeEvents();
  await new Promise((r) => f.server.close(r));
})().catch((e) => {
  console.error(e);
  process.exit(1);
});

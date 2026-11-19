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
    if (name === "output") name = "audio";
    if (name === "config") {
      await page.locator('nav button[data-page="advanced"]').click();
      await page.locator('section[data-section="advanced"]').waitFor();
      const details = page.locator('[data-settings-json]');
      await details.evaluate((element) => { element.open = true; });
      return;
    }
    await page.locator(`nav button[data-page="${name}"]`).click();
    await page.locator(`section[data-section="${name}"]`).waitFor();
  };
  const catalog = async (feature) => {
    await go("models");
    await page.locator(`[data-catalog-feature="${feature}"]`).click();
  };
  const choose = async (settingPath, query, option) => {
    const input = page.locator(`[data-setting-path="${settingPath}"] .combo-input`);
    await input.click();
    await input.fill(query);
    await page.locator(`[data-combo-list="${settingPath}"]`).getByRole("option", { name: option }).click();
  };
  const save = async () => {
    await page.locator("#savebar button[type=submit]").click();
    // A success notice from the preceding save can still be visible. Wait for
    // this draft to become clean before navigating or reloading the page.
    await page.locator("#savebar").waitFor({ state: "detached" });
    await page.getByText("Saved and applied.", { exact: true }).waitFor();
  };
  const draft = async () => {
    const active = await page
      .locator('nav button[aria-current="page"]')
      .getAttribute("data-page");
    await go("config");
    const value = JSON.parse(await page.locator("#json").inputValue());
    if (active === "advanced") await go("advanced");
    else await go(active);
    return value;
  };
  await page.goto(url);
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  const initial = f.getConfig();
  for (const name of [
    "general",
    "audio",
    "reading",
    "models",
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
  await go("audio");
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
  assert(await page.locator('[data-setting-path="audio.gain"] .help button').getAttribute("title"));
  assert.equal(await page.locator('[role="tooltip"]').count(), 0);
  const finishDelay = page.locator('[data-setting-path="audio.stop_grace_ms"] .range-number');
  assert.equal(await finishDelay.inputValue(), "250", "Dictation defaults to a 250 ms finish delay");
  await finishDelay.fill("1250");
  await save();
  assert.equal(f.getConfig().audio.stop_grace_ms, 1250);
  await go("audio");
  await page
    .getByRole("button", { name: "Record Record shortcut", exact: true })
    .click();
  await page.waitForFunction(
    () =>
      document.getElementById("general.record_hotkey").value === "Ctrl+Alt+R",
  );
  await save();
  f.options.hotkey = "Ctrl+Alt+P";
  await page.getByRole("button", { name: "Record Retry last action shortcut", exact: true }).click();
  await page.waitForFunction(() => document.getElementById("general.retry_delivery_hotkey").value === "Ctrl+Alt+P");
  await save();
  assert.equal(f.getConfig().general.retry_delivery_hotkey, "Ctrl+Alt+P");
  await go("audio");
  const clean = page.locator('[data-setting-path="output.preserve_clipboard"] input');
  assert(await clean.isChecked());
  await clean.uncheck();
  assert.equal(f.getConfig().output.preserve_clipboard, true, "clipboard option is draft-only");
  await save();
  assert.equal(f.getConfig().output.preserve_clipboard, false);
  await page.reload();
  await go("audio");
  assert(!(await page.locator('[data-setting-path="output.preserve_clipboard"] input').isChecked()));
  const delivery = page.getByRole("combobox", {
    name: "Current Windows delivery method",
  });
  await delivery.fill("Ctrl+Shift+V");
  await delivery.press("ArrowDown");
  await delivery.press("Enter");
  await save();
  assert.equal(f.getConfig().output.delivery_mode, "paste_ctrl_shift_v");
  await delivery.fill("arbitrary-method");
  await delivery.press("Escape");
  assert.equal((await draft()).output.delivery_mode, "paste_ctrl_shift_v");
  await delivery.fill("Smart Paste");
  await page.getByRole("option", { name: "Smart Paste", exact: true }).click();
  await save();
  const typing = page.locator('#typing-speed');
  await typing.focus();
  await typing.press('Home');
  await save();
  await typing.evaluate((input) => {
    input.value = '50';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  assert.equal(await typing.getAttribute('aria-valuetext'), '450 WPM');
  await save();
  assert.equal(f.getConfig().output.typing_speed_wpm, 450);
  assert.equal(f.getConfig().output.paced_typing_enabled, true);
  await page.reload();
  await go('audio');
  assert.equal(await typing.inputValue(), '50', '450 WPM is the midpoint after reopening');
  const deliveryBeforeInstant = f.getConfig().output.delivery_mode;
  await typing.focus();
  await typing.press('End');
  assert.equal(await typing.inputValue(), '100');
  assert.equal(await typing.getAttribute('aria-valuetext'), 'Instant');
  await save();
  assert.equal(f.getConfig().output.paced_typing_enabled, false);
  assert.equal(f.getConfig().output.typing_speed_wpm, 450, 'Instant preserves the finite speed');
  assert.equal(f.getConfig().output.delivery_mode, deliveryBeforeInstant);
  await page.reload();
  await go('audio');
  assert.equal(await typing.inputValue(), '100', 'Instant survives reopening');
  await typing.focus();
  await typing.press('ArrowLeft');
  assert.equal(await typing.inputValue(), '99');
  assert.equal(await typing.getAttribute('aria-valuetext'), '1650 WPM');
  await save();
  assert.equal(f.getConfig().output.paced_typing_enabled, true);
  assert.equal(f.getConfig().output.typing_speed_wpm, 1650);
  const typingNumber = page.locator('[data-setting-path="output.typing_speed_wpm"] .range-number');
  assert.equal(await typingNumber.inputValue(), "1650", "exact value stays synchronized with the slider");
  await page.getByRole("button", { name: "Reset Typing to defaults" }).click();
  await page.waitForFunction(() => document.querySelector('#typing-speed').value !== '99');
  assert.notEqual(await typing.inputValue(), '100');
  await typingNumber.fill("800");
  await save();
  assert.equal(f.getConfig().output.typing_speed_wpm, 800, "numeric range input saves an exact value");
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
  await page.locator('[data-setting-path="cleanup.screenshot.enabled"] input').waitFor();
  assert.equal(await page.getByRole("textbox", { name: "Provider API key" }).count(), 0,
    "cleanup credentials are not duplicated on the Text cleanup page");
  assert(await page.locator('[data-setting-path="cleanup.screenshot.enabled"] input').isDisabled());
  await page.locator('[data-setting-path="cleanup.enabled"] input').check();
  await page.locator('[data-setting-path="cleanup.screenshot.enabled"] input').check();
  await page.getByRole("textbox", { name: "Never capture" }).fill("private-app\nsecret-app");

  await catalog("cleanup");
  await page.getByRole("textbox", { name: "Provider API key" }).fill("fixture-key");
  await page.getByRole("button", { name: "Save API key" }).click();
  await page.getByRole("button", { name: "Remove saved API key" }).waitFor();
  assert.equal(await page.getByRole("textbox", { name: "Provider API key" }).inputValue(), "");
  await page.getByRole("button", { name: "Fetch models", exact: true }).click();
  await page.getByRole("combobox", { name: "Cleanup model" }).fill("provider-model");
  await page.getByRole("combobox", { name: "Cleanup model" }).press("ArrowDown");
  await page.getByRole("combobox", { name: "Cleanup model" }).press("Enter");
  await go("cleanup");
  await page.getByRole("button", { name: "Test cleanup", exact: true }).click();
  await page.getByText("مرحبا · Hello Jayson", { exact: true }).waitFor();
  await catalog("cleanup");
  await choose("cleanup.provider", "ChatGPT", /ChatGPT/);
  await page.getByRole("button", { name: "Connect ChatGPT", exact: true }).click();
  await page.getByRole("button", { name: "Disconnect ChatGPT", exact: true }).waitFor({ timeout: 8000 });
  await page.getByRole("button", { name: "Disconnect ChatGPT", exact: true }).click();
  await page.getByRole("button", { name: "Connect ChatGPT", exact: true }).waitFor();
  await choose("cleanup.provider", "OpenAI", /OpenAI/);
  await page.getByRole("button", { name: "Remove saved API key" }).click();
  await choose("cleanup.provider", "ChatGPT", /ChatGPT/);
  assert.equal(f.options.keySaved, false);
  f.options.history = [{raw:"مرحبا الأصل",cleaned:"مرحبا العالم",model:"test",latency_ms:10,outcome:"cleaned"}];
  await page.getByRole("button", { name: "Connect using a code", exact: true }).click();
  await page.getByRole("button", { name: "Disconnect ChatGPT", exact: true }).waitFor();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), "TEST-CODE");
  await go("cleanup");
  await page.getByRole("button", { name: "Copy cleaned text", exact: true }).click();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), "مرحبا العالم");
  await page.getByRole("button", { name: "Clear cleanup history", exact: true }).click();
  await page.getByText("No cleaned dictation yet", { exact: true }).waitFor();
  await go("cleanup");
  await page.locator('[data-setting-path="cleanup.enabled"] input').uncheck();
  assert.equal((await draft()).cleanup.screenshot.enabled, false);
  await save();
  f.options.cleanupDelay = 5000;
  await page.getByRole("button", { name: "Test cleanup", exact: true }).click();
  for (let i = 0; i < 50 && !f.options.pendingCleanup; i++)
    await new Promise((r) => setTimeout(r, 10));
  assert.equal(f.options.pendingCleanup, 1);
  await go("general");
  for (let i = 0; i < 50 && f.options.pendingCleanup; i++)
    await new Promise((r) => setTimeout(r, 10));
  assert.equal(f.options.pendingCleanup, 0, "page-owned request must abort on navigation");
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
  await page.locator('[data-setting-path="audio.gain"] input[type="range"]').waitFor();
  assert.equal(
    await page
      .locator('[data-setting-path="audio.gain"] input[type="range"]')
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
  await page.locator('[data-setting-path="audio.gain"] .range-number').fill("1.5");
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
  assert.equal(await page.locator('[data-state="offline"]').count(), 1, "offline state is shown");
  await go("models");
  await page.locator('[data-catalog-feature="stt"]').click();
  assert(await page.locator("#stt-mode").isEnabled(), "model routing remains editable offline");
  await go("audio");
  await page.locator('[data-setting-path="audio.gain"] .range-number').fill("1.6");
  await page.locator("#savebar button[type=submit]").click();
  await page.locator("#savebar").waitFor({ state: "detached" });
  f.options.online = true;
  f.options.platform = "linux";
  f.getConfig().output.linux_automation_backend = "xdotool";
  f.getConfig().output.linux_delivery_cycle = [
    { mode: "type", backend: "ydotool" },
    { mode: "type", backend: "xdotool" },
    { mode: "clipboard", backend: "clipboard_only" },
  ];
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  await page
    .getByRole("button", { name: "Set up shortcuts", exact: true })
    .click();
  await go("audio");
  const linuxDelivery = page.getByRole("combobox", { name: "Current Linux delivery method" });
  await linuxDelivery.fill("");
  assert.equal(await page.locator('[data-combo-list="output.delivery_mode"]').getByRole("option").count(), 6, "Linux exposes the six delivery actions without backend variants");
  assert.equal(await page.getByText("Tool setup", { exact: true }).count(), 0, "Linux delivery has no tool installation matrix");
  await linuxDelivery.fill("Ctrl+V");
  await page.locator('[data-combo-list="output.delivery_mode"]').getByRole("option", { name: "Ctrl+V", exact: true }).click();
  await page.getByText("Cycle between", { exact: true }).click();
  const typeCycle = page.getByRole("checkbox", { name: "Include Type in delivery cycle", exact: true });
  assert(await typeCycle.isChecked(), "legacy duplicate Type entries normalize by mode");
  await typeCycle.uncheck();
  await page.getByRole("checkbox", { name: "Include Ctrl+V in delivery cycle", exact: true }).check();
  await save();
  assert.equal(f.getConfig().output.linux_automation_backend, "auto");
  assert.deepEqual(f.getConfig().output.linux_delivery_cycle, [
    { mode: "clipboard", backend: "auto" },
    { mode: "paste_ctrl_v", backend: "auto" },
  ]);
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
        "reading",
        "models",
        "cleanup",
        "advanced",
      ]) {
        await go(name);
        const railBounds = await page.locator(".rail").boundingBox();
        assert.equal(railBounds.y, 0, `${name} navigation starts at viewport top`);
        if (width > 700) {
          assert.equal(railBounds.height, 900, `${name} desktop sidebar fills viewport`);
          await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
          assert.equal((await page.locator(".rail").boundingBox()).y, 0,
            `${name} desktop sidebar stays anchored while scrolling`);
        } else {
          assert(railBounds.height < 900, `${name} mobile navigation uses compact header height`);
          await page.locator("nav").waitFor();
        }
        const closeButton = page.getByRole("button", { name: "Close Settings", exact: true });
        assert.equal(await closeButton.getAttribute("title"), "Close Settings");
        assert.equal(await page.locator('[role="tooltip"]').count(), 0);
        await page.evaluate(() => window.scrollTo(0, 0));
        await page.mouse.move(width - 1, 0);
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
  f.getConfig().speech.selection_mode = "single_model";
  f.getConfig().speech.single_model_filename =
    "long-unavailable-model-".repeat(10) + ".gguf";
  await page.reload();
  await page.locator('[data-setting-path="general.enabled"] input').waitFor();
  await go("models");
  await page.locator('[data-catalog-feature="stt"]').click();
  assert.match(
    await page
      .locator('[data-setting-path="stt.single.model"] .combo-input')
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
    .locator('[data-setting-path="stt.single.model"] .combo-input')
    .fill("no-such-model");
  assert(await page.locator('[data-combo-list="stt.single.model"]').getByText("No matches", { exact: true }).isVisible());
  await page
    .locator('[data-setting-path="stt.single.model"] .combo-input')
    .press("Escape");
  await page.setViewportSize({ width: 360, height: 900 });
  await page.locator('nav button[data-page="audio"]').focus();
  assert(
    await page.locator('nav button[data-page="audio"]').getAttribute("title"),
    "icon navigation native tooltip",
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

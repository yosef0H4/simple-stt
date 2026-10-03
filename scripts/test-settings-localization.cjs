const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { chromium } = require("playwright");
const f = require("./settings-fixture.cjs");

const pages = ["general", "audio", "models", "output", "cleanup", "advanced", "config"];
const locales = ["en", "ar"];
const themes = ["light", "dark"];
const widths = [360, 768, 1280];

(async () => {
  let browser;
  try {
    f.options.systemLocale = "ar";
    await new Promise((resolve) => f.server.listen(0, "127.0.0.1", resolve));
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${f.server.address().port}/#token=${f.token}`);
    await page.locator('[data-setting-path="general.ui_language"] select').waitFor();

    const language = page.locator('[data-setting-path="general.ui_language"] select');
    assert.equal(await page.locator("html").getAttribute("lang"), "ar", "auto should follow mocked Arabic system locale");
    await page.reload();
    await page.locator('[data-setting-path="general.ui_language"] select').waitFor();
    assert.equal(await page.locator("html").getAttribute("lang"), "ar", "auto locale should survive reload");
    await language.selectOption("ar");
    await page.waitForFunction(() => document.documentElement.lang === "ar");
    assert.equal(await page.locator("html").getAttribute("dir"), "rtl");
    await page.getByRole("heading", { name: "عام" }).waitFor();
    const rail = await page.locator(".rail").boundingBox();
    const main = await page.locator("main").boundingBox();
    assert(rail.x > main.x, "RTL rail should occupy the right edge");

    const search = page.getByRole("combobox", { name: "ابحث في كل الإعدادات" });
    await search.fill("الميكروفون");
    await page.getByRole("option", { name: /الميكروفون/ }).waitFor();
    await search.fill("microphone");
    await page.getByRole("option", { name: /الميكروفون/ }).waitFor();
    await search.fill("");
    await language.selectOption("en");
    await page.waitForFunction(() => document.documentElement.dir === "ltr");
    await page.getByRole("heading", { name: "General" }).waitFor();
    await language.selectOption("ar");
    await page.locator("#savebar").waitFor();
    for (const width of [1280, 360]) {
      await page.setViewportSize({ width, height: 900 });
      const bar = await page.locator("#savebar").boundingBox();
      const content = await page.locator("main").boundingBox();
      assert(bar && content, "save bar and content should be visible");
      assert(
        Math.abs(bar.x + bar.width / 2 - (content.x + content.width / 2)) <= 2,
        `RTL save bar should be centered over content at ${width}px`,
      );
    }
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.locator("#savebar button[type=submit]").click();
    await page.locator("#savebar").waitFor({ state: "detached" });
    assert.equal(f.getConfig().general.ui_language, "ar", "locale should persist when saved");
    const savedSpeech = structuredClone(f.getConfig().speech);
    await page.reload();
    await page.locator('[data-setting-path="general.ui_language"] select').waitFor();
    assert.equal(await page.locator("html").getAttribute("lang"), "ar", "saved locale should be restored on reopen");
    assert.deepEqual(f.getConfig().speech, savedSpeech, "locale changes must not alter speech model settings");

    if (process.env.SETTINGS_VISUAL_REVIEW === "1")
      fs.mkdirSync(path.resolve("artifacts/localization"), { recursive: true });
    for (const locale of locales) {
      await page.locator('nav button[data-page="general"]').click();
      await language.selectOption(locale);
      for (const theme of themes) {
        await page.locator('nav button[data-page="general"]').click();
        await page.locator('[data-setting-path="general.ui_theme"] select').selectOption(theme);
        for (const width of widths) {
          await page.setViewportSize({ width, height: 900 });
          for (const name of pages) {
            await page.locator(`nav button[data-page="${name}"]`).click();
            await page.locator(`section[data-section="${name}"]`).waitFor();
            assert.equal(
              await page.locator("html").evaluate((node) => node.scrollWidth <= node.clientWidth),
              true,
              `${locale}/${theme}/${width}/${name} should not overflow horizontally`,
            );
            if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
              await page.evaluate(async () => {
                await document.fonts.load('14px "Noto Sans Arabic"');
                await document.fonts.ready;
              });
              const file = path.resolve(
                `artifacts/localization/${locale}-${theme}-${width}-${name}.png`,
              );
              await page.screenshot({ path: file, fullPage: true });
            }
          }
        }
      }
    }

    // Check automatic reset and imported-config previews preserve locale behavior.
    await page.locator('nav button[data-page="config"]').click();
    await page.locator(".config-toolbar button:last-of-type").click();
    await page.locator('[data-setting-path="general.ui_language"] select').waitFor({ state: "detached" }).catch(() => {});
    await page.locator('nav button[data-page="general"]').click();
    await page.locator('[data-setting-path="general.ui_language"] select').waitFor();
    assert.equal(await page.locator('[data-setting-path="general.ui_language"] select').inputValue(), "auto");
    assert.equal(await page.locator("html").getAttribute("lang"), "ar", "reset to auto should use system locale");
    const imported = structuredClone(f.getConfig());
    imported.general.ui_language = "ar";
    await page.locator('nav button[data-page="config"]').click();
    await page.locator("#import-file").setInputFiles({
      name: "localized-config.json",
      mimeType: "application/json",
      buffer: Buffer.from(JSON.stringify(imported)),
    });
    await page.waitForFunction(() => document.documentElement.lang === "ar");

    // Capture actual localized interactions in addition to the page matrix.
    await page.locator('nav button[data-page="audio"]').click();
    const combo = page.locator('[data-setting-path="audio.preferred_device_id"] input[role="combobox"]');
    if (await combo.count()) await combo.focus();
    const modelCombo = page.locator('[data-setting-path="speech.single_model_filename"] input[role="combobox"]');
    if (await modelCombo.count()) {
      await modelCombo.fill("missing-saved-model.gguf");
      assert.match(await modelCombo.inputValue(), /\.gguf$/, "technical model identifiers should remain intact in Arabic UI");
      if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.resolve("artifacts/localization/ar-mixed-identifier-combo.png"), fullPage: true });
      }
      await page.keyboard.press("Escape");
    }
    await page.locator('nav button[data-page="models"]').click();
    await page.locator('input[aria-label="ابحث عن نموذج"]').fill("عربي");
    await page.locator(".model-result").first().waitFor();
    if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: path.resolve("artifacts/localization/ar-model-search.png"), fullPage: true });
    }
    await page.locator('nav button[data-page="audio"]').click();
    const help = page.locator(".help button").first();
    if (await help.count()) {
      assert((await help.getAttribute("title"))?.length, "help tooltip should have a localized title");
      await help.hover();
      await page.waitForTimeout(700);
      if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
        await page.evaluate(() => document.fonts.ready);
        await page.screenshot({ path: path.resolve("artifacts/localization/ar-help-tooltip.png"), fullPage: true });
      }
    }
    const general = page.locator('nav button[data-page="general"]');
    await general.click();
    await page.locator('[data-setting-path="general.ui_language"] select').selectOption("ar");
    await page.locator('[data-setting-path="general.ui_theme"] select').selectOption("dark");
    if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
      await page.evaluate(async () => {
        await document.fonts.load('14px "Noto Sans Arabic"');
        await document.fonts.ready;
      });
      await page.screenshot({ path: path.resolve("artifacts/localization/ar-interactions-savebar.png"), fullPage: true });
    }
    f.options.conflict = true;
    await page.locator("#savebar button[type=submit]").click();
    await page.locator(".notice").waitFor();
    const diagnostic = page.locator(".notice-detail");
    await diagnostic.locator('[dir="auto"]').getByText("config changed outside Settings").waitFor();
    await diagnostic.locator("summary").click();
    assert.equal(await diagnostic.getAttribute("open"), null, "diagnostic details should be collapsible");
    await diagnostic.locator("summary").click();
    if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: path.resolve("artifacts/localization/ar-error-details.png"), fullPage: true });
    }
    f.options.conflict = false;

    await page.setViewportSize({ width: 1280, height: 900 });
    await page.locator("html").evaluate((node) => (node.style.zoom = "2"));
    await page.locator('nav button[data-page="output"]').click();
    assert.equal(await page.locator("html").evaluate((node) => node.scrollWidth <= node.clientWidth), true,
      "200% zoom should not create horizontal page overflow");
    if (process.env.SETTINGS_VISUAL_REVIEW === "1") {
      await page.evaluate(() => document.fonts.ready);
      await page.screenshot({ path: path.resolve("artifacts/localization/ar-200-percent.png"), fullPage: true });
    }
    assert.equal(errors.length, 0, errors.join("\n"));
    console.log("PASS: Settings localization, locale persistence, Arabic search, RTL geometry, page coverage and responsive layouts");
  } finally {
    if (browser) await browser.close();
    if (f.server.listening) await new Promise((resolve) => f.server.close(resolve));
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

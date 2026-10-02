const assert = require("node:assert/strict");
const { chromium } = require("playwright");
const f = require("./settings-fixture.cjs");
const { server, token, models, actions, keyboardDiscovery, makeConfig } = f;
(async () => {
  let browser;
  try {
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({
      viewport: { width: 360, height: 820 },
    });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${address.port}/#token=${token}`);
    await page.getByRole("heading", { name: "General" }).waitFor();
    assert.equal(
      f.getKeyboardRequests(),
      1,
      "settings should call the authenticated keyboard language discovery endpoint",
    );
    await page.locator('nav button[data-page="audio"]').click();
    const mode = page.locator(
      '[data-setting-path="speech.selection_mode"] select',
    );
    assert.equal(await mode.inputValue(), "single_model");
    assert(
      await mode
        .locator("option")
        .allTextContents()
        .then((texts) => texts.includes("Use one model")),
    );
    const singleModel = page.locator(
      '[data-setting-path="speech.single_model_filename"] .combo-input',
    );
    assert.match(
      await singleModel.inputValue(),
      /Unavailable · missing-saved-model\.gguf/,
    );
    assert.equal(
      await page
        .locator(
          '[data-setting-path="speech.single_model_filename"] [data-model-test="true"]',
        )
        .isDisabled(),
      true,
    );

    await mode.selectOption("follow_keyboard");
    const draft = async () => {
      const active = await page
        .locator('nav button[aria-current="page"]')
        .getAttribute("data-page");
      await page.locator('nav button[data-page="config"]').click();
      const value = JSON.parse(await page.locator("#json").inputValue());
      await page.locator(`nav button[data-page="${active}"]`).click();
      return value;
    };
    let current = await draft();
    assert.equal(
      current.speech.language_models.en,
      null,
      "explicit None must not be replaced while entering follow mode",
    );
    assert.equal(
      current.speech.language_models.ar,
      "arabic-q8.gguf",
      "existing assignments must survive mode changes",
    );
    assert.equal(
      current.speech.language_models.es,
      "spanish-recommended-q4.gguf",
      "recommended compatible model should outrank Q8",
    );
    assert.equal(
      current.speech.language_models.it,
      "italian-q8.gguf",
      "Q8 should outrank smaller compatible models when none is recommended",
    );
    await page
      .getByRole("button", { name: /Refresh keyboard languages/ })
      .click();
    await page
      .locator('[data-setting-path="speech.language_models.it"]')
      .waitFor();
    assert.equal(
      f.getKeyboardRequests(),
      2,
      "explicit discovery refresh should call the keyboard language endpoint again",
    );
    assert.equal(
      (await draft()).speech.language_models.en,
      null,
      "discovery refresh should preserve explicit None",
    );

    const { recommendedModel } =
      await import("../web/settings/src/lib/models.ts");
    const tieWinner = recommendedModel(
      [
        {
          file: "b.gguf",
          quant: "Q8",
          recommended: false,
          size_mb: 100,
          installed: true,
          languages: ["Slovak (sk)"],
        },
        {
          file: "a.gguf",
          quant: "Q8",
          recommended: false,
          size_mb: 100,
          installed: true,
          languages: ["Slovak (sk)"],
        },
        {
          file: "large.gguf",
          quant: "Q8",
          recommended: false,
          size_mb: 110,
          installed: true,
          languages: ["Slovak (sk)"],
        },
      ],
      { id: "sk" },
    ).file;
    assert.equal(
      tieWinner,
      "a.gguf",
      "size and filename provide deterministic ties",
    );
    keyboardDiscovery.languages = keyboardDiscovery.languages.filter(
      (language) => language.id !== "ar",
    );
    keyboardDiscovery.languages.push(
      { id: "de", name: "German" },
      { id: "xkb:unknown:variant", name: "Unknown layout" },
    );
    await page
      .getByRole("button", { name: "Refresh keyboard languages" })
      .click();
    await page
      .locator('[data-setting-path="speech.language_models.de"]')
      .waitFor();
    current = await draft();
    assert.equal(
      current.speech.language_models.de,
      null,
      "unmatched languages receive None",
    );
    assert.equal(
      current.speech.language_models["xkb:unknown:variant"],
      null,
      "unidentified layouts receive None",
    );
    assert.equal(
      current.speech.language_models.ar,
      "arabic-q8.gguf",
      "removed languages retain assignments",
    );
    keyboardDiscovery.languages.push({ id: "ar", name: "Arabic" });
    await page
      .getByRole("button", { name: "Refresh keyboard languages" })
      .click();
    await page
      .locator('[data-setting-path="speech.language_models.ar"] .combo-input')
      .waitFor();

    const italian = page.locator(
      '[data-setting-path="speech.language_models.it"] .combo-input',
    );
    await italian.click();
    await italian.fill("Italian Q4");
    await italian.press("ArrowDown");
    await page.keyboard.press("Enter");
    current = await draft();
    assert.equal(
      current.speech.language_models.it,
      "italian-q4.gguf",
      "keyboard navigation should select the highlighted installed model",
    );

    const english = page.locator(
      '[data-setting-path="speech.language_models.en"] .combo-input',
    );
    await english.click();
    await english.fill("an-unlisted-custom-filename.gguf");
    await english.press("Tab");
    current = await draft();
    assert.equal(
      current.speech.language_models.en,
      null,
      "typing and blurring an arbitrary filename must not save it",
    );

    const arabic = page.locator(
      '[data-setting-path="speech.language_models.ar"] .combo-input',
    );
    await arabic.click();
    await arabic.fill("English Q8");
    const englishQ8 = page.getByRole("option", { name: /English Q8/ });
    assert.equal(
      await englishQ8.count(),
      1,
      "search should find installed models by label and metadata",
    );
    await englishQ8.click();
    current = await draft();
    assert.equal(
      current.speech.language_models.ar,
      "english-q8.gguf",
      "mouse selection should update the draft assignment",
    );
    await arabic.click();
    await arabic.fill("English New");
    assert.equal(
      await page.getByRole("option", { name: /English New Q8/ }).count(),
      0,
      "uninstalled models must not appear in assignment selectors",
    );
    await arabic.press("Escape");

    const removed = page.locator(".removed-keyboard-languages");
    assert.equal(
      await removed.locator("summary").innerText(),
      "Other languages (1)",
    );
    await removed.locator("summary").click();
    assert.match(
      await removed.locator(".combo-input").inputValue(),
      /Unavailable · missing-french\.gguf/,
    );

    await page.locator('nav button[data-page="models"]').click();
    const catalogSearch = page.locator("#model-search");

    await page.getByRole("button", { name: "Refresh model catalog" }).click();
    await page.locator("#model-results .model-result").first().waitFor();
    assert.equal(
      (await draft()).speech.language_models.en,
      null,
      "catalog refresh should preserve explicit None",
    );
    await catalogSearch.fill("Arabic Q8");
    const savedArabicRemove = page.getByRole("button", {
      name: "Remove Arabic Q8 Q8",
    });
    assert.equal(
      await savedArabicRemove.isDisabled(),
      true,
      "saved assignment must guard removal even after the draft changed",
    );
    assert.match(await savedArabicRemove.getAttribute("title"), /Arabic/);

    await catalogSearch.fill("English New Q8");
    await page.getByRole("button", { name: "Download English New Q8" }).click();
    await page.waitForFunction(
      () => document.querySelector(".download-progress")?.value === 50,
    );
    assert(
      await page
        .getByRole("button", { name: "Downloading English New Q8" })
        .isDisabled(),
    );
    current = await draft();
    assert.equal(
      current.speech.language_models.en,
      null,
      "installation and event refresh must preserve explicit None",
    );
    const removeNewModel = page.getByRole("button", {
      name: /Remove English New Q8/,
    });
    await removeNewModel.waitFor({ timeout: 8000 });
    assert.equal(await removeNewModel.isEnabled(), true);

    await page.locator('nav button[data-page="audio"]').click();
    await page.locator("#savebar button[type=submit]").click();
    await page.getByText("Saved and applied.").waitFor();
    assert.equal(
      f.getConfig().speech.language_models.en,
      null,
      "Save should preserve explicit None",
    );
    assert.equal(f.getConfig().speech.language_models.ar, "english-q8.gguf");

    await page.locator('nav button[data-page="config"]').click();
    await page.getByRole("button", { name: "Reset preview" }).click();
    await page.waitForFunction(
      () =>
        JSON.parse(document.querySelector("#json").value).speech
          .selection_mode === "single_model",
    );
    assert.equal(
      (await draft()).speech.selection_mode,
      "single_model",
      "Reset should preview defaults",
    );
    await page.locator("#savebar button[type=submit]").click();
    await page.getByText("Saved and applied.").waitFor();
    const importConfig = makeConfig();
    importConfig.speech.selection_mode = "follow_keyboard";
    importConfig.speech.language_models = { en: null, ar: "arabic-q8.gguf" };
    await page.locator("#import-file").setInputFiles({
      name: "model-selection-import.json",
      mimeType: "application/json",
      buffer: Buffer.from(JSON.stringify(importConfig)),
    });
    await page.getByText("Import previewed. Save to write it.").waitFor();
    current = await draft();
    assert.equal(current.speech.selection_mode, "follow_keyboard");
    assert.equal(
      current.speech.language_models.en,
      null,
      "Import preview should preserve explicit None",
    );

    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth,
    );
    assert.equal(
      overflow,
      false,
      "narrow settings layout should not overflow horizontally",
    );
    assert.deepEqual(
      pageErrors,
      [],
      `browser errors: ${pageErrors.join("; ")}`,
    );
    assert(
      actions.some(
        (entry) =>
          entry.action === "download_model" &&
          entry.filename === "english-new-q8.gguf",
      ),
    );
    keyboardDiscovery.available = false;
    keyboardDiscovery.message =
      "Keyboard language detection unavailable on this desktop";
    await page.reload();
    await page.locator('[data-setting-path="general.enabled"]').waitFor();
    await page.locator('button[data-page="audio"]').click();
    await page
      .locator('[data-setting-path="speech.selection_mode"] select')
      .selectOption("single_model");
    await page.waitForFunction(
      () =>
        document.querySelector(
          '[data-setting-path="speech.selection_mode"] option[value="follow_keyboard"]',
        )?.disabled === true,
    );
    assert(
      await page
        .getByRole("status")
        .filter({ hasText: "Keyboard detection unavailable" })
        .isVisible(),
    );
    if (process.env.SETTINGS_TEST_SCREENSHOT) {
      keyboardDiscovery.available = true;
      keyboardDiscovery.message = "";
      keyboardDiscovery.languages = [
        { id: "ar", name: "Arabic" },
        { id: "en", name: "English" },
      ];
      f.setConfig(makeConfig());
      f.getConfig().general.ui_theme = "dark";
      f.getConfig().speech.selection_mode = "follow_keyboard";
      f.getConfig().speech.inference_device = "gpu";
      models.find((model) => model.file === "arabic-q8.gguf").family =
        "Lemura Arabic ASR Lite";
      models.find((model) => model.file === "arabic-q8.gguf").quant = "q8_0";
      models.find((model) => model.file === "english-q8.gguf").family =
        "tdt-ctc110m";
      models.find((model) => model.file === "english-q8.gguf").quant = "q8_0";
      f.getConfig().speech.language_models = {
        ar: "arabic-q8.gguf",
        en: "english-q8.gguf",
      };
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.reload();
      await page.locator('[data-setting-path="general.enabled"]').waitFor();
      await page.locator('nav button[data-page="audio"]').click();
      await page
        .locator("main")
        .screenshot({ path: process.env.SETTINGS_TEST_SCREENSHOT });
    }
    console.log(
      "PASS settings model selection: discovery, ranking, combos, None preservation, import/reset/save, guarded removal, and narrow layout",
    );
  } finally {
    await browser?.close();
    f.closeEvents();
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});

#!/usr/bin/env node
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const { chromium } = require("playwright");

const root = path.resolve(__dirname, "..");
const webRoot = path.join(root, "web", "settings");
const token = "settings-model-selection-test";
const keyboardDiscovery = {
  available: true,
  message: "",
  languages: [
    { id: "en", name: "English" },
    { id: "ar", name: "Arabic" },
    { id: "es", name: "Spanish" },
    { id: "it", name: "Italian" },
  ],
};
const makeConfig = () => ({
  schema_version: 9,
  general: {
    enabled: true,
    recording_mode: "hold",
    record_hotkey: "CapsLock+S",
    toggle_delivery_hotkey: "CapsLock+D",
    cancel_hotkey: "CapsLock+A",
    toggle_cleanup_hotkey: "None",
    linux_hotkey_backend: "auto",
    capslock_behavior: "preserve_tap",
    start_at_login: false,
    ui_theme: "light",
  },
  audio: { preferred_device_id: "", gain: 1 },
  speech: {
    inference_device: "auto",
    runtime_dir: "runtime",
    model_dir: "models",
    selection_mode: "single_model",
    single_model_filename: "missing-saved-model.gguf",
    language_models: {
      en: null,
      ar: "arabic-q8.gguf",
      fr: "missing-french.gguf",
    },
    idle_worker_timeout_secs: 180,
    worker_shutdown_grace_ms: 2000,
  },
  output: {
    delivery_mode: "smart_paste",
    enabled_delivery_modes: ["smart_paste", "type"],
    linux_automation_backend: "auto",
    linux_delivery_cycle: [],
    app_overrides: [],
    paced_typing_enabled: true,
    typing_speed_wpm: 80,
    trailing_space: true,
    remove_punctuation: false,
    lowercase: false,
  },
  cleanup: {
    enabled: false,
    provider: "open_ai_compatible",
    prompt: "Keep the transcript clear.",
    timeout_ms: 30000,
    max_output_tokens: 1024,
    openai_compatible: { base_url: "https://example.test/v1", model: "test", reasoning_effort: "low" },
    chatgpt: { model: "test", reasoning_effort: "low" },
    screenshot: { enabled: false, scope: "active_window", excluded_apps: [], max_edge_pixels: 1600, jpeg_quality: 85 },
  },
  diagnostics: { log_level: "normal", diagnostic_overlay: false, log_transcripts: false },
});
const initialConfig = makeConfig();
const defaultConfig = makeConfig();
defaultConfig.speech.single_model_filename = null;
defaultConfig.speech.language_models = {};
let serverConfig = structuredClone(initialConfig);
let configHash = 1;
let keyboardRequests = 0;
let eventSeq = 0;
const actions = [];
const models = [
  { family: "English Q4", quant: "Q4", file: "english-q4.gguf", languages: ["English (en)"], size_mb: 100, installed: true, recommended: false },
  { family: "English Q8", quant: "Q8", file: "english-q8.gguf", languages: ["English (en)"], size_mb: 180, installed: true, recommended: false },
  { family: "Arabic Q8", quant: "Q8", file: "arabic-q8.gguf", languages: ["Arabic (ar)"], size_mb: 190, installed: true, recommended: true },
  { family: "Spanish Q4", quant: "Q4", file: "spanish-recommended-q4.gguf", languages: ["Spanish (es)"], size_mb: 120, installed: true, recommended: true },
  { family: "Spanish Q8", quant: "Q8", file: "spanish-q8.gguf", languages: ["Spanish (es)"], size_mb: 200, installed: true, recommended: false },
  { family: "Italian Q4", quant: "Q4", file: "italian-q4.gguf", languages: ["Italian (it)"], size_mb: 110, installed: true, recommended: false },
  { family: "Italian Q8", quant: "Q8", file: "italian-q8.gguf", languages: ["Italian (it)"], size_mb: 220, installed: true, recommended: false },
  { family: "English New Q8", quant: "Q8", file: "english-new-q8.gguf", languages: ["English (en)"], size_mb: 230, installed: false, recommended: true, download_url: "https://models.example.test/en-new" },
];
const eventWaiters = new Set();
const queuedEvents = [];
const sendJson = (res, status, value) => {
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" });
  res.end(JSON.stringify(value));
};
const clone = (value) => structuredClone(value);
const readBody = (req) => new Promise((resolve, reject) => {
  let text = "";
  req.setEncoding("utf8");
  req.on("data", (chunk) => { text += chunk; });
  req.on("end", () => resolve(text));
  req.on("error", reject);
});
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  if (url.pathname.startsWith("/api/") && req.headers["x-simple-stt-token"] !== token)
    return sendJson(res, 401, { error: "Missing test token" });
  try {
    if (url.pathname === "/api/state" && req.method === "GET")
      return sendJson(res, 200, {
        config: clone(serverConfig), config_hash: `hash-${configHash}`, platform: "windows",
        service_online: true, microphones: [], models: clone(models), cleanup: { history: [] },
        shortcut_state: {}, resolved_runtime_dir: "runtime", resolved_model_dir: "models",
      });
    if (url.pathname === "/api/keyboard-languages" && req.method === "GET") {
      keyboardRequests += 1;
      return sendJson(res, 200, clone(keyboardDiscovery));
    }
    if (url.pathname === "/api/defaults" && req.method === "GET")
      return sendJson(res, 200, { config: clone(defaultConfig) });
    if (url.pathname === "/api/save" && req.method === "POST") {
      const body = JSON.parse(await readBody(req));
      serverConfig = body.config;
      configHash += 1;
      return sendJson(res, 200, { config: clone(serverConfig), config_hash: `hash-${configHash}`, reloaded: true });
    }
    if (url.pathname === "/api/normalize" && req.method === "POST")
      return sendJson(res, 200, { config: JSON.parse(await readBody(req)) });
    if (url.pathname === "/api/action" && req.method === "POST") {
      const body = JSON.parse(await readBody(req));
      actions.push(body);
      if (body.action === "download_model") {
        const model = models.find((entry) => entry.file === body.filename);
        assert(model, `download requested for unknown model ${body.filename}`);
        const progress = { seq: ++eventSeq, kind: "model_download_progress", values: { filename: body.filename, downloaded: "50", total: "100" } };
        queuedEvents.push(progress);
        for (const waiter of [...eventWaiters]) {
          eventWaiters.delete(waiter);
          clearTimeout(waiter.timeout);
          queuedEvents.splice(queuedEvents.indexOf(progress), 1);
          waiter.res.end(`data: ${JSON.stringify({ events: [progress] })}\n\n`);
        }
        await new Promise((resolve) => setTimeout(resolve, 800));
        model.installed = true;
        const event = { seq: ++eventSeq, kind: "model_download_complete", values: { filename: body.filename } };
        queuedEvents.push(event);
        for (const waiter of [...eventWaiters]) {
          eventWaiters.delete(waiter);
          clearTimeout(waiter.timeout);
          queuedEvents.splice(queuedEvents.indexOf(event), 1);
          waiter.res.end(`data: ${JSON.stringify({ events: [event] })}\n\n`);
        }
      }
      return sendJson(res, 200, { message: `${body.action} completed` });
    }
    if (url.pathname === "/api/events" && req.method === "GET") {
      res.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-store", Connection: "keep-alive" });
      if (queuedEvents.length) {
        const events = queuedEvents.splice(0, queuedEvents.length);
        return res.end(`data: ${JSON.stringify({ events })}\n\n`);
      }
      const waiter = { res, timeout: setTimeout(() => { eventWaiters.delete(waiter); res.end(); }, 30000) };
      eventWaiters.add(waiter);
      req.on("close", () => { eventWaiters.delete(waiter); clearTimeout(waiter.timeout); });
      return;
    }
    if (url.pathname === "/" || url.pathname.endsWith(".html") || url.pathname.endsWith(".js") || url.pathname.endsWith(".css")) {
      const relative = url.pathname === "/" ? "index.html" : url.pathname.slice(1);
      const filePath = path.resolve(webRoot, relative);
      if (!filePath.startsWith(webRoot + path.sep)) return sendJson(res, 403, { error: "Forbidden" });
      const ext = path.extname(filePath);
      const types = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8" };
      res.writeHead(200, { "Content-Type": types[ext] || "application/octet-stream" });
      return fs.createReadStream(filePath).pipe(res);
    }
    return sendJson(res, 404, { error: "Not found" });
  } catch (error) {
    return sendJson(res, 500, { error: error.message });
  }
});

(async () => {
  let browser;
  try {
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address();
    browser = await chromium.launch({ headless: true });
    const page = await browser.newPage({ viewport: { width: 360, height: 820 } });
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${address.port}/#token=${token}`);
    await page.getByRole("heading", { name: "General" }).waitFor();
    assert.equal(keyboardRequests, 1, "settings should call the authenticated keyboard language discovery endpoint");
    await page.locator('nav button[data-page="audio"]').click();
    const mode = page.locator('[data-setting-path="speech.selection_mode"] select');
    assert.equal(await mode.inputValue(), "single_model");
    assert(await mode.locator("option").allTextContents().then((texts) => texts.includes("Use one model")));
    const singleModel = page.locator('[data-setting-path="speech.single_model_filename"] .combo-input');
    assert.match(await singleModel.inputValue(), /Unavailable · missing-saved-model\.gguf/);
    assert.equal(await page.locator('[data-setting-path="speech.single_model_filename"] [data-model-test="true"]').isDisabled(), true);

    await mode.selectOption("follow_keyboard");
    const draft = async () => JSON.parse(await page.locator("#json").inputValue());
    let current = await draft();
    assert.equal(current.speech.language_models.en, null, "explicit None must not be replaced while entering follow mode");
    assert.equal(current.speech.language_models.ar, "arabic-q8.gguf", "existing assignments must survive mode changes");
    assert.equal(current.speech.language_models.es, "spanish-recommended-q4.gguf", "recommended compatible model should outrank Q8");
    assert.equal(current.speech.language_models.it, "italian-q8.gguf", "Q8 should outrank smaller compatible models when none is recommended");
    await page.getByRole("button", { name: /Refresh keyboard languages/ }).click();
    await page.waitForFunction(() => JSON.parse(document.querySelector("#json").value).speech.language_models.it === "italian-q8.gguf");
    assert.equal(keyboardRequests, 2, "explicit discovery refresh should call the keyboard language endpoint again");
    assert.equal((await draft()).speech.language_models.en, null, "discovery refresh should preserve explicit None");

    const tieWinner = await page.evaluate(() => {
      const original = state.models;
      try {
        state.models = [
          { file: "b.gguf", quant: "Q8", recommended: false, size_mb: 100, installed: true, languages: ["Slovak (sk)"] },
          { file: "a.gguf", quant: "Q8", recommended: false, size_mb: 100, installed: true, languages: ["Slovak (sk)"] },
          { file: "large.gguf", quant: "Q8", recommended: false, size_mb: 110, installed: true, languages: ["Slovak (sk)"] },
        ];
        return recommendedModel({ id: "sk" }).file;
      } finally { state.models = original; }
    });
    assert.equal(tieWinner, "a.gguf", "size and filename provide deterministic ties");
    keyboardDiscovery.languages = keyboardDiscovery.languages.filter((language) => language.id !== "ar");
    keyboardDiscovery.languages.push({ id: "de", name: "German" }, { id: "xkb:unknown:variant", name: "Unknown layout" });
    await page.getByRole("button", { name: "Refresh keyboard languages" }).click();
    await page.waitForFunction(() => Object.hasOwn(config.speech.language_models, "de"));
    current = await draft();
    assert.equal(current.speech.language_models.de, null, "unmatched languages receive None");
    assert.equal(current.speech.language_models["xkb:unknown:variant"], null, "unidentified layouts receive None");
    assert.equal(current.speech.language_models.ar, "arabic-q8.gguf", "removed languages retain assignments");
    keyboardDiscovery.languages.push({ id: "ar", name: "Arabic" });
    await page.getByRole("button", { name: "Refresh keyboard languages" }).click();
    await page.locator('[data-setting-path="speech.language_models.ar"] .combo-input').waitFor();

    const italian = page.locator('[data-setting-path="speech.language_models.it"] .combo-input');
    await italian.click();
    await italian.fill("Italian Q4");
    await italian.press("ArrowDown");
    await page.keyboard.press("Enter");
    current = await draft();
    assert.equal(current.speech.language_models.it, "italian-q4.gguf", "keyboard navigation should select the highlighted installed model");

    const english = page.locator('[data-setting-path="speech.language_models.en"] .combo-input');
    await english.click();
    await english.fill("an-unlisted-custom-filename.gguf");
    await english.press("Tab");
    current = await draft();
    assert.equal(current.speech.language_models.en, null, "typing and blurring an arbitrary filename must not save it");

    const arabic = page.locator('[data-setting-path="speech.language_models.ar"] .combo-input');
    await arabic.click();
    await arabic.fill("English Q8");
    const englishQ8 = page.getByRole("option", { name: /English Q8/ });
    assert.equal(await englishQ8.count(), 1, "search should find installed models by label and metadata");
    await englishQ8.click();
    current = await draft();
    assert.equal(current.speech.language_models.ar, "english-q8.gguf", "mouse selection should update the draft assignment");
    await arabic.click();
    await arabic.fill("English New");
    assert.equal(await page.getByRole("option", { name: /English New Q8/ }).count(), 0, "uninstalled models must not appear in assignment selectors");
    await arabic.press("Escape");

    const removed = page.locator(".removed-keyboard-languages");
    assert.equal(await removed.locator("summary").innerText(), "Other languages (1)");
    await removed.locator("summary").click();
    assert.match(await removed.locator(".combo-input").inputValue(), /Unavailable · missing-french\.gguf/);

    await page.locator('nav button[data-page="models"]').click();
    const catalogSearch = page.locator("#model-search");
    const previousModelRow = await page.locator("#model-results .model-result").first().elementHandle();
    await page.getByRole("button", { name: "Refresh model catalog" }).click();
    await page.waitForFunction((row) => !row.isConnected, previousModelRow);
    assert.equal((await draft()).speech.language_models.en, null, "catalog refresh should preserve explicit None");
    await catalogSearch.fill("Arabic Q8");
    const savedArabicRemove = page.getByRole("button", { name: "Remove Arabic Q8 Q8" });
    assert.equal(await savedArabicRemove.isDisabled(), true, "saved assignment must guard removal even after the draft changed");
    assert.match(await savedArabicRemove.getAttribute("title"), /Arabic/);

    await catalogSearch.fill("English New Q8");
    await page.getByRole("button", { name: "Download English New Q8" }).click();
    await page.waitForFunction(() => document.querySelector(".download-progress")?.value === 50);
    assert(await page.getByRole("button", { name: "Downloading English New Q8" }).isDisabled());
    current = await draft();
    assert.equal(current.speech.language_models.en, null, "installation and event refresh must preserve explicit None");
    const removeNewModel = page.getByRole("button", { name: /Remove English New Q8/ });
    await removeNewModel.waitFor({ timeout: 8000 });
    assert.equal(await removeNewModel.isEnabled(), true);

    await page.locator('nav button[data-page="audio"]').click();
    await page.locator("#savebar button[type=submit]").click();
    await page.getByText("Saved and applied.").waitFor();
    assert.equal(serverConfig.speech.language_models.en, null, "Save should preserve explicit None");
    assert.equal(serverConfig.speech.language_models.ar, "english-q8.gguf");

    await page.locator('nav button[data-page="config"]').click();
    await page.getByRole("button", { name: "Reset preview" }).click();
    await page.waitForFunction(() => JSON.parse(document.querySelector("#json").value).speech.selection_mode === "single_model");
    assert.equal((await draft()).speech.selection_mode, "single_model", "Reset should preview defaults");
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
    assert.equal(current.speech.language_models.en, null, "Import preview should preserve explicit None");

    const overflow = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
    assert.equal(overflow, false, "narrow settings layout should not overflow horizontally");
    assert.deepEqual(pageErrors, [], `browser errors: ${pageErrors.join("; ")}`);
    assert(actions.some((entry) => entry.action === "download_model" && entry.filename === "english-new-q8.gguf"));
    keyboardDiscovery.available = false;
    keyboardDiscovery.message = "Keyboard language detection unavailable on this desktop";
    await page.reload();
    await page.waitForFunction(() => state?.keyboard_languages?.available === false);
    await page.locator('button[data-page="audio"]').click();
    await page.locator('[data-setting-path="speech.selection_mode"] select').selectOption("single_model");
    await page.waitForFunction(() => document.querySelector('[data-setting-path="speech.selection_mode"] option[value="follow_keyboard"]')?.disabled === true);
    assert(await page.getByRole("status").filter({ hasText: "Keyboard detection unavailable" }).isVisible());
    if (process.env.SETTINGS_TEST_SCREENSHOT) {
      keyboardDiscovery.available = true;
      keyboardDiscovery.message = "";
      keyboardDiscovery.languages = [{ id: "ar", name: "Arabic" }, { id: "en", name: "English" }];
      serverConfig = makeConfig();
      serverConfig.general.ui_theme = "dark";
      serverConfig.speech.selection_mode = "follow_keyboard";
      serverConfig.speech.inference_device = "gpu";
      models.find(model => model.file === "arabic-q8.gguf").family = "Lemura Arabic ASR Lite";
      models.find(model => model.file === "arabic-q8.gguf").quant = "q8_0";
      models.find(model => model.file === "english-q8.gguf").family = "tdt-ctc110m";
      models.find(model => model.file === "english-q8.gguf").quant = "q8_0";
      serverConfig.speech.language_models = { ar: "arabic-q8.gguf", en: "english-q8.gguf" };
      await page.setViewportSize({ width: 1280, height: 900 });
      await page.reload();
      await page.waitForFunction(() => state?.keyboard_languages?.available && !dirty);
      await page.locator('nav button[data-page="audio"]').click();
      await page.locator('#audio-fields > .setting-group').nth(1).screenshot({ path: process.env.SETTINGS_TEST_SCREENSHOT });
    }
    console.log("PASS settings model selection: discovery, ranking, combos, None preservation, import/reset/save, guarded removal, and narrow layout");
  } finally {
    await browser?.close();
    for (const waiter of eventWaiters) { clearTimeout(waiter.timeout); waiter.res.end(); }
    await new Promise((resolve) => server.close(resolve));
  }
})().catch((error) => {
  console.error(error.stack || error);
  process.exitCode = 1;
});

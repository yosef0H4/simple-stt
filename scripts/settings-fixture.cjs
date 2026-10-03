#!/usr/bin/env node
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const webRoot = path.resolve(
  process.env.SETTINGS_WEB_ROOT || path.join(root, "web", "settings", "dist"),
);
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
    retry_delivery_hotkey: "Ctrl+Alt+V",
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
    preserve_clipboard: true,
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
    openai_compatible: {
      base_url: "https://example.test/v1",
      model: "test",
      reasoning_effort: "low",
    },
    chatgpt: { model: "test", reasoning_effort: "low" },
    screenshot: {
      enabled: false,
      scope: "active_window",
      excluded_apps: [],
      max_edge_pixels: 1600,
      jpeg_quality: 85,
    },
  },
  diagnostics: {
    log_level: "normal",
    diagnostic_overlay: false,
    log_transcripts: false,
  },
});
const initialConfig = makeConfig();
const defaultConfig = makeConfig();
defaultConfig.speech.single_model_filename = null;
defaultConfig.speech.language_models = {};
let serverConfig = structuredClone(initialConfig);
let configHash = 1;
const options = {
  platform: "windows",
  online: true,
  conflict: false,
  connected: false,
  keySaved: false,
  hotkey: "Ctrl+Alt+R",
};
let keyboardRequests = 0;
let eventSeq = 0;
const actions = [];
const models = [
  {
    family: "English Q4",
    quant: "Q4",
    file: "english-q4.gguf",
    languages: ["English (en)"],
    size_mb: 100,
    installed: true,
    recommended: false,
  },
  {
    family: "English Q8",
    quant: "Q8",
    file: "english-q8.gguf",
    languages: ["English (en)"],
    size_mb: 180,
    installed: true,
    recommended: false,
  },
  {
    family: "Arabic Q8",
    quant: "Q8",
    file: "arabic-q8.gguf",
    languages: ["Arabic (ar)"],
    size_mb: 190,
    installed: true,
    recommended: true,
  },
  {
    family: "Spanish Q4",
    quant: "Q4",
    file: "spanish-recommended-q4.gguf",
    languages: ["Spanish (es)"],
    size_mb: 120,
    installed: true,
    recommended: true,
  },
  {
    family: "Spanish Q8",
    quant: "Q8",
    file: "spanish-q8.gguf",
    languages: ["Spanish (es)"],
    size_mb: 200,
    installed: true,
    recommended: false,
  },
  {
    family: "Italian Q4",
    quant: "Q4",
    file: "italian-q4.gguf",
    languages: ["Italian (it)"],
    size_mb: 110,
    installed: true,
    recommended: false,
  },
  {
    family: "Italian Q8",
    quant: "Q8",
    file: "italian-q8.gguf",
    languages: ["Italian (it)"],
    size_mb: 220,
    installed: true,
    recommended: false,
  },
  {
    family: "English New Q8",
    quant: "Q8",
    file: "english-new-q8.gguf",
    languages: ["English (en)"],
    size_mb: 230,
    installed: false,
    recommended: true,
    download_url: "https://models.example.test/en-new",
  },
];
const eventWaiters = new Set();
const queuedEvents = [];
const sendJson = (res, status, value) => {
  res.writeHead(status, {
    "Content-Type": "application/json; charset=utf-8",
    "Cache-Control": "no-store",
  });
  res.end(JSON.stringify(value));
};
const clone = (value) => structuredClone(value);
const readBody = (req) =>
  new Promise((resolve, reject) => {
    let text = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      text += chunk;
    });
    req.on("end", () => resolve(text));
    req.on("error", reject);
  });
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  if (
    url.pathname.startsWith("/api/") &&
    req.headers["x-simple-stt-token"] !== token
  )
    return sendJson(res, 401, { error: "Missing test token" });
  try {
    if (url.pathname === "/api/state" && req.method === "GET")
      return sendJson(res, 200, {
        config: clone(serverConfig),
        config_hash: `hash-${configHash}`,
        platform: options.platform,
        service_online: options.online,
        microphones: [],
        models: clone(models),
        cleanup: {
          history: clone(options.history || []),
          compatible_key_saved: options.keySaved,
          chatgpt_connected: options.connected,
        },
        linux_automation: {
          session: "Wayland",
          desktop: "KDE",
          distro: "Fedora",
          distro_id: "fedora",
          native: true,
          wl_clipboard: true,
          recommended: "native",
        },
        linux_hotkeys: {
          requested: "auto",
          active: "portal",
          status: "registered",
        },
        shortcut_state: {},
        resolved_runtime_dir: "runtime",
        resolved_model_dir: "models",
      });
    if (url.pathname === "/api/keyboard-languages" && req.method === "GET") {
      keyboardRequests += 1;
      return sendJson(res, 200, clone(keyboardDiscovery));
    }
    if (url.pathname === "/api/defaults" && req.method === "GET")
      return sendJson(res, 200, { config: clone(defaultConfig) });
    if (url.pathname === "/api/save" && req.method === "POST") {
      const body = JSON.parse(await readBody(req));
      if (options.conflict)
        return sendJson(res, 409, { error: "config changed outside Settings" });
      serverConfig = body.config;
      configHash += 1;
      return sendJson(res, 200, {
        config: clone(serverConfig),
        config_hash: `hash-${configHash}`,
        reloaded: options.online,
      });
    }
    if (url.pathname === "/api/normalize" && req.method === "POST")
      return sendJson(res, 200, { config: JSON.parse(await readBody(req)) });
    if (url.pathname === "/api/cleanup-action") {
      if (options.cleanupDelay) {
        options.pendingCleanup = (options.pendingCleanup || 0) + 1;
        await new Promise((resolve) => {
          let finished = false;
          const done = () => {
            if (finished) return;
            finished = true;
            clearTimeout(timer);
            options.pendingCleanup--;
            resolve();
          };
          const timer = setTimeout(done, options.cleanupDelay);
          res.once("close", done);
        });
        if (res.destroyed) return;
      }
      const body = JSON.parse(await readBody(req));
      actions.push(body);
      if (body.action === "clear_history") options.history = [];
      if (body.action === "save_api_key") options.keySaved = true;
      if (body.action === "delete_api_key") options.keySaved = false;
      if (
        body.action === "chatgpt_login_browser" ||
        body.action === "chatgpt_login_code"
      ) {
        options.connected = true;
        return sendJson(res, 200, {
          url: "about:blank",
          code: "TEST-CODE",
          message: "Connected",
        });
      }
      if (body.action === "chatgpt_logout") options.connected = false;
      return sendJson(res, 200, {
        message: "Cleanup action completed",
        models: [{ id: "provider-model" }],
        result: { text: "مرحبا · Hello Jayson" },
      });
    }
    if (url.pathname === "/api/hotkey-capture")
      return sendJson(res, 200, { hotkey: options.hotkey });
    if (url.pathname === "/api/platform-action") {
      const body = JSON.parse(await readBody(req));
      actions.push(body);
      return sendJson(res, 200, {
        app_id: "test-app",
        message: "Request opened",
      });
    }
    if (url.pathname === "/api/close") return sendJson(res, 200, { ok: true });
    if (url.pathname === "/api/action" && req.method === "POST") {
      const body = JSON.parse(await readBody(req));
      actions.push(body);
      if (body.action === "download_model") {
        const model = models.find((entry) => entry.file === body.filename);
        assert(model, `download requested for unknown model ${body.filename}`);
        const progress = {
          seq: ++eventSeq,
          kind: "model_download_progress",
          values: { filename: body.filename, downloaded: "50", total: "100" },
        };
        queuedEvents.push(progress);
        for (const waiter of [...eventWaiters]) {
          eventWaiters.delete(waiter);
          clearTimeout(waiter.timeout);
          queuedEvents.splice(queuedEvents.indexOf(progress), 1);
          waiter.res.end(`data: ${JSON.stringify({ events: [progress] })}\n\n`);
        }
        await new Promise((resolve) => setTimeout(resolve, 800));
        model.installed = true;
        const event = {
          seq: ++eventSeq,
          kind: "model_download_complete",
          values: { filename: body.filename },
        };
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
      res.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-store",
        Connection: "keep-alive",
      });
      if (queuedEvents.length) {
        const events = queuedEvents.splice(0, queuedEvents.length);
        return res.end(`data: ${JSON.stringify({ events })}\n\n`);
      }
      const waiter = {
        res,
        timeout: setTimeout(() => {
          eventWaiters.delete(waiter);
          res.end();
        }, 30000),
      };
      eventWaiters.add(waiter);
      req.on("close", () => {
        eventWaiters.delete(waiter);
        clearTimeout(waiter.timeout);
      });
      return;
    }
    if (
      url.pathname === "/" ||
      url.pathname.endsWith(".html") ||
      url.pathname.endsWith(".js") ||
      url.pathname.endsWith(".css")
    ) {
      const relative =
        url.pathname === "/" ? "index.html" : url.pathname.slice(1);
      const filePath = path.resolve(webRoot, relative);
      if (!filePath.startsWith(webRoot + path.sep))
        return sendJson(res, 403, { error: "Forbidden" });
      const ext = path.extname(filePath);
      const types = {
        ".html": "text/html; charset=utf-8",
        ".js": "text/javascript; charset=utf-8",
        ".css": "text/css; charset=utf-8",
      };
      res.writeHead(200, {
        "Content-Type": types[ext] || "application/octet-stream",
      });
      return fs.createReadStream(filePath).pipe(res);
    }
    return sendJson(res, 404, { error: "Not found" });
  } catch (error) {
    return sendJson(res, 500, { error: error.message });
  }
});

function publishEvent(kind, values = {}) {
  const event = { seq: ++eventSeq, kind, values };
  queuedEvents.push(event);
  const waiter = [...eventWaiters][0];
  if (waiter) {
    eventWaiters.delete(waiter);
    clearTimeout(waiter.timeout);
    waiter.res.end(
      `data: ${JSON.stringify({ events: queuedEvents.splice(0) })}\n\n`,
    );
  }
}
module.exports = {
  publishEvent,
  options,
  eventWaiters,
  server,
  models,
  actions,
  keyboardDiscovery,
  makeConfig,
  setConfig(value) {
    serverConfig = value;
  },
  getConfig() {
    return serverConfig;
  },
  getKeyboardRequests() {
    return keyboardRequests;
  },
  closeEvents() {
    for (const waiter of eventWaiters) {
      clearTimeout(waiter.timeout);
      waiter.res.end();
    }
  },
  token,
};

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
    show_shortcuts_hotkey: "CapsLock+H",
    show_shortcuts_mode: "hold",
    shortcut_reminders: true,
    retry_delivery_hotkey: "Ctrl+Alt+V",
    toggle_cleanup_hotkey: "None",
    linux_hotkey_backend: "auto",
    capslock_behavior: "preserve_tap",
    start_at_login: false,
    ui_theme: "light",
    ui_language: "auto",
  },
  audio: { preferred_device_id: "", gain: 1, stop_grace_ms: 250 },
  speech: {
    provider: "local",
    remote_openai_compatible: {
      base_url: "",
      model: "",
      fallback_to_local: true,
      timeout_ms: 180000,
    },
    inference_device: "auto",
    runtime_dir: "",
    model_dir: "models",
    selection_mode: "follow_keyboard",
    single_model_filename: null,
    language_models: {},
    language_runtimes: {},
    language_devices: {},
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
    typing_speed_wpm: 450,
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
    clipboard_buffer_notices: false,
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
const modelDownloads = new Map();
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
for (const model of models) model.installed = false;
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
let readerConfig = {
  revision:1,
  read_along:{prefetch:true,advance_key:"Space",action:"key",delay_ms:900,retry_count:5},
  request_budgets:{connect_timeout_secs:3,queue_timeout_secs:30,cold_model_load_timeout_secs:180,warm_model_load_timeout_secs:30,warm_inference_timeout_secs:60,stream_inactivity_timeout_secs:60,total_timeout_secs:300},
  speech:{selection:{mode:"auto_detect",single:{runtime:null,model:null,device:"auto"},languages:{}},provider:{kind:"local_kokoro"},voice:"af_heart",language:"en-us",speed:1,device:"auto",model_dir:"/assets/kokoro",runtime_dir:"/assets/sherpa-cpu",worker_library:null},
  ocr:{selection:{mode:"single_model",single:{runtime:null,model:null,device:"auto"},languages:{}},provider:{kind:"local_paddle"},language:"en",model_dir:"/assets/paddle-ar-en",runtime_dir:"/assets/sherpa-cpu",device:"auto",idle_timeout_secs:300,worker_library:null},
  playback:{keep_warm_seconds:300,output_device:null,max_buffer_seconds:8,sentence_gap_ms:120,auto_dismiss_seconds:2},
  shortcuts:{read_along:"None",read_selection:"Ctrl+Alt+R",read_region:"Ctrl+Alt+O",copy_region:"None",listen_clipboard:"Ctrl+Alt+C"},
  selection_copy:{backend:"auto",shortcut:"ctrl_insert",allow_clipboard_copy:true,show_clipboard_notice:true},selection_copy_overrides:[],privacy:{hide_text_in_notifications:true},server:{enabled:false,listen:"127.0.0.1:8765",allow_remote:false,max_clients:4,max_text_bytes:1048576,max_image_pixels:20000000},
};
const assets = [
  {id:"parakeet-vulkan",label:"Parakeet · Vulkan",kind:"dictation",bytes:18482077,installed:true,available:true,directory:"/assets/parakeet-vulkan",dependencies:[]},
  {id:"parakeet-cuda",label:"Parakeet · CUDA",kind:"dictation",bytes:563569811,installed:false,available:true,directory:"/assets/parakeet-cuda",dependencies:[]},
  {id:"sherpa-cpu",label:"Sherpa ONNX · CPU",kind:"reading",bytes:9816899,installed:true,available:true,directory:"/assets/sherpa-cpu",dependencies:[]},
  {id:"sherpa-cuda",label:"Sherpa ONNX · CUDA",kind:"reading",bytes:1304247059,installed:false,available:true,directory:"/assets/sherpa-cuda",dependencies:["cuda-cudnn","cuda-cublas","cuda-nvrtc"]},
  {id:"kokoro",label:"Kokoro · English",kind:"voice",bytes:349886476,installed:true,available:true,directory:"/assets/kokoro",dependencies:[]},
  {id:"paddle-ar-en",label:"PaddleOCR · Arabic + English",kind:"ocr",bytes:8026538,installed:true,available:true,directory:"/assets/paddle-ar-en",dependencies:[]},
  {id:"supertonic3",label:"Supertonic 3 · multilingual",kind:"voice",bytes:128774318,installed:false,available:true,directory:"/assets/supertonic3",dependencies:[]},
];
for (const asset of assets) asset.installed = false;
const runtimes = [
  {id:"parakeet-vulkan",feature:"stt",engine:"parakeet",backend:"vulkan",label:"Parakeet Vulkan",asset:"parakeet-vulkan",directory:"/assets/parakeet-vulkan"},
  {id:"parakeet-cuda",feature:"stt",engine:"parakeet",backend:"cuda",label:"Parakeet CUDA",asset:"parakeet-cuda",directory:"/assets/parakeet-cuda"},
  {id:"sherpa-cpu",feature:"tts",engine:"sherpa",backend:"cpu",label:"Sherpa ONNX · CPU",asset:"sherpa-cpu",directory:"/assets/sherpa-cpu"},
  {id:"sherpa-cuda",feature:"tts",engine:"sherpa",backend:"cuda",label:"Sherpa ONNX · CUDA",asset:"sherpa-cuda",directory:"/assets/sherpa-cuda"},
  {id:"paddle-cpu",feature:"ocr",engine:"paddleocr",backend:"cpu",label:"Sherpa ONNX · CPU",asset:"sherpa-cpu",directory:"/assets/sherpa-cpu"},
  {id:"paddle-cuda",feature:"ocr",engine:"paddleocr",backend:"cuda",label:"Sherpa ONNX · CUDA",asset:"sherpa-cuda",directory:"/assets/sherpa-cuda"},
];
const moduleModels = [
  {id:"kokoro",feature:"tts",engine:"sherpa",label:"Kokoro · English",languages:["en"],asset:"kokoro",recommended:true},
  {id:"supertonic3",feature:"tts",engine:"sherpa",label:"Supertonic 3",languages:["ar","en"],asset:"supertonic3"},
  {id:"paddle-ar-en",feature:"ocr",engine:"paddleocr",label:"PaddleOCR Arabic + English",languages:["ar","en"],asset:"paddle-ar-en"},
];
function inventory() {
  const recommend = row => options.recommendations && Object.hasOwn(options.recommendations,row.id) ? {...row,recommended:!!options.recommendations[row.id],recommendation:options.recommendations[row.id]} : row;
  return {runtimes:runtimes.map(r=>recommend({...r,installed:r.asset ? !!assets.find(a=>a.id===r.asset)?.installed : !!r.installed})),models:[...models.map(m=>recommend({id:m.file,feature:"stt",engine:"parakeet",label:m.family,languages:m.languages.map(l=>l.match(/\(([^)]+)\)/)?.[1]).filter(Boolean),installed:m.installed,asset:null,recommended:m.recommended})),...moduleModels.map(m=>recommend({...m,installed:!!assets.find(a=>a.id===m.asset)?.installed}))]};
}
function installFixture(id) {
  const asset=assets.find(a=>a.id===id); const model=models.find(m=>m.file===id); const runtime=runtimes.find(r=>r.id===id);
  if(asset) {asset.installed=true;const job=assetJobs.find(job=>job.job_id===id);if(job)job.phase='ready';advanceAssetQueue();} else if(model) model.installed=true; else if(runtime) runtime.installed=true; else throw Error(`Unknown fixture ${id}`);
}
let assetProgress={id:"",phase:"",downloaded:0,total:0,error:"",queued:[]};
const assetJobs=[];
function advanceAssetQueue() {
  let active=assetJobs.filter(job=>['downloading','verifying','extracting'].includes(job.phase)).length;
  for(const job of assetJobs) if(job.phase==='queued'&&active<2){job.phase='downloading';active++;}
}
const server = http.createServer(async (req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  if (
    url.pathname.startsWith("/api/") &&
    req.headers["x-simple-stt-token"] !== token
  )
    return sendJson(res, 401, { error: "Missing test token" });
  try {
    if(url.pathname==="/api/modules") return sendJson(res,200,inventory());
    if(url.pathname==="/api/assets") return sendJson(res,200,{assets,progress:assetProgress,jobs:assetJobs,selection_revision:options.selectionRevision||0});
    if(url.pathname==="/api/asset-action") {
      const body=JSON.parse(await readBody(req));actions.push(body);
      if(body.action==="install") {const asset=assets.find(a=>a.id===body.id);assert(asset);let job=assetJobs.find(job=>job.job_id===body.id);if(!job||!['queued','downloading','verifying','extracting'].includes(job.phase)){const next={id:body.id,job_id:body.id,phase:'queued',downloaded:Math.floor(asset.bytes/4),total:asset.bytes,error:'',queued:[]};if(job)Object.assign(job,next);else{job=next;assetJobs.push(job);}advanceAssetQueue();}assetProgress=job;}
      if(body.action==="cancel") {for(const job of assetJobs)if(!body.id||job.job_id===body.id){job.phase='cancelled';job.error='';}advanceAssetQueue();}
      if(body.action==="remove") {const asset=assets.find(a=>a.id===body.id);assert(asset);asset.installed=false;}
      return sendJson(res,200,{ok:true});
    }
    if(url.pathname==="/api/provider-discovery") {const body=JSON.parse(await readBody(req));actions.push({action:'provider_discovery',...body});return sendJson(res,200,{models:[{id:'server-model',label:'Server model'}],voices:[{id:'af_bella',label:'Bella',model:'server-model'}],voices_supported:true});}
    if(url.pathname==="/api/reader") return sendJson(res,200,{config:clone(readerConfig),status:null,speech_key_saved:false,ocr_key_saved:false});
    if(url.pathname==="/api/reader-status") return sendJson(res,200,{status:null});
    if(url.pathname==="/api/reader-output-devices") return sendJson(res,200,{devices:[{id:"fixture:headset",name:"Fixture headphones"}]});
    if(url.pathname==="/api/reader-save") {const body=JSON.parse(await readBody(req));assert.equal(body.expected_revision,readerConfig.revision);readerConfig=body.config;readerConfig.revision++;return sendJson(res,200,{config:clone(readerConfig)});}
    if(url.pathname==="/api/reader-action") {actions.push(JSON.parse(await readBody(req)));return sendJson(res,200,{message:"Reading started"});}
    if(url.pathname==="/api/state" && req.method === "GET")
      return sendJson(res, 200, {
        config: clone(serverConfig),
        config_hash: `hash-${configHash}`,
        platform: options.platform,
        ui_localization: { system_locale: options.systemLocale || "en", locale: serverConfig.general.ui_language === "auto" ? (options.systemLocale || "en") : serverConfig.general.ui_language, direction: (serverConfig.general.ui_language === "ar" || (serverConfig.general.ui_language === "auto" && options.systemLocale === "ar")) ? "rtl" : "ltr" },
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
        const job = {cancelled:false, stop:()=>{}};
        modelDownloads.set(body.filename,job);
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
        await new Promise(resolve=>{const timer=setTimeout(resolve,options.modelDownloadDelay||800);job.stop=()=>{clearTimeout(timer);resolve();};});
        modelDownloads.delete(body.filename);
        if(!job.cancelled) model.installed = true;
        const event = {
          seq: ++eventSeq,
          kind: job.cancelled ? "model_download_cancelled" : "model_download_complete",
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
      if(body.action==='cancel_model_download') {const job=modelDownloads.get(body.filename);if(job){job.cancelled=true;job.stop();}}
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
      url.pathname.endsWith(".css") ||
      url.pathname.endsWith(".woff2")
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
        ".woff2": "font/woff2",
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

function publishEvent(kind, values = {}, text) {
  const event = { seq: ++eventSeq, kind, values, text };
  queuedEvents.push(event);
  const waiter = [...eventWaiters][0];
  if (waiter) {
    eventWaiters.delete(waiter);
    clearTimeout(waiter.timeout);
    waiter.res.end(
      `data: ${JSON.stringify({ events: queuedEvents.splice(0) })}\n\n`,
    );
  }
  return event.seq;
}
module.exports = {
  publishEvent,
  options,
  eventWaiters,
  server,
  models,
  actions,
  keyboardDiscovery,
  assets,
  assetJobs,
  runtimes,
  installFixture,
  inventory,
  makeConfig,
  setConfig(value) {
    serverConfig = value;
  },
  getConfig() {
    return serverConfig;
  },
  getReaderConfig() {
    return readerConfig;
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

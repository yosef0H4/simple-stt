"use strict";
const token = new URLSearchParams(location.hash.slice(1)).get("token");
let state,
  config,
  baseline,
  dirty = false,
  lastSeq = 0,
  streaming = true,
  noticeTimer,
  modelSearchQuery = "";
const modelDownloads = new Map();
const $ = (s) => document.querySelector(s),
  $$ = (s) => [...document.querySelectorAll(s)];
const actionIcons = {
  refresh:
    '<path d="M20 6v5h-5M4 18v-5h5"/><path d="M18.5 9A7 7 0 0 0 6 6.5L4 9m2 6a7 7 0 0 0 12 2.5L20 15"/>',
  reset:
    '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5"/>',
  download: '<path d="M12 3v12m-5-5 5 5 5-5M5 21h14"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  test: '<path d="m9 3-5 9h7l-1 9 10-12h-7l1-6Z"/>',
  trash: '<path d="M3 6h18M8 6V4h8v2m3 0-1 15H6L5 6m5 4v7m4-7v7"/>',
};
const groupIcons = {
  "Dictation shortcuts": '<path d="M224,48H32A16,16,0,0,0,16,64V192a16,16,0,0,0,16,16H224a16,16,0,0,0,16-16V64A16,16,0,0,0,224,48Zm0,144H32V64H224V192Zm-16-64a8,8,0,0,1-8,8H56a8,8,0,0,1,0-16H200A8,8,0,0,1,208,128Zm0-32a8,8,0,0,1-8,8H56a8,8,0,0,1,0-16H200A8,8,0,0,1,208,96ZM72,160a8,8,0,0,1-8,8H56a8,8,0,0,1,0-16h8A8,8,0,0,1,72,160Zm96,0a8,8,0,0,1-8,8H96a8,8,0,0,1,0-16h64A8,8,0,0,1,168,160Zm40,0a8,8,0,0,1-8,8h-8a8,8,0,0,1,0-16h8A8,8,0,0,1,208,160Z"/>',
  "System behavior": '<path d="M128,80a48,48,0,1,0,48,48A48.05,48.05,0,0,0,128,80Zm0,80a32,32,0,1,1,32-32A32,32,0,0,1,128,160Zm88-29.84q.06-2.16,0-4.32l14.92-18.64a8,8,0,0,0,1.48-7.06,107.21,107.21,0,0,0-10.88-26.25,8,8,0,0,0-6-3.93l-23.72-2.64q-1.48-1.56-3-3L186,40.54a8,8,0,0,0-3.94-6,107.71,107.71,0,0,0-26.25-10.87,8,8,0,0,0-7.06,1.49L130.16,40Q128,40,125.84,40L107.2,25.11a8,8,0,0,0-7.06-1.48A107.6,107.6,0,0,0,73.89,34.51a8,8,0,0,0-3.93,6L67.32,64.27q-1.56,1.49-3,3L40.54,70a8,8,0,0,0-6,3.94,107.71,107.71,0,0,0-10.87,26.25,8,8,0,0,0,1.49,7.06L40,125.84Q40,128,40,130.16L25.11,148.8a8,8,0,0,0-1.48,7.06,107.21,107.21,0,0,0,10.88,26.25,8,8,0,0,0,6,3.93l23.72,2.64q1.49,1.56,3,3L70,215.46a8,8,0,0,0,3.94,6,107.71,107.71,0,0,0,26.25,10.87,8,8,0,0,0,7.06-1.49L125.84,216q2.16.06,4.32,0l18.64,14.92a8,8,0,0,0,7.06,1.48,107.21,107.21,0,0,0,26.25-10.88,8,8,0,0,0,3.93-6l2.64-23.72q1.56-1.48,3-3L215.46,186a8,8,0,0,0,6-3.94,107.71,107.71,0,0,0,10.87-26.25,8,8,0,0,0-1.49-7.06Z"/>',
  Microphone: '<path d="M128,176a48.05,48.05,0,0,0,48-48V64a48,48,0,0,0-96,0v64A48.05,48.05,0,0,0,128,176ZM96,64a32,32,0,0,1,64,0v64a32,32,0,0,1-64,0Zm40,143.6V240a8,8,0,0,1-16,0V207.6A80.11,80.11,0,0,1,48,128a8,8,0,0,1,16,0,64,64,0,0,0,128,0,8,8,0,0,1,16,0A80.11,80.11,0,0,1,136,207.6Z"/>',
  "Recognition engine": '<path d="M152,96H104a8,8,0,0,0-8,8v48a8,8,0,0,0,8,8h48a8,8,0,0,0,8-8V104A8,8,0,0,0,152,96Zm-8,48H112V112h32Zm88,0H216V112h16a8,8,0,0,0,0-16H216V56a16,16,0,0,0-16-16H160V24a8,8,0,0,0-16,0V40H112V24a8,8,0,0,0-16,0V40H56A16,16,0,0,0,40,56V96H24a8,8,0,0,0,0,16H40v32H24a8,8,0,0,0,0,16H40v40a16,16,0,0,0,16,16H96v16a8,8,0,0,0,16,0V216h32v16a8,8,0,0,0,16,0V216h40a16,16,0,0,0,16-16V160h16a8,8,0,0,0,0-16Zm-32,56H56V56H200V200Z"/>',
  Delivery: '<path d="M227.32,28.68a16,16,0,0,0-15.66-4.08L19.57,82.84a16,16,0,0,0-2.49,29.8L102,154l41.3,84.87A15.86,15.86,0,0,0,157.74,248a15.88,15.88,0,0,0,15.38-11.51l58.2-191.94A16,16,0,0,0,227.32,28.68ZM157.83,231.85l-40.13-82.23,48-48a8,8,0,0,0-11.31-11.31l-48,48L24.08,98.25,216,40Z"/>',
  "Text cleanup": '<path d="M235.5,216.81c-22.56-11-35.5-34.58-35.5-64.8V134.73a15.94,15.94,0,0,0-10.09-14.87L165,110a8,8,0,0,1-4.48-10.34l21.32-53a28,28,0,0,0-16.1-37,28.14,28.14,0,0,0-35.82,16L108.9,79a8,8,0,0,1-10.37,4.49L73.11,73.14A15.89,15.89,0,0,0,55.74,76.8C34.68,98.45,24,123.75,24,152a111.45,111.45,0,0,0,31.18,77.53A8,8,0,0,0,61,232H232a8,8,0,0,0,3.5-15.19ZM67.14,88l25.41,10.3a24,24,0,0,0,31.23-13.45l21-53c2.56-6.11,9.47-9.27,15.43-7a12,12,0,0,1,6.88,15.92L145.69,93.76a24,24,0,0,0,13.43,31.14L184,134.73V152c0,.33,0,.66,0,1L55.77,101.71A108.84,108.84,0,0,1,67.14,88Zm48,128a87.53,87.53,0,0,1-24.34-42,8,8,0,0,0-15.49,4,105.16,105.16,0,0,0,18.36,38H64.44A95.54,95.54,0,0,1,40,152a85.9,85.9,0,0,1,7.73-36.29l137.8,55.12c3,18,10.56,33.48,21.89,45.16Z"/>',
  Runtime: '<path d="M245,110.64A16,16,0,0,0,232,104H216V88a16,16,0,0,0-16-16H130.67L102.94,51.2a16.14,16.14,0,0,0-9.6-3.2H40A16,16,0,0,0,24,64V208a8,8,0,0,0,8,8H211.1a8,8,0,0,0,7.59-5.47l28.49-85.47A16.05,16.05,0,0,0,245,110.64ZM93.34,64,123.2,86.4A8,8,0,0,0,128,88h72v16H69.77a16,16,0,0,0-15.18,10.94L40,158.7V64Zm112,136H43.1l26.67-80H232Z"/>',
  Diagnostics: '<path d="M240,128a8,8,0,0,1-8,8H204.94l-37.78,75.58A8,8,0,0,1,160,216h-.4a8,8,0,0,1-7.08-5.14L95.35,60.76,63.28,131.31A8,8,0,0,1,56,136H24a8,8,0,0,1,0-16H50.85L88.72,36.69a8,8,0,0,1,14.76.46l57.51,151,31.85-63.71A8,8,0,0,1,200,120h32A8,8,0,0,1,240,128Z"/>',
  "Speech model": '<path d="M248,124a56.11,56.11,0,0,0-32-50.61V72a48,48,0,0,0-88-26.49A48,48,0,0,0,40,72v1.39a56,56,0,0,0,0,101.2V176a48,48,0,0,0,88,26.49A48,48,0,0,0,216,176v-1.41A56.09,56.09,0,0,0,248,124ZM88,208a32,32,0,0,1-31.81-28.56A55.87,55.87,0,0,0,64,180h8a8,8,0,0,0,0-16H64A40,40,0,0,1,50.67,86.27,8,8,0,0,0,56,78.73V72a32,32,0,0,1,64,0v68.26A47.8,47.8,0,0,0,88,128a8,8,0,0,0,0,16,32,32,0,0,1,0,64Zm104-44h-8a8,8,0,0,0,0,16h8a55.87,55.87,0,0,0,7.81-.56A32,32,0,1,1,168,144a8,8,0,0,0,0-16,47.8,47.8,0,0,0-32,12.26V72a32,32,0,0,1,64,0v6.73a8,8,0,0,0,5.33,7.54A40,40,0,0,1,192,164Z"/>',
};
const actionIcon = (name) =>
  `<svg class="action-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${actionIcons[name]}</svg>`;
const groupIcon = (title) =>
  `<span class="group-icon" aria-hidden="true"><svg viewBox="0 0 256 256" fill="currentColor">${groupIcons[title] || groupIcons["System behavior"]}</svg></span>`;
async function api(path, opts = {}) {
  const headers = { "X-Simple-STT-Token": token, ...(opts.headers || {}) };
  if (opts.body && !headers["Content-Type"])
    headers["Content-Type"] = "application/json";
  const r = await fetch(path, { ...opts, headers });
  const data = await r.json();
  if (!r.ok) throw new Error(data.error || r.statusText);
  return data;
}
const get = (path) => path.split(".").reduce((v, k) => v[k], config);
function set(path, value) {
  const p = path.split("."),
    k = p.pop();
  p.reduce((v, n) => v[n], config)[k] = value;
  if (path === "general.ui_theme") applyTheme(value);
  markDirty();
}
const applyTheme = (value) =>
  (document.documentElement.dataset.theme = value || "auto");
function group(parent, title, description) {
  const root = document.createElement("div");
  root.className = "setting-group";
  root.innerHTML = `${groupIcon(title)}<div class="group-head"><div><h2>${title}</h2>${description ? `<p>${description}</p>` : ""}</div><button type="button" class="group-reset" aria-label="Reset ${title} to defaults" title="Reset only this group to defaults">${actionIcon("reset")}<span>Reset group</span></button></div>`;
  root.querySelector(".group-reset").onclick = async () => {
    try {
      const defaults = (await api("/api/defaults")).config;
      const paths = [...root.querySelectorAll("[data-setting-path]")]
        .map((node) => node.dataset.settingPath)
        .filter(Boolean);
      for (const path of new Set(paths)) {
        const parts = path.split("."),
          key = parts.pop(),
          defaultParent = parts.reduce((value, part) => value?.[part], defaults),
          configParent = parts.reduce((value, part) => value?.[part], config);
        if (defaultParent && configParent && key in defaultParent)
          configParent[key] = structuredClone(defaultParent[key]);
      }
      build();
      markDirty();
      notice(`${title} reset to defaults. Save to apply it.`);
    } catch (error) {
      notice(error.message, "error");
    }
  };
  parent.append(root);
  return root;
}
function field(
  parent,
  { label, description, path, type = "text", options = [], min, max, step, suffix = "" },
) {
  const wrap = document.createElement("label"),
    copy = document.createElement("span"),
    control = document.createElement("span"),
    input =
      type === "select"
        ? document.createElement("select")
        : document.createElement("input");
  wrap.className = "field";
  wrap.dataset.settingPath = path;
  copy.className = "field-copy";
  control.className = "control";
  copy.innerHTML = `<strong>${label}</strong>${description ? `<small>${description}</small>` : ""}`;
  input.dataset.path = path;
  if (type === "checkbox") {
    wrap.classList.add("field-switch");
    input.type = "checkbox";
    input.checked = get(path);
    const toggle = document.createElement("span");
    toggle.className = "switch";
    toggle.append(input, document.createTextNode(input.checked ? "On" : "Off"));
    input.onchange = () => {
      toggle.lastChild.textContent = input.checked ? "On" : "Off";
      set(path, input.checked);
    };
    control.append(toggle);
  } else {
    if (type !== "select") input.type = type;
    if (min !== undefined) input.min = min;
    if (max !== undefined) input.max = max;
    if (step !== undefined) input.step = step;
    for (const [value, name] of options) {
      const option = document.createElement("option");
      option.value = value;
      option.textContent = name;
      input.append(option);
    }
    input.value = get(path);
    const valueOutput = type === "range" ? document.createElement("output") : null;
    if (valueOutput) {
      control.classList.add("range-control");
      valueOutput.className = "range-value";
      valueOutput.textContent = `${input.value}${suffix}`;
    }
    input.oninput = () => {
      const value = input.type === "number" || input.type === "range" ? Number(input.value) : input.value;
      if (valueOutput) valueOutput.textContent = `${value}${suffix}`;
      set(path, value);
    };
    control.append(input);
    if (valueOutput) control.append(valueOutput);
  }
  wrap.append(copy, control);
  parent.append(wrap);
  return input;
}
function hotkeyField(parent, label, description, path) {
  const wrap = document.createElement("div");
  wrap.className = "field";
  wrap.dataset.settingPath = path;
  wrap.innerHTML = `<span class="field-copy"><strong>${label}</strong><small>${description}</small></span>`;
  const control = document.createElement("div"),
    value = document.createElement("output"),
    button = document.createElement("button");
  control.className = "control hotkey-control";
  value.className = "hotkey-value";
  value.textContent = get(path);
  button.type = "button";
  button.textContent =
    state.platform === "windows" ? "Record" : "System managed";
  button.disabled = state.platform !== "windows";
  button.onclick = () => captureHotkey(path, label);
  control.append(value, button);
  wrap.append(control);
  parent.append(wrap);
}
function portalShortcutField(parent, label, description, id) {
  const wrap = document.createElement("div");
  wrap.className = "field portal-shortcut";
  wrap.dataset.settingPath = `linux.shortcut.${id}`;
  wrap.innerHTML = `<span class="field-copy"><strong>${label}</strong><small>${description}</small></span>`;
  const value = document.createElement("output");
  value.className = "hotkey-value";
  value.textContent = state.shortcut_state?.[id] || "Not assigned";
  const control = document.createElement("div");
  control.className = "control";
  control.append(value);
  wrap.append(control);
  parent.append(wrap);
}
function commandShortcutField(parent, label, description, command) {
  const wrap = document.createElement("div");
  wrap.className = "field portal-shortcut";
  wrap.innerHTML = `<span class="field-copy"><strong>${label}</strong><small>${description}</small></span>`;
  const value = document.createElement("output");
  value.className = "hotkey-value";
  value.textContent = command;
  const control = document.createElement("div");
  control.className = "control";
  control.append(value);
  wrap.append(control);
  parent.append(wrap);
}
function linuxHotkeyGuide(parent, backend, tools) {
  const desktop = String(tools.desktop || "Unknown desktop");
  const session = String(tools.session || "Unknown session");
  const resolved = backend === "auto" ? (session === "X11" ? "x11" : "portal") : backend;
  const guide = document.createElement("details");
  guide.className = "automation-guide";
  const summary = document.createElement("summary");
  summary.textContent = `Shortcut setup guide · ${desktop} (${session})`;
  const body = document.createElement("div");
  body.className = "automation-guide-body";
  const note = document.createElement("p");
  note.innerHTML = "<strong>Compatibility note:</strong> Linux desktop support is experimental. KDE Plasma on Fedora Wayland is the only environment tested on real hardware so far.";
  const intro = document.createElement("p");
  if (resolved === "portal") {
    intro.textContent = "The desktop portal owns these shortcuts. Select Configure, approve the request once, then assign Record, Cancel, and Switch delivery in the system dialog.";
  } else if (resolved === "x11") {
    intro.textContent = "Simple STT registers these chords directly with the X11 server. Enter each chord here, Save, then restart Simple STT. If another application owns a chord, Simple STT reports a conflict instead of replacing it.";
  } else {
    intro.textContent = "Your Wayland compositor does not provide the Global Shortcuts portal. Bind the commands below in the compositor configuration, then reload its configuration.";
  }
  const install = document.createElement("p");
  install.innerHTML = "<strong>Add Simple STT to your desktop:</strong> run <code>simple-stt-linux install-user-service</code>, then <code>systemctl --user enable --now simple-stt-linux.service</code>. This installs its app launchers and starts it automatically for your user.";
  body.append(note, install, intro);
  if (resolved === "desktop") {
    const examples = document.createElement("ul");
    examples.innerHTML = `<li><strong>Hyprland</strong> — add <code>bind = SUPER, Z, exec, simple-stt-linux toggle</code>, <code>bind = SUPER, X, exec, simple-stt-linux cancel</code>, and <code>bind = SUPER, D, exec, simple-stt-linux cycle-delivery</code> to <code>~/.config/hypr/hyprland.conf</code>, then run <code>hyprctl reload</code>.</li><li><strong>Sway</strong> — add <code>bindsym $mod+z exec simple-stt-linux toggle</code>, equivalent Cancel and Cycle bindings to <code>~/.config/sway/config</code>, then run <code>swaymsg reload</code>.</li><li><strong>Other compositors</strong> — create three global key bindings that run the Toggle, Cancel, and Switch delivery commands shown above. Use the full executable path if <code>simple-stt-linux</code> is not on PATH.</li>`;
    body.append(examples);
  }
  const caps = document.createElement("small");
  caps.textContent = resolved === "x11" ? "Use ordinary modifier chords such as Ctrl+Alt+R or Meta+Z. Caps Lock custom chords are desktop-dependent on X11." : "Start and Close are separate commands because they must work while the app is not running.";
  body.append(caps);
  guide.append(summary, body);
  parent.append(guide);
}
function linuxDeliveryChooser(parent, tools) {
  const backends = [
    ["auto", "Automatic", true],
    ["ydotool", "ydotool", Boolean(tools.ydotool && tools.ydotool_daemon)],
    ["wtype", "wtype", Boolean(tools.wtype)],
    ["xdotool", "xdotool", Boolean(tools.xdotool)],
    ["native", "Native fast paste", Boolean(tools.native)],
    ["clipboard_only", "wl-clipboard", Boolean(tools.wl_clipboard)],
  ];
  const modes = [
    ["smart_paste", "Smart Paste", "Shift+Insert · terminals use Ctrl+Shift+V", false],
    ["type", "Type", "Simulated typing", false],
    ["clipboard", "Clipboard", "Manual paste", false],
    ["paste_shift_insert", "Shift+Insert", "Always use Shift+Insert", true],
    ["paste_ctrl_shift_v", "Ctrl+Shift+V", "Always use Ctrl+Shift+V", true],
    ["paste_ctrl_v", "Ctrl+V", "Compatibility override", true],
  ];
  const supported = (backend, mode) =>
    backend === "clipboard_only" ? mode === "clipboard" : backend === "native" ? mode !== "type" : true;
  config.output.linux_delivery_cycle ||= [
    { backend: "auto", mode: "smart_paste" },
    { backend: "auto", mode: "type" },
  ];
  const root = document.createElement("div");
  root.className = "delivery-picker";
  root.dataset.settingPath = "output.linux_delivery_cycle";
  const search = document.createElement("input");
  search.type = "search";
  search.className = "delivery-picker-search";
  search.placeholder = "Search tools and delivery methods…";
  search.setAttribute("aria-label", "Search Linux delivery options");
  const results = document.createElement("div");
  results.className = "delivery-picker-results";
  const same = (choice, backend, mode) => choice.backend === backend && choice.mode === mode;
  const draw = () => {
    const query = search.value.trim().toLowerCase();
    results.replaceChildren();
    const advanced = document.createElement("details");
    advanced.className = "delivery-picker-advanced";
    const advancedSummary = document.createElement("summary");
    advancedSummary.textContent = "Advanced paste shortcuts";
    advanced.append(advancedSummary);
    for (const [mode, modeLabel, detail, isAdvanced] of modes) {
      const target = isAdvanced ? advanced : results;
      const matches = backends.filter(([backend, backendLabel]) =>
        supported(backend, mode) && `${backendLabel} ${modeLabel} ${detail}`.toLowerCase().includes(query));
      if (!matches.length) continue;
      const heading = document.createElement("h3");
      heading.textContent = `${modeLabel} · ${detail}`;
      target.append(heading);
      for (const [backend, backendLabel, installed] of matches) {
        const row = document.createElement("div");
        const current = config.output.linux_automation_backend === backend && config.output.delivery_mode === mode;
        const cycling = config.output.linux_delivery_cycle.some((choice) => same(choice, backend, mode));
        row.className = "delivery-picker-row";
        row.classList.toggle("selected", current);
        const choose = document.createElement("button");
        choose.type = "button";
        choose.className = "delivery-picker-select";
        choose.innerHTML = `<span><strong>${backendLabel}</strong><small>${installed ? "Installed" : "Not installed"}${current ? " · Current" : ""}</small></span>`;
        choose.onclick = () => {
          config.output.linux_automation_backend = backend;
          config.output.delivery_mode = mode;
          if (!config.output.enabled_delivery_modes.includes(mode)) config.output.enabled_delivery_modes.push(mode);
          if (!installed) notice(`${backendLabel} is not ready. Install it before using this option.`, "error");
          markDirty();
          draw();
        };
        const cycle = document.createElement("label");
        cycle.className = "delivery-picker-cycle";
        const checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.checked = cycling;
        cycle.append(checkbox, document.createTextNode("Cycle"));
        checkbox.onchange = () => {
          if (checkbox.checked) {
            config.output.linux_delivery_cycle.push({ backend, mode });
          } else {
            config.output.linux_delivery_cycle = config.output.linux_delivery_cycle.filter((choice) => !same(choice, backend, mode));
          }
          markDirty();
          draw();
        };
        row.append(choose, cycle);
        target.append(row);
      }
    }
    if (advanced.childElementCount > 1) {
      if (query || ["paste_shift_insert", "paste_ctrl_shift_v", "paste_ctrl_v"].includes(config.output.delivery_mode)) advanced.open = true;
      results.append(advanced);
    }
  };
  search.oninput = draw;
  root.append(search, results);
  parent.append(root);
  draw();
}
function appOverrides(parent) {
  config.output.app_overrides ||= [];
  const wrap = document.createElement("div");
  wrap.className = "app-overrides";
  const intro = document.createElement("p");
  intro.className = "app-overrides-help";
  intro.textContent = "Override delivery only for specific apps. Useful for terminals, games, and apps that reject simulated paste.";
  const rows = document.createElement("div");
  rows.className = "app-overrides-rows";
  const options = [
    ["smart_paste", "Smart Paste"],
    ["type", "Type"],
    ["clipboard", "Clipboard only"],
    ["paste_shift_insert", "Regular app · Shift+Insert"],
    ["paste_ctrl_shift_v", "Terminal · Ctrl+Shift+V"],
    ["paste_ctrl_v", "Ctrl+V compatibility"],
  ];
  const draw = () => {
    rows.replaceChildren();
    config.output.app_overrides.forEach((entry, index) => {
      const row = document.createElement("div");
      row.className = "app-override-row";
      const app = document.createElement("input");
      app.type = "text";
      app.value = entry.app_id;
      app.placeholder = "Application ID, e.g. kitty";
      app.setAttribute("aria-label", "Application identity");
      app.oninput = () => { entry.app_id = app.value; markDirty(); };
      const mode = document.createElement("select");
      mode.setAttribute("aria-label", `Delivery mode for ${entry.app_id || "application"}`);
      for (const [value, label] of options) {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = label;
        option.selected = entry.mode === value;
        mode.append(option);
      }
      mode.onchange = () => { entry.mode = mode.value; markDirty(); };
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "app-override-remove";
      remove.title = "Remove app override";
      remove.setAttribute("aria-label", "Remove app override");
      remove.textContent = "Remove";
      remove.onclick = () => {
        config.output.app_overrides.splice(index, 1);
        markDirty();
        draw();
      };
      row.append(app, mode, remove);
      rows.append(row);
    });
    if (!config.output.app_overrides.length) {
      const empty = document.createElement("p");
      empty.className = "app-overrides-empty";
      empty.textContent = "No app-specific overrides.";
      rows.append(empty);
    }
  };
  const actions = document.createElement("div");
  actions.className = "app-overrides-actions";
  const addCurrent = document.createElement("button");
  addCurrent.type = "button";
  addCurrent.textContent = "Add current app";
  addCurrent.onclick = async () => {
    notice("Switch to the target app now. Detecting it in 3 seconds…");
    try {
      const result = await api("/api/platform-action", {
        method: "POST",
        body: JSON.stringify({ action: "focused_app", filename: "" }),
      });
      const existing = config.output.app_overrides.find((entry) => entry.app_id.toLowerCase() === result.app_id.toLowerCase());
      if (existing) notice(`${result.app_id} already has an override.`, "error");
      else {
        config.output.app_overrides.push({ app_id: result.app_id, mode: "smart_paste" });
        markDirty();
        draw();
        notice(`${result.app_id} added. Choose its delivery mode, then Save.`, "success");
      }
    } catch (error) {
      notice(`${error.message}. You can add the application ID manually.`, "error");
    }
  };
  const addManual = document.createElement("button");
  addManual.type = "button";
  addManual.textContent = "Add manually";
  addManual.onclick = () => {
    config.output.app_overrides.push({ app_id: "", mode: "smart_paste" });
    markDirty();
    draw();
    rows.querySelector(".app-override-row:last-child input")?.focus();
  };
  actions.append(addCurrent, addManual);
  wrap.append(intro, rows, actions);
  parent.append(wrap);
  draw();
}
function windowsDeliveryChooser(parent) {
  const modes = [
    ["smart_paste", "Smart Paste", "Ctrl+V on Windows"],
    ["type", "Type", "Simulated typing"],
    ["clipboard", "Clipboard", "Manual paste"],
  ];
  const root = document.createElement("div");
  root.className = "delivery-picker";
  for (const [value, label, detail] of modes) {
    const row = document.createElement("div");
    row.className = "delivery-picker-row";
    row.classList.toggle("selected", config.output.delivery_mode === value);
    const choose = document.createElement("button");
    choose.type = "button";
    choose.className = "delivery-picker-select";
    choose.innerHTML = `<span><strong>${label}</strong><small>${detail}${config.output.delivery_mode === value ? " · Current" : ""}</small></span>`;
    choose.onclick = () => {
      config.output.delivery_mode = value;
      if (!config.output.enabled_delivery_modes.includes(value)) config.output.enabled_delivery_modes.push(value);
      markDirty();
      build();
    };
    row.append(choose);
    root.append(row);
  }
  const advanced = document.createElement("details");
  advanced.className = "delivery-picker-advanced";
  const advancedSummary = document.createElement("summary");
  advancedSummary.textContent = "Advanced paste shortcuts";
  advanced.append(advancedSummary);
  for (const [value, label] of [["paste_shift_insert", "Shift+Insert"], ["paste_ctrl_shift_v", "Ctrl+Shift+V"], ["paste_ctrl_v", "Ctrl+V compatibility"]]) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "delivery-picker-select";
    button.textContent = label;
    button.onclick = () => {
      config.output.delivery_mode = value;
      if (!config.output.enabled_delivery_modes.includes(value)) config.output.enabled_delivery_modes.push(value);
      markDirty();
      build();
    };
    advanced.append(button);
  }
  if (["paste_shift_insert", "paste_ctrl_shift_v", "paste_ctrl_v"].includes(config.output.delivery_mode)) advanced.open = true;
  root.append(advanced);
  parent.append(root);
}
function linuxAutomationGuide(parent, tools) {
  const desktop = String(tools.desktop || "Unknown desktop");
  const session = String(tools.session || "Unknown session");
  const distro = String(tools.distro || "Linux");
  const id = String(tools.distro_id || "");
  const wayland = session.toLowerCase() === "wayland";
  const desktopLower = desktop.toLowerCase();
  const packages = !wayland
    ? "xdotool xclip"
    : desktopLower.includes("kde") || desktopLower.includes("plasma")
      ? "wl-clipboard"
      : desktopLower.includes("gnome")
        ? "wl-clipboard ydotool"
        : "wl-clipboard wtype";
  const install = id === "fedora"
    ? `sudo dnf install ${packages}`
    : ["arch", "manjaro", "endeavouros"].includes(id)
      ? `sudo pacman -S ${packages}`
      : ["debian", "ubuntu", "linuxmint", "pop"].includes(id)
        ? `sudo apt install ${packages}`
        : id.startsWith("opensuse")
          ? `sudo zypper install ${packages}`
          : id === "alpine"
            ? `sudo apk add ${packages}`
            : id === "void"
              ? `sudo xbps-install -S ${packages}`
              : ["nixos", "nix"].includes(id)
                ? `nix shell ${packages.split(" ").map((name) => `nixpkgs#${name}`).join(" ")}`
          : "Install wl-clipboard plus the recommended input tool with your package manager.";
  let recommendation;
  if (!wayland) recommendation = "X11: xdotool is the simplest input tool; use xclip or xsel for clipboard access.";
  else if (desktopLower.includes("kde") || desktopLower.includes("plasma")) recommendation = "KDE Plasma Wayland: Native fast paste is the safest compositor-aware choice. Use ydotool when you also need simulated typing; it requires ydotoold.";
  else if (desktopLower.includes("gnome")) recommendation = "GNOME Wayland: ydotool is the dependable typing option because it works through Linux uinput. Native portal paste is permission-aware; wtype usually is not suitable for GNOME.";
  else recommendation = "wlroots Wayland (Sway, Hyprland, river, Wayfire): wtype is the lightweight first choice. ydotool is the compositor-independent fallback.";
  const guide = document.createElement("details");
  guide.className = "automation-guide";
  const summary = document.createElement("summary");
  summary.textContent = `What should I install? · ${distro} · ${desktop} (${session})`;
  const body = document.createElement("div");
  body.className = "automation-guide-body";
  const recommended = document.createElement("p");
  recommended.innerHTML = `<strong>Recommended:</strong> ${recommendation}`;
  const roles = document.createElement("ul");
  roles.innerHTML = `<li><strong>wl-clipboard:</strong> clipboard data on Wayland; it does not inject keys.</li><li><strong>wtype:</strong> fast Wayland typing for compositors supporting virtual-keyboard, mainly wlroots.</li><li><strong>ydotool:</strong> Wayland and X11 through uinput; broad compatibility, but ydotoold needs input-device permission.</li><li><strong>xdotool:</strong> X11 only; do not choose it for native Wayland applications.</li>`;
  const command = document.createElement("code");
  command.textContent = install;
  const caution = document.createElement("small");
  caution.textContent = "You do not need every tool. Install wl-clipboard plus one input tool suited to your session.";
  body.append(recommended, roles, command, caution);
  guide.append(summary, body);
  parent.append(guide);
}
function combobox(
  parent,
  { label, description, path, items, placeholder, onchange },
) {
  const wrap = document.createElement("div"),
    control = document.createElement("div"),
    shell = document.createElement("div"),
    input = document.createElement("input"),
    list = document.createElement("div");
  wrap.className = "field";
  wrap.dataset.settingPath = path;
  wrap.innerHTML = `<span class="field-copy"><strong>${label}</strong><small>${description}</small></span>`;
  control.className = "control combo";
  shell.className = "combo-shell";
  input.className = "combo-input";
  input.type = "text";
  input.role = "combobox";
  input.setAttribute("aria-label", label);
  input.autocomplete = "off";
  input.placeholder = placeholder;
  input.setAttribute("aria-expanded", "false");
  list.className = "combo-list";
  list.role = "listbox";
  list.hidden = true;
  const selected = () => items.find((i) => i.value === get(path));
  input.value = selected()?.label || get(path) || "";
  function choose(item) {
    set(path, item.value);
    input.value = item.label;
    list.hidden = true;
    input.setAttribute("aria-expanded", "false");
    if (onchange) onchange();
  }
  function render() {
    const terms = input.value.toLowerCase().trim().split(/\s+/).filter(Boolean),
      matches = items.filter((i) => {
        const searchable = (i.label + " " + (i.meta || "")).toLowerCase();
        return terms.every((term) => searchable.includes(term));
      });
    list.replaceChildren();
    if (!matches.length) {
      const empty = document.createElement("div");
      empty.className = "combo-option";
      empty.textContent = "No matches";
      list.append(empty);
    }
    for (const item of matches) {
      const option = document.createElement("button");
      option.type = "button";
      option.className = "combo-option";
      option.role = "option";
      option.setAttribute("aria-selected", String(item.value === get(path)));
      option.innerHTML = "<strong></strong><small></small>";
      option.firstChild.textContent = item.label;
      option.lastChild.textContent = item.meta || "";
      option.onclick = () => choose(item);
      list.append(option);
    }
    list.hidden = false;
    input.setAttribute("aria-expanded", "true");
  }
  input.onfocus = () => {
    input.value = "";
    render();
  };
  input.oninput = render;
  input.onkeydown = (e) => {
    if (e.key === "Escape") {
      list.hidden = true;
      input.value = selected()?.label || get(path) || "";
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      list.querySelector("button")?.focus();
    }
  };
  document.addEventListener("pointerdown", (e) => {
    if (!control.contains(e.target)) {
      list.hidden = true;
      input.setAttribute("aria-expanded", "false");
      input.value = selected()?.label || get(path) || "";
    }
  });
  shell.append(input);
  control.append(shell, list);
  wrap.append(control);
  parent.append(wrap);
  return input;
}
function build() {
  applyTheme(config.general.ui_theme);
  const g = $("#general-fields"),
    a = $("#audio-fields"),
    o = $("#output-fields"),
    x = $("#advanced-fields");
  [g, a, o, x].forEach((n) => n.replaceChildren());
  const shortcuts = group(
    g,
    "Dictation shortcuts",
    "",
  );
  field(shortcuts, {
    label: "Simple STT",
    description: "",
    path: "general.enabled",
    type: "checkbox",
  });
  if (state.platform === "linux") {
    const tools = state.linux_automation || {};
    const hotkeyBackend = field(shortcuts, {
      label: "Shortcut system",
      description: "Automatic uses the portal on Wayland and native grabs on X11.",
      path: "general.linux_hotkey_backend",
      type: "select",
      options: [["auto", "Automatic (recommended)"], ["portal", "Desktop portal"], ["x11", "Native X11 (X11 sessions only)"], ["desktop", "Desktop-managed commands"]],
    });
    hotkeyBackend.onchange = () => {
      set("general.linux_hotkey_backend", hotkeyBackend.value);
      build();
    };
    const selected = config.general.linux_hotkey_backend || "auto";
    const detected = state.linux_hotkeys?.requested === selected ? state.linux_hotkeys?.active : "";
    const resolved = detected || (selected === "auto" ? (tools.session === "X11" ? "x11" : "portal") : selected);
    commandShortcutField(shortcuts, "Active system", `Status: ${state.linux_hotkeys?.status || "not started"}`, `${resolved === "portal" ? "Desktop portal" : resolved === "x11" ? "Native X11" : "Desktop-managed commands"}${state.linux_hotkeys?.error ? " · setup required" : ""}`);
    if (resolved === "portal") {
      portalShortcutField(shortcuts, "Record", "Assigned by your Wayland desktop.", "record");
      portalShortcutField(shortcuts, "Cancel", "Assigned by your Wayland desktop.", "cancel");
      portalShortcutField(shortcuts, "Switch delivery", "Assigned by your Wayland desktop.", "delivery");
    } else if (resolved === "x11") {
      field(shortcuts, { label: "Record", description: "Native X11 global chord.", path: "general.record_hotkey", type: "text" });
      field(shortcuts, { label: "Cancel", description: "Native X11 global chord.", path: "general.cancel_hotkey", type: "text" });
      field(shortcuts, { label: "Switch delivery", description: "Native X11 global chord.", path: "general.toggle_delivery_hotkey", type: "text" });
    } else {
      commandShortcutField(shortcuts, "Record / stop", "Bind this command in your compositor.", "simple-stt-linux toggle");
      commandShortcutField(shortcuts, "Cancel", "Bind this command in your compositor.", "simple-stt-linux cancel");
      commandShortcutField(shortcuts, "Switch delivery", "Bind this command in your compositor.", "simple-stt-linux cycle-delivery");
    }
    commandShortcutField(shortcuts, "Start program", "Bind this in your desktop or compositor shortcuts; it works while Simple STT is closed.", state.linux_automation?.start_command || "systemctl --user start simple-stt-linux.service");
    commandShortcutField(shortcuts, "Close program", "Bind this in your desktop or compositor shortcuts.", state.linux_automation?.stop_command || "simple-stt-linux shutdown");
    linuxHotkeyGuide(shortcuts, resolved, tools);
    const shortcutActions = document.createElement("div");
    shortcutActions.className = "group-actions";
    const sync = document.createElement("button");
    sync.type = "button";
    sync.className = "shortcut-refresh";
    sync.innerHTML = `${actionIcon("refresh")}<span>Refresh</span>`;
    sync.setAttribute("aria-label", "Refresh or register system shortcuts");
    sync.title = "Refresh assignments or request portal registration";
    sync.onclick = () =>
      api("/api/platform-action", {
        method: "POST",
        body: JSON.stringify({ action: "sync_shortcuts" }),
      })
        .then(refreshState)
        .catch((error) => notice(error.message, "error"));
    const configure = document.createElement("button");
    configure.type = "button";
    configure.textContent = "Configure";
    configure.onclick = () =>
      api("/api/platform-action", {
        method: "POST",
        body: JSON.stringify({ action: "configure_shortcuts" }),
      })
        .then(refreshState)
        .catch((error) => notice(error.message, "error"));
    if (resolved !== "x11") shortcutActions.append(sync, configure);
    const shortcutHead = shortcuts.querySelector(".group-head");
    shortcutHead.insertBefore(shortcutActions, shortcutHead.querySelector(".group-reset"));
  } else {
    field(shortcuts, {
      label: "Recording mode",
      description:
        "Hold records while pressed; toggle starts and stops on separate presses.",
      path: "general.recording_mode",
      type: "select",
      options: [
        ["hold", "Hold to record"],
        ["toggle", "Press to start and stop"],
      ],
    });
    hotkeyField(
      shortcuts,
      "Record",
      "Starts or toggles dictation.",
      "general.record_hotkey",
    );
    hotkeyField(
      shortcuts,
      "Cancel",
      "Stops without delivering text.",
      "general.cancel_hotkey",
    );
    hotkeyField(
      shortcuts,
      "Switch delivery",
      "Changes between typing and paste.",
      "general.toggle_delivery_hotkey",
    );
  }
  const behavior = group(
    g,
    "System behavior",
    "",
  );
  if (state.platform !== "linux")
    field(behavior, {
      label: "Caps Lock tap",
      description: "",
      path: "general.capslock_behavior",
      type: "select",
      options: [
        ["preserve_tap", "Preserve quick tap"],
        ["always_off", "Always keep off"],
      ],
    });
  field(behavior, {
    label: "Start at login",
    description: "",
    path: "general.start_at_login",
    type: "checkbox",
  });
  field(behavior, {
    label: "Theme",
    description: "",
    path: "general.ui_theme",
    type: "select",
    options: [
      ["auto", "Follow system"],
      ["light", "Light"],
      ["dark", "Dark"],
    ],
  });
  const microphone = group(
    a,
    "Microphone",
    "Your preferred device returns automatically after a disconnect.",
  );
  const micItems = [
    {
      value: "",
      label: "System default",
      meta: "Follows your operating system",
    },
    ...(config.audio.preferred_device_id &&
    !state.microphones.some((m) => m.id === config.audio.preferred_device_id)
      ? [
          {
            value: config.audio.preferred_device_id,
            label: "Unavailable preferred microphone",
            meta: config.audio.preferred_device_id,
          },
        ]
      : []),
    ...state.microphones.map((m) => ({
      value: m.id,
      label: m.name,
      meta: m.id,
    })),
  ];
  combobox(microphone, {
    label: "Preferred microphone",
    description: "",
    path: "audio.preferred_device_id",
    items: micItems,
    placeholder: "Search microphones…",
  });
  field(microphone, {
    label: "Input gain",
    description: "1.0 is unchanged.",
    path: "audio.gain",
    type: "number",
  });
  const engine = group(
    a,
    "Recognition engine",
    "",
  );
  field(engine, {
    label: "Inference device",
    description: "",
    path: "speech.inference_device",
    type: "select",
    options: [
      ["auto", "Automatic"],
      ["cpu", "CPU"],
      ["nvidia_gpu", "NVIDIA GPU"],
    ],
  });
  renderModels();
  const delivery = group(
    o,
    "Delivery",
    "",
  );
  if (state.platform === "linux") {
    const tools = state.linux_automation || {};
    const summary = group(o, "Automation & delivery", "Choose the automation tool and how Simple STT inserts text. Missing tools can be selected after you install them.");
    const autoReady = Boolean(
      tools.native ||
      (tools.ydotool && tools.ydotool_daemon) ||
      (tools.session === "Wayland" && tools.wtype) ||
      (tools.session === "X11" && tools.xdotool),
    );
    linuxDeliveryChooser(summary, tools);
    linuxAutomationGuide(summary, tools);
    commandShortcutField(
      summary,
      "Automatic status",
      autoReady
        ? "Ready. Automatic will choose the best installed tool for this session."
        : "Action required: install at least one supported automation tool.",
      autoReady ? `Using recommendation: ${tools.recommended}` : "Please install wtype, ydotool, or xdotool",
    );
    commandShortcutField(summary, "Detected", "wtype: type and paste on Wayland · ydotool: type and paste on Wayland/X11 (requires ydotoold) · xdotool: type and paste on X11 · wl-clipboard: clipboard copy/paste data only.", [
      tools.wl_clipboard && "wl-clipboard",
      tools.wtype && "wtype",
      tools.ydotool && `ydotool${tools.ydotool_daemon ? " (ready)" : " (daemon not running)"}`,
      tools.xdotool && "xdotool",
    ].filter(Boolean).join(", ") || "No supported tools detected");
    const refreshTools = document.createElement("button");
    refreshTools.type = "button";
    refreshTools.className = "shortcut-refresh";
    refreshTools.innerHTML = `${actionIcon("refresh")}<span>Refresh tools</span>`;
    refreshTools.title = "Detect installed Linux automation tools again";
    refreshTools.onclick = () => refreshState().catch((error) => notice(error.message, "error"));
    const summaryHead = summary.querySelector(".group-head");
    summaryHead.insertBefore(refreshTools, summaryHead.querySelector(".group-reset"));
    o.insertBefore(summary, delivery);
  } else {
    const automation = group(o, "Automation & delivery", "Choose how Simple STT inserts text. App overrides can replace this for selected applications.");
    commandShortcutField(automation, "Automation tool", "AutoHotkey handles shortcuts, typing, clipboard preservation, and paste delivery on Windows.", "AutoHotkey · Ready");
    windowsDeliveryChooser(automation);
    o.insertBefore(automation, delivery);
  }
  const pacedTyping = field(delivery, {
    label: "Type gradually",
    description: "Turn off to insert the transcript all at once.",
    path: "output.paced_typing_enabled",
    type: "checkbox",
  });
  const typingSpeed = field(delivery, {
    label: "Speed",
    description: "",
    path: "output.typing_speed_wpm",
    type: "range",
    min: 50,
    max: 850,
    step: 1,
    suffix: " WPM",
  });
  const updateTypingSpeedState = () => {
    typingSpeed.disabled = !pacedTyping.checked;
    typingSpeed.closest(".field").classList.toggle("field-disabled", !pacedTyping.checked);
  };
  pacedTyping.addEventListener("change", updateTypingSpeedState);
  updateTypingSpeedState();
  const overrides = group(o, "App overrides", "Choose a different delivery method only for selected applications.");
  appOverrides(overrides);
  const transforms = group(
    o,
    "Text cleanup",
    "",
  );
  field(transforms, {
    label: "Trailing space",
    description: "Add a space after each transcript.",
    path: "output.trailing_space",
    type: "checkbox",
  });
  field(transforms, {
    label: "Remove punctuation",
    description: "Strip punctuation from recognized text.",
    path: "output.remove_punctuation",
    type: "checkbox",
  });
  field(transforms, {
    label: "Lowercase",
    description: "Convert the transcript to lowercase.",
    path: "output.lowercase",
    type: "checkbox",
  });
  const runtime = group(
    x,
    "Runtime",
    "",
  );
  field(runtime, {
    label: "Runtime directory",
    description: "Relative paths stay portable.",
    path: "speech.runtime_dir",
  });
  field(runtime, {
    label: "Model directory",
    description: "Downloaded GGUF files live here.",
    path: "speech.model_dir",
  });
  field(runtime, {
    label: "Worker idle timeout",
    description: "Seconds before RAM and VRAM are released.",
    path: "speech.idle_worker_timeout_secs",
    type: "number",
  });
  field(runtime, {
    label: "Shutdown grace",
    description: "Milliseconds allowed for a clean exit.",
    path: "speech.worker_shutdown_grace_ms",
    type: "number",
  });
  const diagnostics = group(
    x,
    "Diagnostics",
    "",
  );
  field(diagnostics, {
    label: "Log detail",
    description: "Development builds honor this setting. Released builds always use Minimal to reduce overhead.",
    path: "diagnostics.log_level",
    type: "select",
    options: [
      ["minimal", "Minimal"],
      ["normal", "Normal"],
      ["debug", "Debug"],
      ["extreme", "Extreme"],
    ],
  });
  field(diagnostics, {
    label: "Diagnostic overlay",
    description: "Show additional runtime details.",
    path: "diagnostics.diagnostic_overlay",
    type: "checkbox",
  });
  field(diagnostics, {
    label: "Save transcript text in logs",
    description: "Off by default for privacy. When off, logs contain only character counts; enable it temporarily only when needed.",
    path: "diagnostics.log_transcripts",
    type: "checkbox",
  });
  $("#configure-shortcuts").hidden = true;
  const paths = document.createElement("div");
  paths.className = "path-card";
  paths.innerHTML = `<strong>Resolved locations</strong><p>Runtime: ${state.resolved_runtime_dir || "Unavailable"}</p><p>Models: ${state.resolved_model_dir || "Unavailable"}</p>`;
  x.append(paths);
  if (
    config.audio.preferred_device_id &&
    !state.microphones.some((m) => m.id === config.audio.preferred_device_id)
  )
    notice(
      "Your preferred microphone is unavailable. The system default is active until it returns.",
      "warning",
    );
  renderSettingsSearch();
  syncJson();
}
function renderModels() {
  const root = $("#model-workbench");
  root.replaceChildren();
  const head = document.createElement("div");
  head.className = "group-head";
  head.innerHTML =
    "<div><h2>Speech model</h2></div>";
  const refresh = document.createElement("button");
  refresh.id = "refresh-models";
  refresh.type = "button";
  refresh.className = "icon-button";
  refresh.innerHTML = actionIcon("refresh");
  refresh.setAttribute("aria-label", "Refresh model catalog");
  refresh.title = "Refresh model catalog";
  refresh.onclick = () => action("refresh_models").then(refreshState);
  head.append(refresh);
  root.append(document.createRange().createContextualFragment(groupIcon("Speech model")), head);
  const languageText = (m) =>
      (m.languages || []).join(", ") ||
      "Language not declared for this local model",
    languagePreview = (m) =>
      (m.languages || []).length > 3
        ? `${m.languages.length} languages · English, Spanish, French, German, and more`
        : languageText(m);
  const searchField = document.createElement("label");
  searchField.className = "model-search-field";
  searchField.dataset.settingPath = "speech.selected_model_filename";
  searchField.innerHTML = "<strong>Find a model</strong>";
  const search = document.createElement("input");
  search.id = "model-search";
  search.type = "search";
  search.placeholder = "Try “v2 en” or “Spanish q4”…";
  search.autocomplete = "off";
  search.setAttribute("aria-label", "Search language or model");
  search.setAttribute("aria-controls", "model-results");
  search.value = modelSearchQuery;
  searchField.append(search);
  const results = document.createElement("div");
  results.id = "model-results";
  results.className = "model-results";
  root.append(searchField, results);
  let visibleModelLimit = 8;

  function drawResults() {
    const terms = search.value
      .toLowerCase()
      .trim()
      .split(/\s+/)
      .filter(Boolean);
    const matches = state.models.filter((model) => {
      const searchable = [
        model.family,
        model.quant,
        model.file,
        ...(model.languages || []),
        model.installed ? "installed downloaded local" : "available download",
        model.recommended ? "recommended" : "",
        model.file === config.speech.selected_model_filename ? "selected" : "",
      ]
        .join(" ")
        .toLowerCase();
      return terms.every((term) => searchable.includes(term));
    }).sort((a, b) =>
      Number(b.recommended) - Number(a.recommended) ||
      Number(b.installed) - Number(a.installed) ||
      a.family.localeCompare(b.family),
    );
    results.replaceChildren();
    if (!matches.length) {
      const empty = document.createElement("p");
      empty.className = "model-empty";
      empty.textContent = "No models match that search.";
      results.append(empty);
      return;
    }
    const visibleMatches = terms.length ? matches : matches.slice(0, visibleModelLimit);
    for (const model of visibleMatches) {
      const row = document.createElement("article");
      row.className = "model-result";
      if (model.file === config.speech.selected_model_filename)
        row.dataset.selected = "true";
      const copy = document.createElement("div");
      copy.className = "model-result-copy";
      copy.innerHTML = `<strong>${model.family} · ${model.quant}</strong><span>${languagePreview(model)} · ${model.file}${model.size_mb ? ` · ${model.size_mb} MB` : ""}</span>`;
      const badges = document.createElement("div");
      badges.className = "model-badges";
      if (model.recommended) badges.innerHTML += "<span>Recommended</span>";
      if (model.installed) badges.innerHTML += "<span>Installed</span>";
      copy.append(badges);
      const actions = document.createElement("div");
      actions.className = "model-actions";
      const primary = document.createElement("button");
      primary.type = "button";
      const downloadState = modelDownloads.get(model.file);
      if (downloadState) {
        primary.innerHTML = actionIcon("download");
        primary.className = "icon-button";
        primary.setAttribute("aria-label", `Downloading ${model.family}`);
        primary.title = "Downloading";
        primary.disabled = true;
        primary.dataset.state = "loading";
      } else if (!model.installed) {
        primary.innerHTML = actionIcon("download");
        primary.className = "icon-button";
        primary.setAttribute("aria-label", `Download ${model.family} ${model.quant}`);
        primary.title = "Download model";
        primary.onclick = () => download(model.file);
      } else if (model.file === config.speech.selected_model_filename) {
        primary.innerHTML = `${actionIcon("check")}Selected`;
        primary.disabled = true;
      } else {
        primary.innerHTML = `${actionIcon("check")}Select`;
        primary.className = "primary";
        primary.onclick = () => {
          set("speech.selected_model_filename", model.file);
          drawResults();
        };
      }
      actions.append(primary);
      if (model.file === config.speech.selected_model_filename) {
        const test = document.createElement("button");
        test.type = "button";
        test.className = "icon-button";
        test.innerHTML = actionIcon("test");
        test.setAttribute("aria-label", "Test selected model");
        test.disabled = !state.service_online || dirty;
        test.title = dirty
          ? "Save your selection before testing"
          : "Run the bundled speech sample";
        test.onclick = () => action("test_model", model.file);
        actions.append(test);
      }
      if (model.installed) {
        const remove = document.createElement("button");
        remove.type = "button";
        remove.className = "icon-button danger-quiet";
        remove.innerHTML = actionIcon("trash");
        remove.setAttribute(
          "aria-label",
          `Remove ${model.family} ${model.quant}`,
        );
        remove.title =
          model.file === config.speech.selected_model_filename
            ? "Select another model before removing this one"
            : "Remove downloaded model";
        remove.disabled = model.file === config.speech.selected_model_filename;
        remove.onclick = async () => {
          if (
            !window.confirm(
              `Remove ${model.family} · ${model.quant} from this computer?`,
            )
          )
            return;
          await action("remove_model", model.file);
          notice("Model removed.", "success");
          await refreshState();
        };
        actions.append(remove);
      }
      row.append(copy, actions);
      if (downloadState) {
        const downloaded = downloadState.downloaded || 0,
          total = downloadState.total || 0,
          percent = total ? Math.floor((downloaded * 100) / total) : 0,
          progressWrap = document.createElement("div");
        progressWrap.className = "model-download-progress";
        progressWrap.innerHTML = `<progress class="download-progress" max="100" value="${percent}"></progress><span>${total ? `${percent}%` : `${Math.floor(downloaded / 1048576)} MB`}</span>`;
        row.append(progressWrap);
      }
      results.append(row);
    }
    if (!terms.length && matches.length > visibleMatches.length) {
      const more = document.createElement("button");
      const remaining = matches.length - visibleMatches.length;
      more.type = "button";
      more.className = "model-view-more";
      more.textContent = "View more";
      more.setAttribute("aria-label", `View more models (${remaining} remaining)`);
      more.onclick = () => {
        visibleModelLimit += 8;
        drawResults();
      };
      results.append(more);
    }
  }
  search.oninput = () => {
    modelSearchQuery = search.value;
    visibleModelLimit = 8;
    drawResults();
  };
  drawResults();
}
async function captureHotkey(path, label) {
  const dialog = $("#capture-dialog");
  $("#capture-current").textContent = "Listening…";
  dialog.showModal();
  try {
    const result = await api("/api/hotkey-capture", {
      method: "POST",
      body: "{}",
    });
    dialog.close();
    const duplicate = [
      "general.record_hotkey",
      "general.cancel_hotkey",
      "general.toggle_delivery_hotkey",
    ].some((p) => p !== path && get(p) === result.hotkey);
    if (duplicate)
      throw new Error(
        "That shortcut is already assigned. Choose another chord.",
      );
    set(path, result.hotkey);
    build();
    notice(`${label} shortcut recorded. Save to apply it.`, "success");
  } catch (e) {
    dialog.close();
    notice(e.message, "error");
  }
}
function markDirty(sync = true) {
  dirty = true;
  $("#savebar").hidden = false;
  $("#dirty").textContent = "Unsaved changes";
  if (sync) syncJson();
}
function syncJson() {
  if ($("#json")) $("#json").value = JSON.stringify(config, null, 2) + "\n";
}
function notice(message, type = "info") {
  clearTimeout(noticeTimer);
  const node = $("#notice");
  node.textContent = message;
  node.dataset.state = type;
  node.hidden = false;
  noticeTimer = setTimeout(
    () => (node.hidden = true),
    type === "error" ? 10000 : 6000,
  );
}
async function load() {
  state = await api("/api/state");
  baseline = state.config_hash;
  if (state.config) config = structuredClone(state.config);
  else {
    config = (await api("/api/defaults")).config;
    notice(`Config is malformed and preserved: ${state.config_error}`, "error");
  }
  const service = $("#service");
  service.lastChild.textContent = state.service_online
    ? "Capture service connected"
    : "Offline editing";
  service.dataset.state = state.service_online ? "online" : "offline";
  build();
  if (state.service_online) eventLoop();
}
async function save() {
  try {
    const r = await api("/api/save", {
      method: "POST",
      body: JSON.stringify({ config, expected_hash: baseline }),
    });
    config = r.config;
    baseline = r.config_hash;
    dirty = false;
    $("#savebar").hidden = true;
    notice(
      r.reloaded ? "Saved and applied." : "Saved. Capture service is offline.",
      "success",
    );
    build();
  } catch (e) {
    notice(e.message, "error");
  }
}
async function action(name, filename = "") {
  if (!state.service_online) {
    notice("The capture service is offline.", "error");
    return;
  }
  try {
    const r = await api("/api/action", {
      method: "POST",
      body: JSON.stringify({ action: name, filename }),
    });
    notice(r.message, "success");
    return r;
  } catch (e) {
    notice(e.message, "error");
    throw e;
  }
}
async function download(file) {
  modelDownloads.set(file, { downloaded: 0, total: 0 });
  renderModels();
  try {
    await action("download_model", file);
  } catch (error) {
    modelDownloads.delete(file);
    renderModels();
    throw error;
  }
}
async function eventLoop() {
  while (streaming) {
    try {
      const r = await fetch(`/api/events?after=${lastSeq}`, {
          headers: { "X-Simple-STT-Token": token },
        }),
        line = (await r.text()).split("\n").find((v) => v.startsWith("data: "));
      if (!line) continue;
      for (const e of JSON.parse(line.slice(6)).events) {
        lastSeq = Math.max(lastSeq, e.seq);
        if (e.kind === "model_download_progress") {
          const d = Number(e.values.downloaded || 0),
            t = Number(e.values.total || 0),
            file = e.values.filename;
          modelDownloads.set(file, { downloaded: d, total: t });
          renderModels();
        }
        if (e.kind === "model_download_complete") {
          modelDownloads.delete(e.values.filename);
          notice("Model downloaded and ready.", "success");
          await refreshState();
        }
        if (e.kind === "model_download_failed") {
          modelDownloads.delete(e.values.filename);
          renderModels();
        }
        if (e.kind === "configuration_reloaded") {
          await refreshState(true);
        }
        if (e.text) notice(e.text);
      }
    } catch (e) {
      await new Promise((r) => setTimeout(r, 1000));
    }
  }
}
async function refreshState(reloadConfig = false) {
  const nextState = await api("/api/state");
  if (reloadConfig && nextState.config) {
    if (dirty) {
      config.output.linux_automation_backend = nextState.config.output.linux_automation_backend;
      config.output.delivery_mode = nextState.config.output.delivery_mode;
      baseline = nextState.config_hash;
      syncJson();
    } else {
      config = structuredClone(nextState.config);
      baseline = nextState.config_hash;
    }
  }
  state = nextState;
  build();
}
function showPage(page) {
  $$("nav button").forEach((button) =>
    button.classList.toggle("active", button.dataset.page === page),
  );
  $$("main section").forEach(
    (section) => (section.hidden = section.dataset.section !== page),
  );
}
function fuzzyScore(query, text) {
  const haystack = text.toLowerCase();
  let total = 0;
  for (const term of query.toLowerCase().trim().split(/\s+/).filter(Boolean)) {
    const direct = haystack.indexOf(term);
    if (direct >= 0) {
      total += 100 - Math.min(direct, 80);
      continue;
    }
    let cursor = 0;
    for (const character of haystack) {
      if (character === term[cursor]) cursor += 1;
      if (cursor === term.length) break;
    }
    if (cursor !== term.length) return -1;
    total += 25;
  }
  return total;
}
function settingsIndex() {
  return $$("[data-setting-path]").map((node) => {
    const page =
      node.closest("section[data-section]")?.dataset.section || "general";
    const title =
      node.querySelector(".field-copy strong, :scope > strong")?.textContent ||
      node.dataset.settingPath;
    const description =
      node.querySelector(".field-copy small, :scope > small")?.textContent ||
      "";
    return {
      page,
      path: node.dataset.settingPath,
      title,
      description,
      text: `${title} ${description} ${page} ${node.dataset.settingPath.replaceAll("_", " ")}`,
    };
  });
}
function renderSettingsSearch() {
  const input = $("#settings-search"),
    results = $("#settings-search-results");
  if (!input || !results) return;
  const query = input.value.trim();
  results.replaceChildren();
  if (!query) {
    results.hidden = true;
    input.setAttribute("aria-expanded", "false");
    return;
  }
  const matches = settingsIndex()
    .map((item) => ({ ...item, score: fuzzyScore(query, item.text) }))
    .filter((item) => item.score >= 0)
    .sort((a, b) => b.score - a.score || a.title.localeCompare(b.title))
    .slice(0, 8);
  if (!matches.length) {
    const empty = document.createElement("div");
    empty.className = "settings-search-empty";
    empty.textContent = "No settings found";
    results.append(empty);
  }
  for (const item of matches) {
    const option = document.createElement("button");
    option.type = "button";
    option.role = "option";
    option.className = "settings-search-option";
    option.innerHTML = "<strong></strong><span></span><small></small>";
    option.children[0].textContent = item.title;
    option.children[1].textContent = item.description;
    option.children[2].textContent =
      item.page === "audio" ? "Audio & models" : item.page;
    option.onclick = () => {
      showPage(item.page);
      input.value = "";
      renderSettingsSearch();
      requestAnimationFrame(() => {
        const target = $$("[data-setting-path]").find(
          (node) => node.dataset.settingPath === item.path,
        );
        target?.scrollIntoView({ behavior: "smooth", block: "center" });
        target
          ?.querySelector("input, select, button")
          ?.focus({ preventScroll: true });
      });
    };
    results.append(option);
  }
  results.hidden = false;
  input.setAttribute("aria-expanded", "true");
}
$$("nav button").forEach((button) => {
  button.onclick = () => showPage(button.dataset.page);
});
$("#settings-search").oninput = renderSettingsSearch;
$("#settings-search").onfocus = renderSettingsSearch;
$("#settings-search").onkeydown = (event) => {
  if (event.key === "Escape") {
    event.currentTarget.value = "";
    renderSettingsSearch();
  }
  if (event.key === "ArrowDown") {
    event.preventDefault();
    $("#settings-search-results button")?.focus();
  }
};
document.addEventListener("pointerdown", (event) => {
  if (!event.target.closest(".settings-search")) {
    $("#settings-search-results").hidden = true;
    $("#settings-search").setAttribute("aria-expanded", "false");
  }
});
$("#settings").onsubmit = (e) => {
  e.preventDefault();
  save();
};
$("#configure-shortcuts").onclick = () =>
  api("/api/platform-action", {
    method: "POST",
    body: JSON.stringify({ action: "configure_shortcuts" }),
  })
    .then((r) => notice(r.message, "success"))
    .catch((e) => notice(e.message, "error"));
$("#reset").onclick = async () => {
  config = (await api("/api/defaults")).config;
  build();
  markDirty();
  notice("Defaults previewed. Save to write them.");
};
$("#reload").onclick = load;
$("#copy").onclick = () =>
  navigator.clipboard
    .writeText($("#json").value)
    .then(() => notice("JSON copied.", "success"));
$("#export").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(
    new Blob([$("#json").value], { type: "application/json" }),
  );
  a.download = "simple-stt-config.json";
  a.click();
  URL.revokeObjectURL(a.href);
};
$("#import").onclick = () => $("#import-file").click();
$("#import-file").onchange = async (e) => {
  try {
    config = (
      await api("/api/normalize", {
        method: "POST",
        body: await e.target.files[0].text(),
        headers: { "Content-Type": "application/json" },
      })
    ).config;
    build();
    markDirty();
    notice("Import previewed. Save to write it.");
  } catch (x) {
    notice(x.message, "error");
  }
};
$("#json").oninput = () => {
  try {
    config = JSON.parse($("#json").value);
    markDirty(false);
  } catch {
    dirty = true;
    $("#savebar").hidden = false;
    $("#dirty").textContent = "Invalid JSON";
  }
};
$("#open-config").onclick = () =>
  api("/api/platform-action", {
    method: "POST",
    body: JSON.stringify({ action: "open_config" }),
  });
$("#open-folder").onclick = () =>
  api("/api/platform-action", {
    method: "POST",
    body: JSON.stringify({ action: "open_config_folder" }),
  });
$("#close").onclick = () =>
  api("/api/close", { method: "POST" }).finally(() => window.close());
load().catch((e) => notice(e.message, "error"));

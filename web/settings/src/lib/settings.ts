import type { FieldSpec, Page, DeliveryMode } from "./types";
export const pages: { id: Page; name: string; icon: string }[] = [
  { id: "general", name: "General", icon: "general" },
  { id: "audio", name: "Recognition", icon: "audio" },
  { id: "models", name: "Models", icon: "models" },
  { id: "output", name: "Output", icon: "output" },
  { id: "cleanup", name: "AI cleanup", icon: "cleanup" },
  { id: "advanced", name: "Advanced", icon: "advanced" },
  { id: "config", name: "Config", icon: "config" },
];
export const deliveryOptions: [DeliveryMode, string][] = [
  ["smart_paste", "Smart Paste"],
  ["type", "Type"],
  ["clipboard", "Clipboard only"],
  ["paste_shift_insert", "Shift+Insert"],
  ["paste_ctrl_shift_v", "Ctrl+Shift+V"],
  ["paste_ctrl_v", "Ctrl+V"],
];
export const fields: Record<string, FieldSpec> = {
  enabled: { path: "general.enabled", label: "Simple STT", type: "checkbox" },
  shortcutSystem: {
    path: "general.linux_hotkey_backend",
    label: "Shortcut system",
    type: "select",
    help: "Automatic uses the desktop portal on Wayland and native X11 grabs on X11.",
    options: [
      ["auto", "Automatic"],
      ["portal", "Desktop portal"],
      ["x11", "Native X11"],
      ["desktop", "Desktop commands"],
    ],
  },
  recordingMode: {
    path: "general.recording_mode",
    label: "Recording mode",
    type: "select",
    options: [
      ["hold", "Hold to record"],
      ["toggle", "Press to start / stop"],
    ],
  },
  caps: {
    path: "general.capslock_behavior",
    label: "Caps Lock tap",
    type: "select",
    options: [
      ["preserve_tap", "Preserve tap"],
      ["always_off", "Always off"],
    ],
  },
  startup: {
    path: "general.start_at_login",
    label: "Start at login",
    type: "checkbox",
  },
  theme: {
    path: "general.ui_theme",
    label: "Theme",
    type: "select",
    options: [
      ["auto", "System"],
      ["light", "Light"],
      ["dark", "Dark"],
    ],
  },
  gain: {
    path: "audio.gain",
    label: "Gain",
    type: "number",
    min: 0,
    max: 10,
    step: 0.1,
    help: "1.0 leaves microphone volume unchanged.",
  },
  device: {
    path: "speech.inference_device",
    label: "Device",
    type: "select",
    options: [
      ["auto", "Automatic"],
      ["gpu", "GPU · Vulkan"],
      ["cpu", "CPU"],
    ],
  },
  paced: {
    path: "output.paced_typing_enabled",
    label: "Gradual typing",
    type: "checkbox",
    help: "Turn off to insert the whole transcript at once.",
  },
  speed: {
    path: "output.typing_speed_wpm",
    label: "Speed",
    type: "range",
    min: 50,
    max: 850,
    step: 1,
    suffix: " WPM",
  },
  space: {
    path: "output.trailing_space",
    label: "Trailing space",
    type: "checkbox",
  },
  punctuation: {
    path: "output.remove_punctuation",
    label: "Remove punctuation",
    type: "checkbox",
  },
  preserveClipboard: {
    path: "output.preserve_clipboard",
    label: "Keep clipboard clean",
    type: "checkbox",
    help: "Insert without the clipboard where supported; otherwise exclude dictation from supported clipboard histories. Restore previous contents only after confirmed insertion; otherwise keep the text available safely. Some clipboard managers ignore exclusion hints. Clipboard-only mode always copies normally.",
  },
  lowercase: { path: "output.lowercase", label: "Lowercase", type: "checkbox" },
  cleanup: {
    path: "cleanup.enabled",
    label: "Clean dictated text",
    type: "checkbox",
  },
  provider: {
    path: "cleanup.provider",
    label: "Provider",
    type: "select",
    options: [
      ["open_ai_compatible", "OpenAI-compatible"],
      ["chat_gpt", "ChatGPT"],
    ],
  },
  url: { path: "cleanup.openai_compatible.base_url", label: "Base URL" },
  timeout: {
    path: "cleanup.timeout_ms",
    label: "Timeout · ms",
    type: "number",
    min: 15000,
    max: 120000,
    help: "On failure or timeout, the original transcript is delivered.",
  },
  tokens: {
    path: "cleanup.max_output_tokens",
    label: "Output limit",
    type: "number",
    min: 64,
    max: 32768,
    help: "Maximum generated tokens.",
  },
  prompt: {
    path: "cleanup.prompt",
    label: "Instructions",
    type: "textarea",
    help: "Transcript and screen text are always treated as untrusted content.",
  },
  screenshot: {
    path: "cleanup.screenshot.enabled",
    label: "Screen context",
    type: "checkbox",
    help: "An explicit privacy indicator appears during screenshot capture.",
  },
  scope: {
    path: "cleanup.screenshot.scope",
    label: "Capture",
    type: "select",
    options: [
      ["active_window", "Active window"],
      ["full_screen", "Full screen"],
    ],
  },
  imageSize: {
    path: "cleanup.screenshot.max_edge_pixels",
    label: "Image size · px",
    type: "number",
    min: 320,
    max: 4096,
  },
  quality: {
    path: "cleanup.screenshot.jpeg_quality",
    label: "JPEG quality",
    type: "number",
    min: 20,
    max: 100,
  },
  runtime: {
    path: "speech.runtime_dir",
    label: "Runtime directory",
    help: "Relative paths keep the installation portable.",
  },
  modelDir: { path: "speech.model_dir", label: "Model directory" },
  idle: {
    path: "speech.idle_worker_timeout_secs",
    label: "Worker idle · sec",
    type: "number",
    min: 0,
    help: "Worker exit releases model RAM and VRAM.",
  },
  grace: {
    path: "speech.worker_shutdown_grace_ms",
    label: "Shutdown grace · ms",
    type: "number",
    min: 0,
  },
  logs: {
    path: "diagnostics.log_level",
    label: "Log detail",
    type: "select",
    options: [
      ["minimal", "Minimal"],
      ["normal", "Normal"],
      ["debug", "Debug"],
      ["extreme", "Extreme"],
    ],
    help: "Release builds always use Minimal.",
  },
  overlay: {
    path: "diagnostics.diagnostic_overlay",
    label: "Diagnostic overlay",
    type: "checkbox",
  },
  transcripts: {
    path: "diagnostics.log_transcripts",
    label: "Log transcripts",
    type: "checkbox",
    help: "Off by default. Enable temporarily only when troubleshooting.",
  },
};
export const hotkeys = [
  ["general.record_hotkey", "Record"],
  ["general.cancel_hotkey", "Cancel"],
  ["general.toggle_delivery_hotkey", "Switch delivery"],
  ["general.toggle_cleanup_hotkey", "Toggle AI cleanup"],
  ["general.retry_delivery_hotkey", "Retry last dictation"],
] as const;
const pageFor = (path: string): Page =>
  path.startsWith("diagnostics.") ||
  [
    "speech.runtime_dir",
    "speech.model_dir",
    "speech.idle_worker_timeout_secs",
    "speech.worker_shutdown_grace_ms",
  ].includes(path)
    ? "advanced"
    : path.startsWith("speech.") || path.startsWith("audio.")
      ? "audio"
      : path.startsWith("general.")
        ? "general"
        : path.startsWith("output.")
          ? "output"
          : "cleanup";
export const searchEntries = [
  ...Object.values(fields).map((f) => ({ ...f, page: pageFor(f.path) })),
  ...hotkeys.map(([path, label]) => ({ path, label, page: "general" as Page })),
  {
    path: "speech.selection_mode",
    label: "Model selection · keyboard languages",
    page: "audio" as Page,
  },
  {
    path: "audio.preferred_device_id",
    label: "Microphone",
    page: "audio" as Page,
  },
  {
    path: "output.app_overrides",
    label: "App overrides",
    page: "output" as Page,
  },
  {
    path: "output.linux_delivery_cycle",
    label: "Delivery · tools · paste",
    page: "output" as Page,
  },
  {
    path: "cleanup.screenshot.excluded_apps",
    label: "Never capture",
    page: "cleanup" as Page,
  },
  {
    path: "cleanup.openai_compatible.model",
    label: "Cleanup model",
    page: "cleanup" as Page,
  },
];
export function fuzzyScore(query: string, text: string): number {
  let score = 0;
  for (const term of query.toLowerCase().trim().split(/\s+/).filter(Boolean)) {
    const value = text.toLowerCase();
    const direct = value.indexOf(term);
    if (direct >= 0) {
      score += 100 - Math.min(direct, 80);
      continue;
    }
    let i = 0;
    for (const c of value) if (c === term[i]) i++;
    if (i !== term.length) return -1;
    score += 5;
  }
  return score;
}

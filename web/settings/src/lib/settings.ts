import type { FieldSpec, Page, DeliveryMode } from "./types";
export const pages: { id: Page; name: string; icon: string }[] = [
  { id: "general", name: "ui.general.9239ee", icon: "general" },
  { id: "audio", name: "ui.recognition.343bc8", icon: "audio" },
  { id: "models", name: "ui.models.f3798f", icon: "models" },
  { id: "output", name: "ui.output.4bed33", icon: "output" },
  { id: "cleanup", name: "ui.ai_cleanup.05cb5b", icon: "cleanup" },
  { id: "advanced", name: "ui.advanced.4d0647", icon: "advanced" },
  { id: "config", name: "ui.config.885114", icon: "config" },
];
export const deliveryOptions: [DeliveryMode, string][] = [
  ["smart_paste", "ui.smart_paste.883633"],
  ["type", "ui.type.3deb74"],
  ["clipboard", "ui.clipboard_only.3e5459"],
  ["paste_shift_insert", "ui.shift_insert.668fd0"],
  ["paste_ctrl_shift_v", "ui.ctrl_shift_v.a939f7"],
  ["paste_ctrl_v", "ui.ctrl_v.652c7d"],
];
export const fields: Record<string, FieldSpec> = {
  enabled: { path: "general.enabled", label: "ui.simple_stt.d84a2c", type: "checkbox" },
  shortcutSystem: {
    path: "general.linux_hotkey_backend",
    label: "ui.shortcut_system.7d97bc",
    type: "select",
    help: "ui.automatic_uses_the_desktop_portal.f92b7c",
    options: [
      ["auto", "ui.automatic.ac9041"],
      ["portal", "ui.desktop_portal.543c53"],
      ["x11", "ui.native_x11.93e28b"],
      ["desktop", "ui.desktop_commands.166a7d"],
    ],
  },
  recordingMode: {
    path: "general.recording_mode",
    label: "ui.recording_mode.db806e",
    type: "select",
    options: [
      ["hold", "ui.hold_to_record.89b827"],
      ["toggle", "ui.press_to_start_stop.49a7fd"],
    ],
  },
  caps: {
    path: "general.capslock_behavior",
    label: "ui.caps_lock_tap.b8818e",
    type: "select",
    options: [
      ["preserve_tap", "ui.preserve_tap.6e8543"],
      ["always_off", "ui.always_off.bae7fc"],
    ],
  },
  startup: {
    path: "general.start_at_login",
    label: "ui.start_at_login.4c0a93",
    type: "checkbox",
  },
  theme: {
    path: "general.ui_theme",
    label: "ui.theme.a797e3",
    type: "select",
    options: [
      ["auto", "ui.system.bc0792"],
      ["light", "ui.light.a36ef8"],
      ["dark", "ui.dark.ae1ef0"],
    ],
  },
  gain: {
    path: "audio.gain",
    label: "ui.gain.96dd91",
    type: "number",
    min: 0,
    max: 10,
    step: 0.1,
    help: "ui.1_0_leaves_microphone_volume.a2aee0",
  },
  device: {
    path: "speech.inference_device",
    label: "ui.device.a5a74a",
    type: "select",
    options: [
      ["auto", "ui.automatic.ac9041"],
      ["gpu", "ui.gpu_vulkan.7c5e57"],
      ["cpu", "ui.cpu.ff221d"],
    ],
  },
  paced: {
    path: "output.paced_typing_enabled",
    label: "ui.gradual_typing.e05780",
    type: "checkbox",
    help: "ui.turn_off_to_insert_the.e880c8",
  },
  speed: {
    path: "output.typing_speed_wpm",
    label: "ui.speed.2d2cb0",
    type: "range",
    min: 50,
    max: 850,
    step: 1,
    suffix: "ui.wpm.3fc2c6",
  },
  space: {
    path: "output.trailing_space",
    label: "ui.trailing_space.ae92f6",
    type: "checkbox",
  },
  punctuation: {
    path: "output.remove_punctuation",
    label: "ui.remove_punctuation.100017",
    type: "checkbox",
  },
  preserveClipboard: {
    path: "output.preserve_clipboard",
    label: "ui.keep_clipboard_clean.10b3d3",
    type: "checkbox",
    help: "ui.insert_without_the_clipboard_where.4abefd",
  },
  lowercase: { path: "output.lowercase", label: "ui.lowercase.b563fa", type: "checkbox" },
  cleanup: {
    path: "cleanup.enabled",
    label: "ui.clean_dictated_text.38d788",
    type: "checkbox",
  },
  provider: {
    path: "cleanup.provider",
    label: "ui.provider.7ceee3",
    type: "select",
    options: [
      ["open_ai_compatible", "ui.openai_compatible.b10d74"],
      ["chat_gpt", "ui.chatgpt.495265"],
    ],
  },
  url: { path: "cleanup.openai_compatible.base_url", label: "ui.base_url.1dbd61" },
  timeout: {
    path: "cleanup.timeout_ms",
    label: "ui.timeout_ms.8b6678",
    type: "number",
    min: 15000,
    max: 120000,
    help: "ui.on_failure_or_timeout_the.43084f",
  },
  tokens: {
    path: "cleanup.max_output_tokens",
    label: "ui.output_limit.63e1b0",
    type: "number",
    min: 64,
    max: 32768,
    help: "ui.maximum_generated_tokens.bb5534",
  },
  prompt: {
    path: "cleanup.prompt",
    label: "ui.instructions.ed58f2",
    type: "textarea",
    help: "ui.transcript_and_screen_text_are.beac8a",
  },
  screenshot: {
    path: "cleanup.screenshot.enabled",
    label: "ui.screen_context.2ca2ca",
    type: "checkbox",
    help: "ui.an_explicit_privacy_indicator_appears.e192c8",
  },
  scope: {
    path: "cleanup.screenshot.scope",
    label: "ui.capture.772a4b",
    type: "select",
    options: [
      ["active_window", "ui.active_window.1de2ee"],
      ["full_screen", "ui.full_screen.d15f88"],
    ],
  },
  imageSize: {
    path: "cleanup.screenshot.max_edge_pixels",
    label: "ui.image_size_px.1bc868",
    type: "number",
    min: 320,
    max: 4096,
  },
  quality: {
    path: "cleanup.screenshot.jpeg_quality",
    label: "ui.jpeg_quality.e85408",
    type: "number",
    min: 20,
    max: 100,
  },
  runtime: {
    path: "speech.runtime_dir",
    label: "ui.runtime_directory.5878d0",
    help: "ui.relative_paths_keep_the_installation.d536ab",
  },
  modelDir: { path: "speech.model_dir", label: "ui.model_directory.e60e75" },
  idle: {
    path: "speech.idle_worker_timeout_secs",
    label: "ui.worker_idle_sec.3e9c4b",
    type: "number",
    min: 0,
    help: "ui.worker_exit_releases_model_ram.28bfee",
  },
  grace: {
    path: "speech.worker_shutdown_grace_ms",
    label: "ui.shutdown_grace_ms.ced86b",
    type: "number",
    min: 0,
  },
  logs: {
    path: "diagnostics.log_level",
    label: "ui.log_detail.5a1287",
    type: "select",
    options: [
      ["minimal", "ui.minimal.a711cc"],
      ["normal", "ui.normal.45e118"],
      ["debug", "ui.debug.bd604d"],
      ["extreme", "ui.extreme.55590e"],
    ],
    help: "ui.release_builds_always_use_minimal.0620a8",
  },
  overlay: {
    path: "diagnostics.diagnostic_overlay",
    label: "ui.diagnostic_overlay.29d752",
    type: "checkbox",
  },
  transcripts: {
    path: "diagnostics.log_transcripts",
    label: "ui.log_transcripts.4072a1",
    type: "checkbox",
    help: "ui.off_by_default_enable_temporarily.abcc6f",
  },
};
export const hotkeys = [
  ["general.record_hotkey", "ui.record.1c5413"],
  ["general.cancel_hotkey", "ui.cancel.77dfd2"],
  ["general.toggle_delivery_hotkey", "ui.switch_delivery.02b21d"],
  ["general.toggle_cleanup_hotkey", "ui.toggle_ai_cleanup.c7e3d1"],
  ["general.retry_delivery_hotkey", "ui.retry_last_dictation.a48897"],
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
    path: "general.ui_language",
    label: "ui.language.89b86a",
    page: "general" as Page,
  },
  {
    path: "speech.selection_mode",
    label: "ui.model_selection_keyboard_languages.d7d7f8",
    page: "audio" as Page,
  },
  {
    path: "audio.preferred_device_id",
    label: "ui.microphone.242805",
    page: "audio" as Page,
  },
  {
    path: "output.app_overrides",
    label: "ui.app_overrides.9e577a",
    page: "output" as Page,
  },
  {
    path: "output.linux_delivery_cycle",
    label: "ui.delivery_tools_paste.b60d4c",
    page: "output" as Page,
  },
  {
    path: "cleanup.screenshot.excluded_apps",
    label: "ui.never_capture.91a15d",
    page: "cleanup" as Page,
  },
  {
    path: "cleanup.openai_compatible.model",
    label: "ui.cleanup_model.eddd54",
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

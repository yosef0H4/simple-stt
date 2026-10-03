export type Page =
  "general" | "audio" | "models" | "output" | "cleanup" | "advanced" | "config";
export type DeliveryMode =
  | "smart_paste"
  | "type"
  | "clipboard"
  | "paste_shift_insert"
  | "paste_ctrl_v"
  | "paste_ctrl_shift_v";
export type Backend =
  "auto" | "native" | "wtype" | "ydotool" | "xdotool" | "clipboard_only";
export type Reasoning = "none" | "low" | "medium" | "high" | "xhigh" | "max";
export interface Config {
  schema_version: number;
  general: {
    enabled: boolean;
    recording_mode: "hold" | "toggle";
    record_hotkey: string;
    toggle_delivery_hotkey: string;
    cancel_hotkey: string;
    toggle_cleanup_hotkey: string;
    retry_delivery_hotkey: string;
    linux_hotkey_backend: "auto" | "portal" | "x11" | "desktop";
    capslock_behavior: "preserve_tap" | "always_off";
    start_at_login: boolean;
    ui_theme: "auto" | "light" | "dark";
    ui_language: "auto" | "en" | "ar";
  };
  audio: { preferred_device_id: string; gain: number };
  speech: {
    inference_device: "auto" | "cpu" | "gpu";
    runtime_dir: string;
    model_dir: string;
    selection_mode: "single_model" | "follow_keyboard";
    single_model_filename: string | null;
    language_models: Record<string, string | null>;
    idle_worker_timeout_secs: number;
    worker_shutdown_grace_ms: number;
  };
  output: {
    delivery_mode: DeliveryMode;
    enabled_delivery_modes: DeliveryMode[];
    linux_automation_backend: Backend;
    linux_delivery_cycle: { backend: Backend; mode: DeliveryMode }[];
    app_overrides: { app_id: string; mode: DeliveryMode }[];
    paced_typing_enabled: boolean;
    typing_speed_wpm: number;
    preserve_clipboard: boolean;
    trailing_space: boolean;
    remove_punctuation: boolean;
    lowercase: boolean;
  };
  cleanup: {
    enabled: boolean;
    provider: "open_ai_compatible" | "chat_gpt";
    prompt: string;
    timeout_ms: number;
    max_output_tokens: number;
    openai_compatible: {
      base_url: string;
      model: string;
      reasoning_effort: Reasoning;
    };
    chatgpt: { model: string; reasoning_effort: Reasoning };
    screenshot: {
      enabled: boolean;
      scope: "active_window" | "full_screen";
      excluded_apps: string[];
      max_edge_pixels: number;
      jpeg_quality: number;
    };
  };
  diagnostics: {
    log_level: "minimal" | "normal" | "debug" | "extreme";
    diagnostic_overlay: boolean;
    log_transcripts: boolean;
  };
}
export interface Language {
  id: string;
  name: string;
}
export interface Discovery {
  available: boolean;
  message: string;
  languages: Language[];
}
export interface Model {
  file: string;
  family: string;
  quant: string;
  size_mb: number | null;
  recommended: boolean;
  installed: boolean;
  languages: string[];
  download_url?: string;
}
export interface HistoryEntry {
  raw: string;
  cleaned: string;
  model: string;
  latency_ms: number;
  outcome: string;
}
export interface Tools {
  session?: string;
  desktop?: string;
  distro?: string;
  distro_id?: string;
  native?: boolean;
  wtype?: boolean;
  ydotool?: boolean;
  ydotool_daemon?: boolean;
  xdotool?: boolean;
  wl_clipboard?: boolean;
  recommended?: string;
  start_command?: string;
  stop_command?: string;
}
export interface SettingsState {
  config: Config | null;
  config_hash: string;
  config_path?: string;
  config_error?: string;
  platform: "linux" | "windows" | "other";
  ui_localization: {
    system_locale: "en" | "ar";
    locale: "en" | "ar";
    direction: "ltr" | "rtl";
  };
  service_online: boolean;
  models: Model[];
  microphones: { id: string; name: string }[];
  keyboard_languages?: Discovery;
  shortcut_state?: Record<string, string>;
  linux_hotkeys?: {
    requested: string;
    active: string;
    status: string;
    error?: string;
  };
  linux_automation?: Tools;
  resolved_runtime_dir?: string;
  resolved_model_dir?: string;
  cleanup?: {
    compatible_key_saved?: boolean;
    chatgpt_connected?: boolean;
    auth_status?: { state: string; message?: string; code?: string };
    history: HistoryEntry[];
  };
}
export interface SaveResult {
  config: Config;
  config_hash: string;
  reloaded: boolean;
}
export interface ActionResult {
  message?: string;
  message_id?: string;
  message_args?: Record<string, string | number> | string;
  url?: string;
  code?: string;
  app_id?: string;
  models?: { id: string }[];
  result?: { text: string };
  [key: string]: unknown;
}
export interface ServiceEvent {
  seq: number;
  kind: string;
  text?: string;
  values: Record<string, string>;
}
export interface ComboItem {
  value: string;
  label: string;
  meta?: string;
}
export interface FieldSpec {
  path: string;
  label: string;
  help?: string;
  type?: "text" | "number" | "range" | "checkbox" | "select" | "textarea";
  options?: [string, string][];
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
  disabled?: boolean;
}
export function assertConfig(value: unknown): asserts value is Config {
  if (!value || typeof value !== "object")
    throw Error("Invalid configuration response");
  const c = value as Record<string, unknown>;
  if (c.schema_version !== 9) throw Error("Unsupported configuration schema");
  const shape: Record<string, string[]> = {
    general: [
      "enabled",
      "recording_mode",
      "record_hotkey",
      "toggle_delivery_hotkey",
      "cancel_hotkey",
      "toggle_cleanup_hotkey",
      "retry_delivery_hotkey",
      "linux_hotkey_backend",
      "capslock_behavior",
      "start_at_login",
      "ui_theme",
      "ui_language",
    ],
    audio: ["preferred_device_id", "gain"],
    speech: [
      "inference_device",
      "runtime_dir",
      "model_dir",
      "selection_mode",
      "single_model_filename",
      "language_models",
      "idle_worker_timeout_secs",
      "worker_shutdown_grace_ms",
    ],
    output: [
      "delivery_mode",
      "enabled_delivery_modes",
      "linux_automation_backend",
      "linux_delivery_cycle",
      "app_overrides",
      "paced_typing_enabled",
      "typing_speed_wpm",
      "preserve_clipboard",
      "trailing_space",
      "remove_punctuation",
      "lowercase",
    ],
    cleanup: [
      "enabled",
      "provider",
      "prompt",
      "timeout_ms",
      "max_output_tokens",
      "openai_compatible",
      "chatgpt",
      "screenshot",
    ],
    diagnostics: ["log_level", "diagnostic_overlay", "log_transcripts"],
  };
  for (const [section, keys] of Object.entries(shape)) {
    const part = c[section];
    if (!part || typeof part !== "object" || keys.some((k) => !(k in part)))
      throw Error(`Incomplete ${section} configuration`);
  }
  const at = (path: string): unknown =>
    path
      .split(".")
      .reduce<unknown>(
        (v, k) =>
          v && typeof v === "object"
            ? (v as Record<string, unknown>)[k]
            : undefined,
        c,
      );
  const primitives: Record<string, string[]> = {
    boolean: [
      "general.enabled",
      "general.start_at_login",
      "output.paced_typing_enabled",
      "output.preserve_clipboard",
      "output.trailing_space",
      "output.remove_punctuation",
      "output.lowercase",
      "cleanup.enabled",
      "cleanup.screenshot.enabled",
      "diagnostics.diagnostic_overlay",
      "diagnostics.log_transcripts",
    ],
    number: [
      "audio.gain",
      "speech.idle_worker_timeout_secs",
      "speech.worker_shutdown_grace_ms",
      "output.typing_speed_wpm",
      "cleanup.timeout_ms",
      "cleanup.max_output_tokens",
      "cleanup.screenshot.max_edge_pixels",
      "cleanup.screenshot.jpeg_quality",
    ],
    string: [
      "general.record_hotkey",
      "general.toggle_delivery_hotkey",
      "general.cancel_hotkey",
      "general.toggle_cleanup_hotkey",
      "general.retry_delivery_hotkey",
      "audio.preferred_device_id",
      "speech.runtime_dir",
      "speech.model_dir",
      "cleanup.prompt",
      "cleanup.openai_compatible.base_url",
      "cleanup.openai_compatible.model",
      "cleanup.chatgpt.model",
    ],
  };
  for (const [type, paths] of Object.entries(primitives))
    for (const path of paths) {
      const value = at(path);
      if (
        typeof value !== type ||
        (type === "number" && !Number.isFinite(value))
      )
        throw Error(`Invalid ${path}`);
    }
  const enums: Record<string, string[]> = {
    "general.recording_mode": ["hold", "toggle"],
    "general.linux_hotkey_backend": ["auto", "portal", "x11", "desktop"],
    "general.capslock_behavior": ["preserve_tap", "always_off"],
    "general.ui_theme": ["auto", "light", "dark"],
    "general.ui_language": ["auto", "en", "ar"],
    "speech.inference_device": ["auto", "cpu", "gpu"],
    "speech.selection_mode": ["single_model", "follow_keyboard"],
    "output.delivery_mode": [
      "smart_paste",
      "type",
      "clipboard",
      "paste_shift_insert",
      "paste_ctrl_v",
      "paste_ctrl_shift_v",
    ],
    "output.linux_automation_backend": [
      "auto",
      "native",
      "wtype",
      "ydotool",
      "xdotool",
      "clipboard_only",
    ],
    "cleanup.provider": ["open_ai_compatible", "chat_gpt"],
    "cleanup.openai_compatible.reasoning_effort": [
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ],
    "cleanup.chatgpt.reasoning_effort": [
      "none",
      "low",
      "medium",
      "high",
      "xhigh",
      "max",
    ],
    "cleanup.screenshot.scope": ["active_window", "full_screen"],
    "diagnostics.log_level": ["minimal", "normal", "debug", "extreme"],
  };
  for (const [path, values] of Object.entries(enums))
    if (!values.includes(String(at(path)))) throw Error(`Invalid ${path}`);
  for (const path of [
    "output.enabled_delivery_modes",
    "output.linux_delivery_cycle",
    "output.app_overrides",
    "cleanup.screenshot.excluded_apps",
  ])
    if (!Array.isArray(at(path))) throw Error(`Invalid ${path}`);
  const output = c.output as Config["output"];
  if (
    output.enabled_delivery_modes.some(
      (v) => !enums["output.delivery_mode"].includes(v),
    ) ||
    output.linux_delivery_cycle.some(
      (v) =>
        !v ||
        !enums["output.delivery_mode"].includes(v.mode) ||
        !enums["output.linux_automation_backend"].includes(v.backend),
    ) ||
    output.app_overrides.some(
      (v) =>
        !v ||
        typeof v.app_id !== "string" ||
        !enums["output.delivery_mode"].includes(v.mode),
    )
  )
    throw Error("Invalid delivery choices");
  if (
    (at("cleanup.screenshot.excluded_apps") as unknown[]).some(
      (v) => typeof v !== "string",
    )
  )
    throw Error("Invalid screenshot exclusions");
  const speech = c.speech as Config["speech"];
  if (
    speech.single_model_filename !== null &&
    typeof speech.single_model_filename !== "string"
  )
    throw Error("Invalid single-model assignment");
  if (
    !speech.language_models ||
    typeof speech.language_models !== "object" ||
    Array.isArray(speech.language_models) ||
    Object.values(speech.language_models).some(
      (v) => v !== null && typeof v !== "string",
    )
  )
    throw Error("Invalid language assignments");
}

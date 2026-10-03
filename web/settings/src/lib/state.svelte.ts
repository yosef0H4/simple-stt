import { api, events, ApiError } from "./api";
import { responseMessage, t } from "./i18n";
import { assertConfig } from "./types";
import type {
  Config,
  SettingsState,
  Discovery,
  Page,
  SaveResult,
  ActionResult,
} from "./types";
import { recommendedModel } from "./models";
export const ui = $state({
  config: null as Config | null,
  state: null as SettingsState | null,
  baseline: null as Config | null,
  hash: "",
  dirty: false,
  page: "general" as Page,
  notice: "",
  noticeDetails: "",
  noticeKind: "info",
  saving: false,
  ready: false,
  jsonText: "",
  jsonPending: false,
  jsonInvalid: false,
  downloads: {} as Record<string, { downloaded: number; total: number }>,
  cleanupModels: [] as { id: string }[],
});
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value));
let noticeTimer: ReturnType<typeof setTimeout> | undefined;
export function notice(message: string, kind = "info", details = "") {
  ui.notice = message;
  ui.noticeDetails = details;
  ui.noticeKind = kind;
  clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => (ui.notice = ""), 6500);
}
export function markDirty() {
  ui.dirty =
    ui.jsonInvalid || JSON.stringify(ui.config) !== JSON.stringify(ui.baseline);
}
export function get(path: string): unknown {
  return path
    .split(".")
    .reduce<unknown>(
      (v, k) =>
        v && typeof v === "object"
          ? (v as Record<string, unknown>)[k]
          : undefined,
      ui.config,
    );
}
export function set(path: string, value: unknown) {
  if (!ui.config) return;
  const keys = path.split(".");
  const key = keys.pop()!;
  let target = ui.config as unknown as Record<string, unknown>;
  for (const k of keys) target = target[k] as Record<string, unknown>;
  target[key] = value;
  if (!ui.jsonInvalid) ui.jsonPending = false;
  markDirty();
  if (path === "speech.selection_mode") initializeLanguages();
}
export function initializeLanguages() {
  if (ui.config?.speech.selection_mode !== "follow_keyboard") return;
  for (const language of ui.state?.keyboard_languages?.languages || [])
    if (!Object.hasOwn(ui.config.speech.language_models, language.id))
      ui.config.speech.language_models[language.id] =
        recommendedModel(ui.state?.models || [], language)?.file || null;
  markDirty();
}
export async function refreshLanguages() {
  const discovered = await api<Discovery>("/api/keyboard-languages");
  if (ui.state) ui.state.keyboard_languages = discovered;
  initializeLanguages();
}
export function changedPaths(
  before: unknown,
  after: unknown,
  prefix = "",
): string[] {
  if (Object.is(before, after)) return [];
  if (
    before === null ||
    after === null ||
    typeof before !== "object" ||
    typeof after !== "object" ||
    Array.isArray(before) ||
    Array.isArray(after)
  )
    return JSON.stringify(before) === JSON.stringify(after) ? [] : [prefix];
  const b = before as Record<string, unknown>,
    a = after as Record<string, unknown>;
  return [...new Set([...Object.keys(b), ...Object.keys(a)])].flatMap((k) =>
    changedPaths(b[k], a[k], prefix ? `${prefix}.${k}` : k),
  );
}
export async function refreshState(reloaded = false, signal?: AbortSignal) {
  const next = await api<SettingsState>("/api/state", undefined, signal);
  if (next.config) assertConfig(next.config);
  const discovery = ui.state?.keyboard_languages;
  ui.state = {
    ...next,
    keyboard_languages: discovery || next.keyboard_languages,
  };
  if (reloaded && next.config) {
    if (!ui.dirty) {
      ui.config = clone(next.config);
      ui.baseline = clone(next.config);
      ui.hash = next.config_hash;
    } else if (ui.baseline) {
      const allowed = new Set([
        "general.enabled",
        "output.delivery_mode",
        "cleanup.enabled",
        "output.linux_automation_backend",
      ]);
      const changed = changedPaths(ui.baseline, next.config);
      if (changed.every((p) => allowed.has(p))) {
        for (const p of changed)
          set(
            p,
            p
              .split(".")
              .reduce<unknown>(
                (v, k) => (v as Record<string, unknown>)[k],
                next.config,
              ),
          );
        ui.baseline = clone(next.config);
        ui.hash = next.config_hash;
        markDirty();
      }
    }
  }
  initializeLanguages();
}
export async function load() {
  ui.jsonPending = false;
  ui.jsonInvalid = false;
  ui.dirty = false;
  await refreshState();
  ui.hash = ui.state?.config_hash || "";
  if (ui.state?.config) {
    ui.config = clone(ui.state.config);
    ui.baseline = clone(ui.state.config);
    ui.hash = ui.state.config_hash;
  } else {
    ui.baseline = null;
    ui.config = (await api<{ config: Config }>("/api/defaults")).config;
    notice(t("ui.configuration_preserved_detail.6fe0a0", { detail: ui.state?.config_error || "" }), "error");
  }
  assertConfig(ui.config);
  try {
    await refreshLanguages();
  } catch (e) {
    if (ui.state)
      ui.state.keyboard_languages = {
        available: false,
        message: String(e),
        languages: [],
      };
  }
  initializeLanguages();
  markDirty();
  ui.ready = true;
}
export async function save(retried = false) {
  ui.saving = true;
  try {
    if (ui.jsonPending) {
      const normalized = await api<{ config: Config }>(
        "/api/normalize",
        JSON.parse(ui.jsonText),
      );
      assertConfig(normalized.config);
      ui.config = normalized.config;
    }
    const result = await api<SaveResult>("/api/save", {
      config: ui.config,
      expected_hash: ui.hash,
    });
    assertConfig(result.config);
    ui.config = clone(result.config);
    ui.baseline = clone(result.config);
    if (ui.state) ui.state.config = clone(result.config);
    ui.hash = result.config_hash;
    ui.jsonPending = false;
    ui.jsonInvalid = false;
    markDirty();
    notice(
      t(result.reloaded ? "Saved and applied." : "Saved. Capture service is offline."),
      "success",
    );
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    if (
      !retried &&
      message.includes("config changed outside Settings") &&
      ui.baseline
    ) {
      const latest = await api<SettingsState>("/api/state");
      const allowed = new Set([
        "general.enabled",
        "output.delivery_mode",
        "cleanup.enabled",
      ]);
      const paths = changedPaths(ui.baseline, latest.config);
      if (paths.length && paths.every((p) => allowed.has(p)) && latest.config) {
        ui.hash = latest.config_hash;
        ui.baseline = clone(latest.config);
        return save(true);
      }
    }
    const localized = e instanceof ApiError
      ? responseMessage(e.message_id, e.message_args, "")
      : "";
    notice(localized || t("ui.operation_failed.c4e6ed"), "error", localized ? "" : message);
  } finally {
    ui.saving = false;
  }
}
export async function preview(value: unknown, message: string) {
  ui.jsonPending = false;
  ui.jsonInvalid = false;
  ui.config = (await api<{ config: Config }>("/api/normalize", value)).config;
  assertConfig(ui.config);
  initializeLanguages();
  markDirty();
  notice(message);
}
export async function reset() {
  ui.jsonPending = false;
  ui.jsonInvalid = false;
  ui.config = (await api<{ config: Config }>("/api/defaults")).config;
  markDirty();
  notice(t("ui.defaults_previewed_save_to_apply.d36d8c"));
}
export async function resetPaths(paths: string[]) {
  const defaults = (await api<{ config: Config }>("/api/defaults")).config;
  for (const path of paths)
    set(
      path,
      path
        .split(".")
        .reduce<unknown>((v, k) => (v as Record<string, unknown>)[k], defaults),
    );
  initializeLanguages();
  notice(t("ui.group_reset_save_to_apply.e9aec1"));
}
export async function action(
  action: string,
  filename = "",
  language = "english",
  signal?: AbortSignal,
) {
  if (!ui.state?.service_online) throw Error("Capture service is offline.");
  const r = await api<ActionResult>(
    "/api/action",
    {
      action,
      filename,
      language,
    },
    signal,
  );
  if (r.message_id || r.message) notice(responseMessage(r.message_id, r.message_args, r.message || ""), "success");
  return r;
}
export async function cleanupAction(
  action: string,
  extra: Record<string, unknown> = {},
  signal?: AbortSignal,
) {
  const r = await api<ActionResult>(
    "/api/cleanup-action",
    {
      action,
      config: ui.config,
      ...extra,
    },
    signal,
  );
  if (r.message_id || r.message) notice(responseMessage(r.message_id, r.message_args, r.message || ""), "success");
  return r;
}
export async function platformAction(action: string) {
  const r = await api<ActionResult>("/api/platform-action", { action });
  await refreshState();
  return r;
}
export async function run(task: () => Promise<unknown>) {
  try {
    await task();
  } catch (e) {
    if (!(e instanceof DOMException && e.name === "AbortError"))
      if (e instanceof ApiError) {
        const localized = responseMessage(e.message_id, e.message_args);
        notice(localized || t("ui.operation_failed.c4e6ed"), "error", localized ? "" : e.message);
      } else notice(t("ui.operation_failed.c4e6ed"), "error", e instanceof Error ? e.message : String(e));
  }
}
export async function download(file: string) {
  ui.downloads[file] = { downloaded: 0, total: 0 };
  try {
    await action("download_model", file);
  } catch (e) {
    delete ui.downloads[file];
    throw e;
  }
}
export async function eventLoop(signal: AbortSignal) {
  let seq = 0;
  while (!signal.aborted) {
    try {
      const batch = await events(seq, signal);
      for (const e of batch) {
        seq = Math.max(seq, e.seq);
        if (e.kind === "model_download_progress")
          ui.downloads[e.values.filename] = {
            downloaded: Number(e.values.downloaded || 0),
            total: Number(e.values.total || 0),
          };
        if (
          e.kind === "model_download_complete" ||
          e.kind === "model_download_failed"
        ) {
          delete ui.downloads[e.values.filename];
          await refreshState();
        }
        if (e.kind === "configuration_reloaded") await refreshState(true);
        if (e.text || e.values.message_id) {
          const args = e.values.message_args;
          const localized = responseMessage(e.values.message_id, args, e.text || "");
          notice(localized || e.text || "");
        }
      }
      if (!batch.length) await delay(500, signal);
    } catch {
      if (!signal.aborted) await delay(1000, signal);
    }
  }
}
export function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const done = () => {
      clearTimeout(timer);
      signal.removeEventListener("abort", done);
      resolve();
    };
    const timer = setTimeout(done, ms);
    signal.addEventListener("abort", done, { once: true });
  });
}
export function dispose() {
  clearTimeout(noticeTimer);
}

export function editJson(text: string) {
  ui.jsonText = text;
  ui.jsonPending = true;
  try {
    const value = JSON.parse(text);
    assertConfig(value);
    ui.config = value;
    ui.jsonInvalid = false;
  } catch {
    ui.jsonInvalid = true;
  }
  markDirty();
}

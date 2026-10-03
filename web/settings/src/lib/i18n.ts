import en from "./locales/en.json";
import ar from "./locales/ar.json";
import desktopEn from "./locales/desktop.en.json";
import desktopAr from "./locales/desktop.ar.json";
import { aliases } from "./locales/aliases";
import { ui } from "./state.svelte";

export type Locale = "en" | "ar";
const messages: Record<Locale, Record<string, string>> = {
  en: { ...en, ...desktopEn },
  ar: { ...ar, ...desktopAr },
};

export function locale(): Locale {
  const choice = ui.config?.general.ui_language || "auto";
  return choice === "auto"
    ? ui.state?.ui_localization?.system_locale || "en"
    : choice;
}

export function direction(): "ltr" | "rtl" {
  return locale() === "ar" ? "rtl" : "ltr";
}

export function languageName(name: string): string {
  const clean = name.trim();
  if (/keyboard language$/i.test(clean)) {
    const language = clean.replace(/\s+keyboard language$/i, "");
    return t("ui.language_name.43ea60", { name: languageName(language) });
  }
  const key = clean.toLowerCase().replace(/\s*\([a-z-]+\)\s*$/, "").trim();
  const languageCodes: Record<string, string> = {
    english: "en",
    arabic: "ar",
    spanish: "es",
    italian: "it",
  };
  const code = clean.match(/\(([a-z]{2,3}(?:-[a-z0-9]{2,8})*)\)/i)?.[1] || languageCodes[key] || (/^[a-z]{2,3}(?:-[a-z0-9]{2,8})*$/i.test(clean) ? clean : "");
  if (!code) return hasMessage(name) ? t(name) : name;
  try {
    return new Intl.DisplayNames([locale()], { type: "language" }).of(code) || name;
  } catch {
    return name;
  }
}

function interpolate(value: string, args?: Record<string, string | number>): string {
  return value.replace(/\{([a-z_]+)\}/gi, (match, name: string) =>
    Object.hasOwn(args || {}, name) ? String(args![name]) : match,
  );
}

export function t(key: string, args?: Record<string, string | number>): string {
  const id = aliases[key] || key;
  let value = messages[locale()][id] || messages.en[id] || key;
  return interpolate(value, args);
}

export function tEnglish(key: string, args?: Record<string, string | number>): string {
  const id = aliases[key] || key;
  return interpolate(messages.en[id] || key, args);
}

export function hasMessage(key: string): boolean {
  const id = aliases[key] || key;
  return Object.hasOwn(messages.en, id) || Object.hasOwn(messages.ar, id);
}

export function responseMessage(
  id?: string,
  args?: Record<string, string | number> | string,
  fallback = "",
): string {
  let parsed = args;
  if (typeof parsed === "string") {
    try { parsed = JSON.parse(parsed) as Record<string, string | number>; }
    catch { parsed = {}; }
  }
  return id && hasMessage(id) ? t(id, parsed as Record<string, string | number>) : fallback;
}

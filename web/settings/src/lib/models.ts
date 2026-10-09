import type { Model, Language, Config, ComboItem } from "./types";
export function compatible(model: Model, language: Language): boolean {
  const id = language.id.toLowerCase();
  return (
    model.languages.some((v) => {
      const code = v.toLowerCase().match(/\(([a-z]{2,3})\)/);
      return code ? code[1] === id : v.toLowerCase() === id;
    }) ||
    (id === "ar" && model.file.startsWith("lemura-arabic-asr-lite-"))
  );
}
const compareFiles = (a: Model, b: Model) =>
  a.file < b.file ? -1 : a.file > b.file ? 1 : 0;

export function orderModels(models: Model[], language?: Language): Model[] {
  return [...models].sort(
    (a, b) =>
      Number(b.recommended) - Number(a.recommended) ||
      (language
        ? Number(compatible(b, language)) - Number(compatible(a, language))
        : 0) ||
      Number(/q8/i.test(b.quant)) - Number(/q8/i.test(a.quant)) ||
      (a.size_mb ?? Infinity) - (b.size_mb ?? Infinity) ||
      compareFiles(a, b),
  );
}

export function recommendedModel(
  models: Model[],
  language: Language,
): Model | undefined {
  return orderModels(
    models.filter((model) => model.installed && compatible(model, language)),
    language,
  )[0];
}
export function modelItems(models: Model[], language?: Language): ComboItem[] {
  return [
    { value: "", label: "None" },
    ...orderModels(models.filter((model) => model.installed), language)
      .map((m) => ({
        value: m.file,
        label: `${m.family} · ${m.quant}`,
        meta: `${m.file} ${m.languages.join(" ")}`,
      })),
  ];
}
export function usages(
  config: Config | null,
  file: string,
  languages: Language[],
): string[] {
  const used: string[] = [];
  if (config?.speech.single_model_filename === file) used.push("Use one model");
  for (const [id, f] of Object.entries(config?.speech.language_models || {}))
    if (f === file)
      used.push(
        languages.find((l) => l.id === id)?.name || `${id} keyboard language`,
      );
  return used;
}

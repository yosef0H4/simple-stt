import en from './locales/en.json';
import ar from './locales/ar.json';
import desktopEn from './locales/desktop.en.json';
import desktopAr from './locales/desktop.ar.json';

/** Normalize text for forgiving, order-independent search. */
export function normalizeSearch(value: string): string {
  return value.normalize('NFKD').toLowerCase()
    .replace(/[\u0300-\u036f\u064b-\u065f\u0670\u06d6-\u06ed\u0640]/g, '')
    .replace(/[أإآٱ]/g, 'ا').replace(/[ىي]/g, 'ي');
}

export function matchesSearch(query: string, ...fields: (string | null | undefined)[]): boolean {
  const terms = normalizeSearch(query).trim().split(/\s+/).filter(Boolean);
  const haystack = normalizeSearch(fields.filter(Boolean).join(' '));
  return terms.every(term => haystack.includes(term));
}

export interface SearchModule {
  id: string;
  label: string;
  feature: string;
  engine: string;
  installed: boolean;
  asset: string | null;
  languages?: string[];
  backend?: string;
  recommended?: boolean;
}
export interface SearchAsset { id: string; label: string; kind: string; installed: boolean; available: boolean; directory: string; dependencies: string[] }

/** Status keywords describe the item itself, not an incidental word in a filename. */
export function matchesCatalogSearch(query: string, item: SearchModule, text: string): boolean {
  const words = normalizeSearch(query).trim().split(/\s+/).filter(Boolean);
  const recommended = words.some(word => word === 'recommended' || word === 'موصي');
  const installed = words.some(word => ['installed', 'downloaded', 'مثبت', 'مثبتة'].includes(word));
  return (!recommended || !!item.recommended) && (!installed || item.installed) && matchesSearch(query, text);
}

/** Search metadata stays separate from the visible option label. */
export function moduleSearchText(
  item: SearchModule,
  models: SearchModule[],
  runtimes: SearchModule[],
  assets: SearchAsset[] = [],
  includeStatus = true,
): string {
  const asset = assets.find(entry => entry.id === item.asset);
  const compatibleModels = models.filter(model => model.feature === item.feature && model.engine === item.engine);
  const compatibleRuntimes = runtimes.filter(runtime => runtime.feature === item.feature && runtime.engine === item.engine);
  const compatibleNames = item.languages === undefined ? compatibleModels.flatMap(model => [model.id, model.label]) : [];
  const languages = new Set(item.languages?.length ? item.languages : compatibleModels.flatMap(model => model.languages ?? []));
  const names = [...languages].flatMap(code => {
    try {
      return [code, new Intl.DisplayNames(['en'], {type:'language'}).of(code) || '', new Intl.DisplayNames(['ar'], {type:'language'}).of(code) || ''];
    } catch { return [code]; }
  });
  const backend = [item.backend ?? '', ...compatibleRuntimes.map(runtime => runtime.backend ?? '')].join(' ');
  const descriptor = `${item.id} ${item.label} ${item.engine} ${asset?.id || ''} ${asset?.label || ''}`.toLowerCase();
  const aliases = [
    /paddle/.test(descriptor) ? 'paddle paddleocr paddle ocr بادل' : '',
    /supertonic/.test(descriptor) ? 'supertonic سوبرتونك' : '',
    /kokoro/.test(descriptor) ? 'kokoro كوكورو' : '',
    item.engine.toLowerCase() === 'whisper' ? 'whispered' : '',
    /cuda|gpu|vulkan/.test(backend.toLowerCase()) ? 'gpu graphics معالج رسومي' : '',
    /cpu/.test(backend.toLowerCase()) ? 'cpu processor معالج' : '',
    /cuda/.test(backend.toLowerCase()) ? 'nvidia إنفيديا انفيديا نفيديا' : '',
    includeStatus && item.installed ? 'installed downloaded مثبت مثبتة تم التنزيل' : '',
    includeStatus && (asset?.available || (!item.installed && !item.asset)) ? 'available download متاح متوفرة تنزيل' : '',
    item.recommended ? 'recommended موصى به' : '',
    item.feature === 'tts' ? 'text to speech reading read aloud speak قراءة تحويل النص كلام' : '',
    item.feature === 'ocr' ? 'ocr image text recognition استخراج النص الصور' : '',
    item.feature === 'stt' ? 'speech to text voice recognition dictation املاء التعرف الصوت' : '',
  ];
  const dictionaries: Record<string, string>[] = [en, ar, desktopEn, desktopAr];
  const translatedLabels = [item.label, asset?.label || ''].flatMap(label => dictionaries.map(dictionary => dictionary[label] || ''));
  return [item.id,item.label,item.engine,item.feature,backend,...compatibleNames,...names,...aliases,...translatedLabels,asset?.id,asset?.label,asset?.kind,asset?.directory,...(asset?.dependencies ?? [])].filter(Boolean).join(' ');
}

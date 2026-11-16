import { t, tEnglish } from './i18n';
import { matchesSearch, normalizeSearch } from './search';
import type { ComboItem } from './types';

/** Sherpa's voices.bin sorts upstream F1..F5, then M1..M5. */
export function supertonicVoices(): (ComboItem & { search: string })[] {
  return Array.from({ length: 10 }, (_, index) => {
    const gender = index < 5 ? 'female' : 'male';
    const number = index % 5 + 1;
    const id = `${gender === 'female' ? 'F' : 'M'}${number}`;
    const tone = `voice.${id.toLowerCase()}`;
    return {
      value: `sid:${index}`,
      label: t(`voice.${gender}`, { number }),
      meta: t(tone),
      search: `${id} ${index} ${tEnglish(`voice.${gender}`, { number })} ${tEnglish(tone)}`,
    };
  });
}

/** Gender words are facets: "male" must not also match "female". */
export function matchesSupertonicVoice(query: string, item: ComboItem & { search?: string }): boolean {
  const index = Number(item.value.replace(/^sid:/, ''));
  const terms = normalizeSearch(query)
    .replace(/[٠-٩۰-۹]/g, digit => String(digit.charCodeAt(0) - (digit <= '٩' ? 0x0660 : 0x06f0)))
    .replace(/(female|male|نسائي|رجالي)(?=\d)/g, '$1 ')
    .trim().split(/\s+/).filter(Boolean);
  const male = terms.some(term => term === 'male' || term === 'رجالي');
  const female = terms.some(term => term === 'female' || term === 'نسائي');
  if ((male && (index < 5 || female)) || (female && index >= 5)) return false;
  // With a gender, the number is its visible ordinal. Unqualified numbers keep
  // matching the legacy zero-based speaker IDs.
  if ((male || female) && terms.some(term => /^\d+$/.test(term) && Number(term) !== index % 5 + 1)) return false;
  const remaining = terms.filter(term => !['male', 'female', 'رجالي', 'نسائي'].includes(term));
  return matchesSearch(remaining.join(' '), item.label, item.meta, item.value, item.search);
}

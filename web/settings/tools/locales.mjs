// Canonical UTF-8 catalogs feed TypeScript, Rust and this generated AHK adapter.
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { createHash } from 'node:crypto';
const root = new URL('../../../', import.meta.url);
const directory = new URL('web/settings/src/lib/locales/', root);
const read = (name) => readFileSync(new URL(name, directory), 'utf8');
const placeholders = (text) => [...text.matchAll(/\{([a-z_]+)\}/gi)].map(m => m[1]).sort().join(',');
for (const name of readdirSync(directory).filter(n => n.endsWith('en.json'))) {
  const en = JSON.parse(read(name)), ar = JSON.parse(read(name.replace('en.json', 'ar.json')));
  if (JSON.stringify(Object.keys(en).sort()) !== JSON.stringify(Object.keys(ar).sort())) throw Error(`Locale keys differ: ${name}`);
  for (const key of Object.keys(en)) {
    if (typeof en[key] !== 'string' || typeof ar[key] !== 'string' || !ar[key].trim() || placeholders(en[key]) !== placeholders(ar[key])) throw Error(`Invalid translation: ${key}`);
  }
}
const enText = read('desktop.en.json'), arText = read('desktop.ar.json');
const digest = createHash('sha256').update(enText).update(arText).digest('hex');
const quote = (s) => '"' + s.replaceAll('`', '``').replaceAll('"', '`"').replaceAll('\n', '`n').replaceAll('\r', '`r') + '"';
const map = (raw) => 'Map(\n' + Object.entries(JSON.parse(raw)).map(([key, value]) => '        '+quote(key)+', '+quote(value)).join(',\n') + '\n    )';
const content = `; Generated from desktop catalogs. Run node web/settings/tools/locales.mjs.
; catalog-sha256: ${digest}
UiSetLanguage(language := "auto") {
    global SimpleSttUiLocale
    if language = "auto"
        language := (DllCall("GetUserDefaultUILanguage", "UShort") & 0x3ff) = 1 ? "ar" : "en"
    SimpleSttUiLocale := language = "ar" ? "ar" : "en"
}
UiText(id, args := unset) {
    global SimpleSttUiLocale
    static english := ${map(enText)}
    static arabic := ${map(arText)}
    if !IsSet(SimpleSttUiLocale)
        UiSetLanguage()
    catalog := SimpleSttUiLocale = "ar" ? arabic : english
    text := catalog.Has(id) ? catalog[id] : (english.Has(id) ? english[id] : id)
    output := ""
    offset := 1
    while RegExMatch(text, "\\{([a-zA-Z_]+)\\}", &part, offset) {
        output .= SubStr(text, offset, part.Pos - offset)
        output .= IsSet(args) && args.Has(part[1]) ? args[part[1]] : part[0]
        offset := part.Pos + part.Len
    }
    return output . SubStr(text, offset)
}
`;
const target = new URL('ahk/lib/Locale.ahk', root);
if (process.argv.includes('--check')) {
  if (readFileSync(target, 'utf8') !== content) throw Error('AHK translations are stale; run node web/settings/tools/locales.mjs');
} else writeFileSync(target, content);
console.log('PASS locale keys/placeholders and generated AHK translations');

import { api } from "./api";
import type { ReaderConfig } from "./reader";
export const reading = $state({
  config: null as ReaderConfig | null, baseline: "", speechKey: "", ocrKey: "", raw: "", rawError: "",
});
export function readingDirty() {
  return !!reading.config && (JSON.stringify(reading.config) !== reading.baseline || !!reading.speechKey || !!reading.ocrKey);
}
export async function saveReading() {
  if (!reading.config || !readingDirty()) return;
  if (reading.speechKey || reading.ocrKey) {
    await api("/api/reader-secrets",{speech:reading.speechKey,ocr:reading.ocrKey});
    reading.speechKey="";reading.ocrKey="";
  }
  const result = await api<{config:ReaderConfig}>("/api/reader-save",{config:reading.config,expected_revision:reading.config.revision});
  reading.config=result.config;reading.baseline=JSON.stringify(result.config);reading.raw=JSON.stringify(result.config,null,2);
}

export interface ReadingState { config: ReaderConfig; status: Record<string, unknown> | null; speech_key_saved: boolean; ocr_key_saved: boolean; stt_key_saved?: boolean }
let pending: Promise<ReadingState> | undefined;
export function loadReading(): Promise<ReadingState> {
  return pending ??= api<ReadingState>("/api/reader").then(state => {
    if (!reading.config) {
      const config = state.config;
      config.read_along ??= {prefetch:true,advance_key:"Space",action:"key",delay_ms:900,retry_count:5};
      config.selection_copy ??= {backend:"auto",shortcut:"ctrl_insert",allow_clipboard_copy:true,show_clipboard_notice:true};
      config.selection_copy_overrides ??= [];
      config.speech.selection ??= {mode:"auto_detect",single:{runtime:null,model:null,device:"auto"},languages:{}};
      config.ocr.selection ??= {mode:"single_model",single:{runtime:null,model:null,device:"auto"},languages:{}};
      config.speech.fallback_to_local ??= true; config.ocr.fallback_to_local ??= true;
      reading.config=config; reading.baseline=JSON.stringify(config); reading.raw=JSON.stringify(config,null,2);
    }
    return state;
  }).finally(() => { pending=undefined; });
}

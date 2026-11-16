import {mergeSavedFields} from "./draft";
import { api } from "./api";
import type { ReaderConfig } from "./reader";
export const reading = $state({
  config: null as ReaderConfig | null, baseline: "", speechKey: "", ocrKey: "", raw: "", rawError: "", speechKeySaved:false, ocrKeySaved:false, sttKeySaved:false,
});
export function readingDirty() {
  return !!reading.config && (JSON.stringify(reading.config) !== reading.baseline || !!reading.speechKey || !!reading.ocrKey);
}
export async function saveReading() {
  if (!reading.config || !readingDirty()) return;
  if (reading.speechKey || reading.ocrKey) {
    await api("/api/reader-secrets",{speech:reading.speechKey,ocr:reading.ocrKey});
    if(reading.speechKey)reading.speechKeySaved=true;
    if(reading.ocrKey)reading.ocrKeySaved=true;
    reading.speechKey="";reading.ocrKey="";
  }
  const result = await api<{config:ReaderConfig}>("/api/reader-save",{config:reading.config,expected_revision:reading.config.revision});
  reading.config=result.config;reading.baseline=JSON.stringify(result.config);reading.raw=JSON.stringify(result.config,null,2);
}

export interface ReadingState { config: ReaderConfig; status: Record<string, unknown> | null; speech_key_saved: boolean; ocr_key_saved: boolean; stt_key_saved?: boolean }
let pending: Promise<ReadingState> | undefined;
export function loadReading(): Promise<ReadingState> {
  return pending ??= api<ReadingState>("/api/reader").then(state => {
    reading.speechKeySaved=state.speech_key_saved;reading.ocrKeySaved=state.ocr_key_saved;reading.sttKeySaved=!!state.stt_key_saved;
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

export async function clearReaderKey(kind: "speech"|"ocr") {
  await api("/api/reader-secrets",{[`clear_${kind}`]:true});
  if(kind==="speech"){reading.speechKey="";reading.speechKeySaved=false;}
  else {reading.ocrKey="";reading.ocrKeySaved=false;}
}
export async function saveReadingProvider(kind: "speech"|"ocr") {
  if(!reading.config)return;
  const latest=await api<ReadingState>("/api/reader");
  const config=structuredClone($state.snapshot(latest.config));
  // Persist this connection only; other settings and typed keys stay as drafts.
  Object.assign(config[kind],{provider:JSON.parse(JSON.stringify(reading.config[kind].provider)),fallback_to_local:reading.config[kind].fallback_to_local});
  const result=await api<{config:ReaderConfig}>("/api/reader-save",{config,expected_revision:latest.config.revision});
  const baseline=JSON.parse(reading.baseline);
  mergeSavedFields(reading.config as unknown as Record<string,unknown>,baseline,result.config as unknown as Record<string,unknown>);
  Object.assign(reading.config[kind],{provider:result.config[kind].provider,fallback_to_local:result.config[kind].fallback_to_local});
  reading.baseline=JSON.stringify(result.config);reading.raw=JSON.stringify(reading.config,null,2);
  const key=kind==="speech"?reading.speechKey:reading.ocrKey;
  if(key.trim()) {
    await api("/api/reader-secrets",{[kind]:key});
    if(kind==="speech"){reading.speechKey="";reading.speechKeySaved=true;}
    else {reading.ocrKey="";reading.ocrKeySaved=true;}
  }
}

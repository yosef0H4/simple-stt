export interface RecordedKeyEvent {key:string;down:boolean;at_ms:number}
export interface RecordedAction {events:RecordedKeyEvent[]}
import type {Selection} from "./modules.svelte";
export interface ReaderConfig {
  request_budgets: {connect_timeout_secs: number; queue_timeout_secs: number; cold_model_load_timeout_secs: number; warm_model_load_timeout_secs: number; warm_inference_timeout_secs: number; stream_inactivity_timeout_secs: number; total_timeout_secs: number};
  revision: number;
  speech: { excluded_words?:string[];saved_remote?:ReaderConfig["speech"]["provider"]|null;key_slot?:string|null;remote_fallback?:import("./modules.svelte").Connection|null;selection: Selection; provider: {kind: "local_kokoro" | "local_supertonic" | "open_ai_compatible"; base_url?: string; model?: string; voice?: string; format?: string}; fallback_to_local: boolean; voice: string; language: string; speed: number; device: "auto" | "cpu" | "cuda"; model_dir: string; runtime_dir: string; worker_library: string | null };
  ocr: {saved_remote?:ReaderConfig["ocr"]["provider"]|null;idle_timeout_secs:number;selection: Selection; provider: {kind: "local_paddle" | "open_ai_compatible"; base_url?: string; model?: string; api?: "chat_completions" | "responses"}; fallback_to_local: boolean; language: string; model_dir: string; runtime_dir: string; device: "auto" | "cpu" | "cuda"};
  playback: {keep_warm_seconds: number; output_device: string | null; max_buffer_seconds: number; sentence_gap_ms: number; auto_dismiss_seconds: number};
  read_along: {prefetch: boolean; advance_key: string; sequence?: RecordedAction|null; action: "key" | "click"; delay_ms: number; retry_count: number};
  shortcuts: {read_selection: string; read_region: string; read_along: string; copy_region: string; listen_clipboard: string};
  selection_copy: {backend: string; shortcut: string; allow_clipboard_copy: boolean; show_clipboard_notice: boolean};
  selection_copy_overrides: {app_id: string; shortcut: string; backend: string | null}[];
  privacy: {hide_text_in_notifications: boolean};
  server: {enabled: boolean; listen: string; allow_remote: boolean; max_clients: number; max_text_bytes: number; max_image_pixels: number};
}
export interface Asset {id: string; label: string; kind: string; bytes: number; installed: boolean; available: boolean; directory: string; dependencies: string[]}
export interface AssetProgress {id: string; job_id?: string; phase: string; queued?:string[]; downloaded: number; total: number; error: string}
export interface AssetState { assets: Asset[]; progress: AssetProgress; jobs?: AssetProgress[]; selection_revision?: number }

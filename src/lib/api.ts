// Typed wrappers over the Tauri IPC commands.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type OutputMode = "paste" | "type";
export type HotkeyMode = "push_to_talk" | "toggle";

export interface Settings {
  hotkey: string;
  hotkey_mode: HotkeyMode;
  language: string;
  asr_model: string;
  llm_model: string;
  cleanup_enabled: boolean;
  output_mode: OutputMode;
  mic_device: string;
  launch_at_login: boolean;
  history_enabled: boolean;
  low_memory_unload_secs: number;
  threads: number;
  onboarding_done: boolean;
}

export interface ModelSpec {
  id: string;
  kind: "asr" | "llm";
  file_name: string;
  url: string;
  sha256: string;
  size_bytes: number;
  label: string;
  note: string;
}

export interface ModelStatus {
  id: string;
  installed: boolean;
  bytes_on_disk: number;
}

export interface HistoryEntry {
  id: number;
  raw_text: string;
  final_text: string;
  language: string;
  app_name: string;
  duration_ms: number;
  created_at: number;
}

export interface StageTimings {
  record_ms: number;
  vad_ms: number;
  asr_ms: number;
  llm_ms: number;
  inject_ms: number;
  total_ms: number;
}

export type PipelineEvent =
  | { type: "recording_started" }
  | { type: "level"; value: number; elapsed_secs: number }
  | { type: "recording_stopped" }
  | { type: "transcribing" }
  | { type: "cleaning" }
  | {
      type: "done";
      raw_text: string;
      final_text: string;
      outcome: "injected" | "clipboard_only" | "refused_secure_field";
      timings: StageTimings;
      history_id: number | null;
    }
  | { type: "empty" }
  | { type: "error"; message: string }
  | { type: "model_loading"; which: string }
  | { type: "model_ready"; which: string };

export interface DownloadProgress {
  id: string;
  downloaded: number;
  total: number;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  listMics: () => invoke<string[]>("list_mics"),
  modelCatalog: () => invoke<ModelSpec[]>("model_catalog"),
  modelStatus: () => invoke<ModelStatus[]>("model_status"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  cancelDownload: (id: string) => invoke<void>("cancel_download", { id }),
  deleteModel: (id: string) => invoke<void>("delete_model", { id }),
  historyList: (query = "", limit = 100) =>
    invoke<HistoryEntry[]>("history_list", { query, limit }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
  dictionaryList: () => invoke<string[]>("dictionary_list"),
  dictionaryAdd: (word: string) => invoke<string[]>("dictionary_add", { word }),
  dictionaryRemove: (word: string) => invoke<string[]>("dictionary_remove", { word }),
  startDictation: () => invoke<void>("start_dictation"),
  stopDictation: (discard = false) => invoke<void>("stop_dictation", { discard }),
  cleanTextPreview: (raw: string) => invoke<string>("clean_text_preview", { raw }),
  openSettingsPane: (pane: string) => invoke<void>("open_settings_pane", { pane }),
};

export function onPipelineEvent(cb: (ev: PipelineEvent) => void): Promise<UnlistenFn> {
  return listen<PipelineEvent>("pipeline-event", (e) => cb(e.payload));
}

export function onDownloadProgress(
  cb: (p: DownloadProgress) => void
): Promise<UnlistenFn> {
  return listen<DownloadProgress>("model-download-progress", (e) => cb(e.payload));
}

export function onNavigate(cb: (route: string) => void): Promise<UnlistenFn> {
  return listen<string>("navigate", (e) => cb(e.payload));
}

export function fmtBytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${(n / 1e6).toFixed(0)} MB`;
  return `${(n / 1e3).toFixed(0)} KB`;
}

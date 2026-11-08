// Types and command wrappers for the Rust core.
import { invoke, convertFileSrc } from "@tauri-apps/api/core";

export interface Issue {
  code: string;
  message: string;
  data: Record<string, unknown>;
}

interface ErrorResponse {
  code: string;
  message: string;
  issues: Issue[];
}

export interface VoiceCard {
  id: string;
  /** builtin = ships with the app (read-only) */
  source: "builtin" | "designed" | "clone";
  folder: string;
  name: string;
  display_name: string;
  alias: string | null;
  variant: number | null;
  language: string;
  description: string;
  design_text: string;
  design_seed: number | null;
  read_seed: number;
  reference: string;
  reference_s: number | null;
  preview: string | null;
  preview_text: string | null;
  /** "female" | "male" */
  gender: string | null;
  created_at: string | null;
  job_id: string | null;
  favorite: boolean;
  warning: string | null;
  preview_error: string | null;
  /** speech minus background-noise floor of the reference, dB */
  noise_margin_db: number | null;
  /** audible background noise in the reference: readings inherit it */
  noisy: boolean;
}

export interface Word {
  word: string;
  t0: number;
  t1: number;
  line?: number;
}

export interface LineRecord {
  line: number;
  text: string;
  spoken: string;
  pause: number;
  seed: number;
  wav: string;
  t0: number;
  t1: number;
  words: Word[];
  speed_warning: boolean;
  version: number;
  heard?: string;
  wer?: number;
  speed?: number;
  drift?: number;
  target_t0?: number;
}

export interface DesignVariant {
  variant?: number;
  seed?: number;
  design_seed?: number;
  read_seed?: number;
  reference_s?: number;
  reference_path?: string;
  preview_path?: string;
  preview_s?: number;
  preview_error?: string;
  warning?: string;
  error?: string;
  display_name?: string;
  /** speech minus background-noise floor of the reference, dB */
  noise_margin_db?: number | null;
  /** audible background noise: every reading would inherit it */
  noisy?: boolean;
  /** library voice created from this variant with `add_design_variant`; null until added */
  library_voice_id?: string | null;
}

export interface Cue {
  t0: number;
  t1: number;
  text: string;
  speaker?: string | null;
}

export type JobKind = "design" | "read" | "clone" | "line" | "transcribe" | "review" | "timed";
export type JobStatus = "queued" | "running" | "done" | "failed" | "cancelled" | "interrupted";

export interface Job {
  id: string;
  kind: JobKind;
  status: JobStatus;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  title: string;
  voice_id: string | null;
  voice_name: string | null;
  language: string | null;
  text: string;
  request: Record<string, unknown>;
  result: Record<string, any> | null;
  folder: string | null;
  audio_s: number | null;
  duration_s: number | null;
  gpu_peak_mb: number | null;
  error: string | null;
  starred: boolean;
  notes: string;
  completed: number;
  total: number;
  estimated_s: number | null;
  parent_id: string | null;
}

interface JobFilter {
  search?: string | null;
  kind?: JobKind | null;
  status?: JobStatus | null;
  voice?: string | null;
  starred?: boolean;
  limit?: number;
}

export interface Draft {
  id: number;
  name: string;
  text: string;
  updated_at: string;
}

export interface VoicePrompt {
  id: string;
  language: string;
  role: string;
  category: string;
  /** "female" | "male" */
  gender: string;
  age: string;
  name_tr: string;
  name_en: string;
  description: string;
  text: string;
}

export interface TrashEntry {
  trash_id: string;
  voice_id: string;
  display_name: string;
  deleted_at: string;
}

export interface Settings {
  /** auto | mps | cpu */
  device: string;
  /** bfloat16 | float32 */
  precision: string;
  steps: number;
  cfg: number;
  idle_unload_min: number;
  word_timing: boolean;
  tone_match: boolean;
  model_path: string | null;
  export_dir: string | null;
  /** wav | mp3 */
  export_format: string;
  file_name_template: string;
  keep_intermediate: boolean;
  /** system | light | dark */
  theme: string;
  last_voice: string | null;
  /** local HTTP API (127.0.0.1 only) */
  api_enabled: boolean;
  /** ≥ 1024, default 7860 */
  api_port: number;
  /** 32 hex; saving an empty token keeps the current one */
  api_token: string;
}

export interface ApiInfo {
  enabled: boolean;
  port: number;
  url: string;
  token: string;
  running: boolean;
  error: string | null;
}

/** Response of a request sent to the running local API (`call_local_api`). */
export interface LocalCall {
  status: number;
  body: unknown;
}

export interface Status {
  version: string;
  data_dir: string;
  resources: string;
  engine: {
    name: string;
    script: string;
    present: boolean;
    uv: boolean;
    /** missing | outdated | ready */
    env: string;
    env_ready: boolean;
    env_size: number;
    loaded: boolean;
  };
  whisper: { binary_present: boolean; model_ready: boolean; size: number };
  ffmpeg: string | null;
  model: {
    path: string;
    present: boolean;
    ready: boolean;
    verified: boolean;
    size: number;
    expected: number;
    partial: number;
    commit: string;
    repo: string;
  };
  engine_label: string;
  system: { os: string; arch: string; ram_gb: number | null; gpu: string | null; warnings: string[] };
  stats: { completed: number; total_audio_s: number };
}

export interface TextCheck {
  language: string;
  issues: Issue[];
  findings: { line: number; column: number; length: number; code: string; message: string }[];
  line_count: number;
  audio_s: number;
  job_s: number;
  estimate_samples: number;
  dictionary: { line: number; column: number; length: number; pronunciation: string }[];
}

export interface FixOptions {
  numbers: boolean;
  abbreviations: boolean;
  citations: boolean;
  parentheses: boolean;
  quotes: boolean;
  sentences: boolean;
}

export interface FixResult {
  language: string;
  text: string;
  changes: { kind: string; before: string; after: string }[];
  remaining: TextCheck["findings"];
}

export interface DesignCheck {
  language: string;
  seconds: number;
  limit: number;
  hard_limit: number;
  issues: Issue[];
  job_s: number;
}

type JobRequest =
  | {
      kind: "design";
      name: string;
      description: string;
      text: string;
      language?: string | null;
      variants: number;
      seeds?: number[] | null;
      preview_text?: string | null;
    }
  | { kind: "read"; voice: string; text: string; language?: string | null; title?: string | null }
  | {
      kind: "clone";
      recording: string;
      recording_text: string;
      text: string;
      seed?: number | null;
      consent: boolean;
      library_name?: string | null;
    }
  | { kind: "line"; job_id: string; line: number; text?: string | null; seed?: number | null }
  | { kind: "transcribe"; file: string; language?: string | null }
  | { kind: "review"; job_id: string }
  | {
      kind: "timed";
      voice: string;
      cues: Cue[];
      speaker_voices?: Record<string, string>;
      language?: string | null;
      max_speedup?: number;
      video?: string | null;
      original_audio?: "duck" | "remove";
      title?: string | null;
    };

export interface DownloadStatus {
  stage: "downloading" | "verifying" | "extracting" | "installing" | "done" | "error" | "cancelled";
  current: number;
  total: number;
  speed_mb_s: number;
  message: string | null;
}

export type AppEvent =
  | {
      event: "job";
      id: string;
      kind: string;
      status: JobStatus;
      completed: number;
      total: number;
      elapsed_s: number;
      estimated_s: number | null;
      message: string | null;
      parent_id: string | null;
    }
  | ({ event: "model" } & DownloadStatus)
  | ({ event: "engine_env" } & DownloadStatus)
  | ({ event: "whisper" } & DownloadStatus)
  | { event: "library" }
  | { event: "queue" };

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as ErrorResponse).message);
  return String(e);
}

export function errorIssues(e: unknown): Issue[] {
  if (e && typeof e === "object" && "issues" in e) return (e as ErrorResponse).issues ?? [];
  return [];
}

/** Turns a local file path into a URL the webview can load; `version` busts the cache. */
export function fileUrl(path: string, version?: string | number): string {
  const url = convertFileSrc(path);
  return version !== undefined ? `${url}?v=${encodeURIComponent(String(version))}` : url;
}

export function joinPath(dir: string, name: string): string {
  return dir.replace(/\/+$/, "") + "/" + name.replace(/^\/+/, "");
}

export const api = {
  getStatus: () => invoke<Status>("get_status"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  setDataDir: (dir: string) => invoke<void>("set_data_dir", { dir }),
  unloadEngine: () => invoke<void>("unload_engine"),
  clearCache: () => invoke<number>("clear_cache"),
  listVoices: () => invoke<VoiceCard[]>("list_voices"),
  manageVoice: (id: string, action: "rename" | "favorite" | "unfavorite" | "trash" | "restore", name?: string | null) =>
    invoke<void>("manage_voice", { id, action, name: name ?? null }),
  listTrash: () => invoke<TrashEntry[]>("list_trash"),
  exportVoice: (id: string, target: string) => invoke<string>("export_voice", { id, target }),
  importVoice: (source: string) => invoke<string>("import_voice", { source }),
  listVoicePrompts: () => invoke<VoicePrompt[]>("list_voice_prompts"),
  checkText: (text: string, language?: string | null, voice?: string | null) =>
    invoke<TextCheck>("check_text", { text, language: language ?? null, voice: voice ?? null }),
  checkDesign: (description: string, text: string, language: string | null, variants: number, previewText: string | null) =>
    invoke<DesignCheck>("check_design", { description, text, language, variants, previewText }),
  fixText: (text: string, language: string | null, voice: string | null, options: FixOptions) =>
    invoke<FixResult>("fix_text", { text, language, voice, options }),
  readDictionary: () => invoke<string>("read_dictionary"),
  saveDictionary: (content: string) => invoke<number>("save_dictionary", { content }),
  previewPronunciation: (text: string, language: string) => invoke<string>("preview_pronunciation", { text, language }),
  enqueueJob: (request: JobRequest) => invoke<Job>("enqueue_job", { request }),
  cancelJob: (id: string) => invoke<void>("cancel_job", { id }),
  rerunJob: (id: string) => invoke<Job>("rerun_job", { id }),
  /** Copies a design study's variant (`result.variants[i].variant`) into the library (idempotent); name defaults to the study's name. */
  addDesignVariant: (jobId: string, variant: number, name?: string | null) =>
    invoke<VoiceCard>("add_design_variant", { jobId, variant, name: name ?? null }),
  /** Removes a design study (its folder and job record); library voices already added stay. */
  deleteDesign: (jobId: string) => invoke<void>("delete_design", { jobId }),
  listQueue: () => invoke<Job[]>("list_queue"),
  listJobs: (filter?: JobFilter) => invoke<Job[]>("list_jobs", { filter: filter ?? null }),
  getJob: (id: string) => invoke<Job | null>("get_job", { id }),
  updateJob: (id: string, fields: { title?: string; starred?: boolean; notes?: string }) =>
    invoke<void>("update_job", { id, title: fields.title ?? null, starred: fields.starred ?? null, notes: fields.notes ?? null }),
  deleteJob: (id: string, deleteFiles: boolean) => invoke<void>("delete_job", { id, deleteFiles }),
  exportJob: (id: string, format: string, target: string) => invoke<string>("export_job", { id, format, target }),
  suggestedFileName: (id: string) => invoke<string>("suggested_file_name", { id }),
  listDrafts: () => invoke<Draft[]>("list_drafts"),
  saveDraft: (name: string, text: string, id?: number | null) => invoke<number>("save_draft", { id: id ?? null, name, text }),
  deleteDraft: (id: number) => invoke<void>("delete_draft", { id }),
  downloadModel: () => invoke<void>("download_model"),
  cancelModelDownload: () => invoke<void>("cancel_model_download"),
  useLocalModel: (path: string) => invoke<void>("use_local_model", { path }),
  installEngineEnv: () => invoke<void>("install_engine_env"),
  cancelEngineEnvInstall: () => invoke<void>("cancel_engine_env_install"),
  downloadWhisperModel: () => invoke<void>("download_whisper_model"),
  cancelWhisperDownload: () => invoke<void>("cancel_whisper_download"),
  parseSubtitles: (path: string) => invoke<Cue[]>("parse_subtitles", { path }),
  allowFile: (path: string) => invoke<void>("allow_file", { path }).catch(() => undefined),
  saveRecording: (data: number[]) => invoke<string>("save_recording", { data }),
  audioDuration: (path: string) => invoke<number>("audio_duration", { path }),
  restartApp: () => invoke<void>("restart_app"),
  getApiInfo: () => invoke<ApiInfo>("get_api_info"),
  /** Sends a real HTTP request to the local API (used by the example request in Ayarlar → Yerel API). */
  callLocalApi: (method: "GET" | "POST", path: string, body?: unknown) => invoke<LocalCall>("call_local_api", { method, path, body: body ?? null }),
  regenerateApiToken: () => invoke<ApiInfo>("regenerate_api_token"),
};

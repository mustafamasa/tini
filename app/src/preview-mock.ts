// Dev only: outside Tauri (in a plain browser) renders the UI against a snapshot of real data.
// The snapshot is produced with `tini-cli json app/preview-data/data.json` and is not bundled.
// Design studies, the queue, the local API and the whisper download are simulated here so every page renders.
import { mockConvertFileSrc, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";

type Json = Record<string, any>;

const now = () => new Date().toISOString().replace("T", " ").slice(0, 19);
const ago = (minutes: number) => new Date(Date.now() - minutes * 60_000).toISOString().replace("T", " ").slice(0, 19);

function job(p: Json): Json {
  return {
    status: "done",
    created_at: now(),
    started_at: null,
    finished_at: null,
    title: "",
    voice_id: null,
    voice_name: null,
    language: "tr",
    text: "",
    request: {},
    result: null,
    folder: null,
    audio_s: null,
    duration_s: null,
    gpu_peak_mb: null,
    error: null,
    starred: false,
    notes: "",
    completed: 0,
    total: 1,
    estimated_s: null,
    parent_id: null,
    ...p,
  };
}

function designStudies(voices: Json[]): Json[] {
  const ref = (i: number) => voices[i % Math.max(1, voices.length)]?.reference ?? null;
  const variant = (n: number, seed: number, extra: Json = {}) => ({
    variant: n,
    seed,
    design_seed: seed,
    read_seed: 42,
    reference_s: 11.4 + n,
    reference_path: ref(n),
    preview_path: null,
    preview_s: null,
    preview_error: null,
    warning: null,
    error: null,
    noise_margin_db: 38,
    noisy: false,
    folder: `/designs/x/variant_0${n}`,
    library_voice_id: null,
    ...extra,
  });
  const description =
    "A calm Turkish woman in her forties with a warm, clear mezzo voice, a native Turkish speaker from Istanbul, measured pace, clean studio recording, no background noise";
  const text = "Deniz fenerinin ışığı her gece aynı saatte yanar; kıyıdaki herkes bilir ki yol bu ışığı izleyen için hiç kaybolmaz.";
  return [
    job({
      id: "design_running",
      kind: "design",
      status: "running",
      created_at: ago(2),
      started_at: ago(1),
      title: "Radyo tiyatrosu anlatıcısı",
      text,
      request: { kind: "design", name: "Radyo tiyatrosu anlatıcısı", description: "A deep, theatrical Turkish man in his sixties…", text, variants: 4 },
      completed: 1,
      total: 4,
      estimated_s: 180,
    }),
    job({
      id: "design_queued",
      kind: "design",
      status: "queued",
      created_at: ago(1),
      title: "Çocuk masalı",
      text,
      request: { kind: "design", name: "Çocuk masalı", description: "A cheerful young Turkish woman…", text, variants: 3 },
      estimated_s: 140,
    }),
    job({
      id: "design_done",
      kind: "design",
      created_at: ago(55),
      finished_at: ago(50),
      title: "Deniz feneri anlatıcısı",
      text,
      duration_s: 212,
      request: { kind: "design", name: "Deniz feneri anlatıcısı", description, text, variants: 3, preview_text: "Bu sesle uzun bir metin okutmak istiyorum." },
      result: {
        name: "Deniz feneri anlatıcısı",
        display_name: "Deniz feneri anlatıcısı",
        slug: "deniz-feneri-anlaticisi",
        description,
        design_text: text,
        language: "tr",
        variants: [
          variant(1, 2692139539, { preview_path: ref(1), preview_s: 4.2 }),
          variant(2, 7, { noisy: true, noise_margin_db: 17, preview_path: ref(2), preview_s: 4.4 }),
          variant(3, 42, { library_voice_id: voices[0]?.id ?? null, preview_path: ref(3), preview_s: 4.1 }),
        ],
      },
    }),
    job({
      id: "design_failed",
      kind: "design",
      status: "failed",
      created_at: ago(60 * 26),
      finished_at: ago(60 * 26 - 2),
      title: "Belgesel sesi",
      text,
      error: "Ses motoru beklenmedik biçimde kapandı (bellek yetersiz olabilir).",
      request: { kind: "design", name: "Belgesel sesi", description: "A low, authoritative documentary narrator…", text, variants: 5 },
    }),
  ];
}

export async function installPreviewMock() {
  const data = await fetch("/preview-data/data.json").then((r) => r.json());
  if (data.fix_text && !localStorage.getItem("tini.studioText")) localStorage.setItem("tini.studioText", data.fix_text);
  data.settings = { api_enabled: true, api_port: 7860, api_token: "3f9c2a7be41d48f0a6c55e2d19b07c8e", ...data.settings };
  // Preview switches: tini.mock.whisper=1 → transcript model ready; tini.mock.setup=1 → first-run setup screen.
  if (localStorage.getItem("tini.mock.whisper") === "1") data.status.whisper.model_ready = true;
  if (localStorage.getItem("tini.mock.setup") === "1") data.status.model = { ...data.status.model, ready: false, present: false, partial: 0 };
  const studies = designStudies(data.voices);
  data.jobs = [...studies, ...data.jobs];
  for (const s of studies) data.job[s.id] = s;
  const jobs = (): Json[] => data.jobs;
  const queue = () => jobs().filter((j) => j.status === "queued" || j.status === "running");

  mockWindows("main");
  mockConvertFileSrc("macos");
  // Serve local files through Vite (only paths under fs.allow, e.g. resources/voices, actually load).
  (window as any).__TAURI_INTERNALS__.convertFileSrc = (path: string) => `/@fs${path}`;

  // Tick the running study so the sidebar queue card and progress bars move.
  let step = 1;
  setInterval(() => {
    const running = jobs().find((j) => j.status === "running");
    if (!running) return;
    step = (step % running.total) + 1;
    emit("tini-event", {
      event: "job",
      id: running.id,
      kind: running.kind,
      status: "running",
      completed: step - 1,
      total: running.total,
      elapsed_s: 30 + step * 20,
      estimated_s: running.estimated_s,
      message: `Çeşit ${step}/${running.total} tasarlanıyor`,
      parent_id: null,
    });
  }, 2500);

  const apiInfo = () => ({
    enabled: data.settings.api_enabled,
    port: data.settings.api_port,
    url: `http://127.0.0.1:${data.settings.api_port}`,
    token: data.settings.api_token,
    running: data.settings.api_enabled,
    error: null,
  });

  const simulateWhisper = () => {
    const total = data.status.whisper.size;
    let current = 0;
    const t = setInterval(() => {
      current = Math.min(total, current + total / 12);
      if (current >= total) {
        clearInterval(t);
        data.status.whisper.model_ready = true;
        emit("tini-event", { event: "whisper", stage: "done", current, total, speed_mb_s: 0, message: null });
      } else {
        emit("tini-event", { event: "whisper", stage: "downloading", current, total, speed_mb_s: 48.2, message: null });
      }
    }, 600);
  };

  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Json;
      switch (cmd) {
        case "get_status":
          return data.status;
        case "get_settings":
          return data.settings;
        case "save_settings":
          data.settings = { ...data.settings, ...a.settings, api_token: a.settings?.api_token || data.settings.api_token };
          return data.settings;
        case "get_api_info":
          return apiInfo();
        case "call_local_api": {
          const done = jobs().find((j) => j.kind === "read" && j.status === "done");
          return { status: 201, body: done ?? { id: "preview" } };
        }
        case "regenerate_api_token":
          data.settings.api_token = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, "0")).join("");
          return apiInfo();
        case "list_voices":
          return data.voices;
        case "list_voice_prompts":
          return data.voice_prompts;
        case "list_jobs": {
          const f = a.filter ?? {};
          return jobs().filter(
            (j) =>
              (!f.kind || j.kind === f.kind) &&
              (!f.status || j.status === f.status) &&
              (!f.voice || j.voice_id === f.voice) &&
              (!f.starred || j.starred) &&
              (!f.search || `${j.title} ${j.text}`.toLocaleLowerCase("tr").includes(String(f.search).toLocaleLowerCase("tr"))),
          );
        }
        case "get_job":
          return data.job[a.id] ?? jobs().find((j) => j.id === a.id) ?? null;
        case "list_queue":
          return queue();
        case "enqueue_job": {
          const r = a.request ?? {};
          const j = job({
            id: `mock_${Date.now()}`,
            kind: r.kind,
            status: "queued",
            title: r.name || r.title || "Yeni iş",
            text: r.text ?? "",
            request: r,
            total: r.variants ?? 1,
            estimated_s: 120,
          });
          data.jobs.unshift(j);
          data.job[j.id] = j;
          emit("tini-event", { event: "queue" });
          return j;
        }
        case "rerun_job": {
          const src = data.job[a.id] ?? jobs().find((j) => j.id === a.id);
          const j = job({ ...src, id: `mock_${Date.now()}`, status: "queued", result: null, created_at: now(), error: null });
          data.jobs.unshift(j);
          data.job[j.id] = j;
          emit("tini-event", { event: "queue" });
          return j;
        }
        case "cancel_job": {
          const j = data.job[a.id];
          if (j) {
            j.status = "cancelled";
            emit("tini-event", { event: "job", id: j.id, kind: j.kind, status: "cancelled", completed: 0, total: j.total, elapsed_s: 0, estimated_s: null, message: null, parent_id: null });
          }
          return null;
        }
        case "add_design_variant": {
          const j = data.job[a.jobId];
          const v = j?.result?.variants?.find((x: Json) => x.variant === a.variant);
          if (!v) throw { code: "not_found", message: "Çeşit bulunamadı", issues: [] };
          const existing = data.voices.find((x: Json) => x.id === v.library_voice_id);
          if (existing) return existing;
          const name = a.name || j.result.name;
          const card = {
            ...data.voices[0],
            id: `${j.result.slug}/variant_${v.variant}`,
            source: "designed",
            name,
            display_name: `${name} ${v.variant}`,
            alias: null,
            variant: v.variant,
            description: j.result.description,
            design_text: j.result.design_text,
            design_seed: v.design_seed,
            reference: v.reference_path,
            preview: v.preview_path,
            job_id: j.id,
            favorite: false,
            noisy: v.noisy,
            noise_margin_db: v.noise_margin_db,
            created_at: now(),
          };
          data.voices.push(card);
          v.library_voice_id = card.id;
          emit("tini-event", { event: "library" });
          return card;
        }
        case "delete_design":
        case "delete_job":
          data.jobs = jobs().filter((j) => j.id !== a.jobId && j.id !== a.id);
          delete data.job[a.jobId ?? a.id];
          return null;
        case "download_whisper_model":
          simulateWhisper();
          return null;
        case "list_drafts":
        case "list_trash":
          return [];
        case "check_text":
          return data.text_check;
        case "check_design":
          return { language: "tr", seconds: (a.text?.length ?? 0) / 14.5, limit: 30, hard_limit: 33, issues: [], job_s: 60 * Math.max(1, a.variants ?? 1) };
        case "fix_text":
          return data.fix;
        case "read_dictionary":
          return data.dictionary;
        case "suggested_file_name":
          return "preview";
        case "preview_pronunciation":
          return a.text;
        default:
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}

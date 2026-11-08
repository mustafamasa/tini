// App state (zustand) and core event handling.
import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type AppEvent, type Cue, type DownloadStatus, type Job, type Settings, type Status, type VoiceCard } from "./api";

export type Page = "studio" | "design" | "library" | "clone" | "transcribe" | "timed" | "jobs" | "settings";

interface Notification {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
  action?: { label: string; run: () => void };
}

interface LiveJob {
  id: string;
  kind: string;
  status: Job["status"];
  completed: number;
  total: number;
  elapsed_s: number;
  estimated_s: number | null;
  message: string | null;
  parent_id: string | null;
  startedAt: number;
}

interface Store {
  page: Page;
  status: Status | null;
  settings: Settings | null;
  voices: VoiceCard[];
  queue: Job[];
  live: Record<string, LiveJob>;
  notifications: Notification[];
  model: DownloadStatus | null;
  engineEnv: DownloadStatus | null;
  whisper: DownloadStatus | null;
  transcribeResult: string | null;
  timedResult: string | null;
  /** Cues handed from the Transcribe page to Timed narration. */
  timedCues: Cue[] | null;
  studioVoice: string | null;
  studioResult: string | null;
  designResult: string | null;
  cloneResult: string | null;
  selectedJob: string | null;
  selectedVoice: string | null;
  /** Settings section to open next (e.g. the sidebar API badge opens "api"). */
  settingsSection: string | null;
  /** Bumped whenever a job finishes; views watch it to reload. */
  revision: number;
  navigate: (page: Page) => void;
  set: (p: Partial<Store>) => void;
  notify: (text: string, kind?: Notification["kind"], action?: Notification["action"]) => void;
  dismissNotification: (id: number) => void;
  refreshStatus: () => Promise<void>;
  refreshVoices: () => Promise<void>;
  refreshQueue: () => Promise<void>;
  saveSettings: (s: Partial<Settings>) => Promise<void>;
}

let nextId = 1;

export const useStore = create<Store>((set, get) => ({
  page: "studio",
  status: null,
  settings: null,
  voices: [],
  queue: [],
  live: {},
  notifications: [],
  model: null,
  engineEnv: null,
  whisper: null,
  transcribeResult: null,
  timedResult: null,
  timedCues: null,
  studioVoice: localStorage.getItem("tini.studioVoice"),
  studioResult: null,
  designResult: null,
  cloneResult: null,
  selectedJob: null,
  selectedVoice: null,
  settingsSection: null,
  revision: 0,
  navigate: (page) => set({ page }),
  set: (p) => set(p),
  notify: (text, kind = "info", action) => {
    const id = nextId++;
    set({ notifications: [...get().notifications, { id, kind, text, action }] });
    setTimeout(() => get().dismissNotification(id), kind === "error" ? 9000 : 5000);
  },
  dismissNotification: (id) => set({ notifications: get().notifications.filter((n) => n.id !== id) }),
  refreshStatus: async () => {
    const [status, settings] = await Promise.all([api.getStatus(), api.getSettings()]);
    set({ status, settings });
  },
  refreshVoices: async () => {
    try {
      set({ voices: await api.listVoices() });
    } catch (e) {
      get().notify(errorMessage(e), "error");
    }
  },
  refreshQueue: async () => {
    try {
      set({ queue: await api.listQueue() });
    } catch {
      /* ignore */
    }
  },
  saveSettings: async (s) => {
    const current = get().settings;
    if (!current) return;
    try {
      set({ settings: await api.saveSettings({ ...current, ...s }) });
    } catch (e) {
      get().notify(errorMessage(e), "error");
      throw e;
    }
  },
}));

useStore.subscribe((s, prev) => {
  if (s.studioVoice !== prev.studioVoice && s.studioVoice) localStorage.setItem("tini.studioVoice", s.studioVoice);
});

async function systemNotification(title: string, body: string) {
  if (document.hasFocus()) return;
  try {
    const n = await import("@tauri-apps/plugin-notification");
    let granted = await n.isPermissionGranted();
    if (!granted) granted = (await n.requestPermission()) === "granted";
    if (granted) n.sendNotification({ title, body });
  } catch {
    /* notifications unavailable */
  }
}

const KIND_LABEL: Record<string, string> = {
  design: "Ses tasarımı",
  read: "Okuma",
  clone: "Klon okuma",
  line: "Satır yenileme",
  transcribe: "Transkript",
  review: "Kalite kontrolü",
  timed: "Altyazıdan seslendirme",
};

let statusTimer: ReturnType<typeof setTimeout> | null = null;
/** Coalesces status refreshes (engine state changes when jobs finish). */
function refreshStatusSoon() {
  if (statusTimer) clearTimeout(statusTimer);
  statusTimer = setTimeout(() => {
    statusTimer = null;
    useStore
      .getState()
      .refreshStatus()
      .catch(() => {});
  }, 800);
}

/** Starts listening to core events (once, at startup). */
export async function listenToEvents() {
  await listen<AppEvent>("tini-event", (e) => {
    const ev = e.payload;
    const s = useStore.getState();
    if (ev.event === "job") {
      const prev = s.live[ev.id];
      const { event: _, ...job } = ev;
      const live: LiveJob = { ...job, startedAt: prev?.startedAt ?? Date.now() - ev.elapsed_s * 1000 };
      if (ev.status === "running") {
        s.set({ live: { ...s.live, [ev.id]: live } });
      } else {
        const { [ev.id]: _, ...rest } = s.live;
        s.set({ live: rest, revision: s.revision + 1 });
        const label = KIND_LABEL[ev.kind] ?? "İş";
        if (ev.status === "done") {
          s.notify(`${label} bitti`, "success");
          systemNotification("Tını", `${label} bitti`);
        } else if (ev.status === "failed") {
          s.notify(`${label} başarısız: ${ev.message ?? ""}`, "error");
          systemNotification("Tını", `${label} başarısız`);
        }
        s.refreshQueue();
        refreshStatusSoon();
      }
    } else if (ev.event === "model") {
      const { event: _, ...m } = ev;
      s.set({ model: m });
      if (m.stage === "done") {
        s.refreshStatus();
        s.notify("Model hazır", "success");
      } else if (m.stage === "error") {
        s.notify(m.message ?? "Model işlemi başarısız", "error");
      }
    } else if (ev.event === "engine_env") {
      const { event: _, ...m } = ev;
      s.set({ engineEnv: m });
      if (m.stage === "done") {
        s.refreshStatus();
        s.notify("Ses motoru kuruldu", "success");
      } else if (m.stage === "error") {
        s.notify(m.message ?? "Ses motoru kurulamadı", "error");
      }
    } else if (ev.event === "whisper") {
      const { event: _, ...m } = ev;
      s.set({ whisper: m });
      if (m.stage === "done") {
        s.refreshStatus();
        s.notify("Transkript modeli hazır", "success");
      } else if (m.stage === "error") {
        s.notify(m.message ?? "Transkript modeli indirilemedi", "error");
      }
    } else if (ev.event === "library") {
      s.refreshVoices();
    } else if (ev.event === "queue") {
      s.refreshQueue();
    }
  });
}

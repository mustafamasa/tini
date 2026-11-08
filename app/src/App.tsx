// App shell: grouped sidebar with live queue / engine / API status, pages, notifications, theme.
import { useEffect, useState, type ReactNode } from "react";
import { AudioWaveform, Captions, CheckCircle2, History, Info, Library as LibraryIcon, Loader2, Mic, PencilRuler, ScrollText, Settings as SettingsIcon, X, XCircle } from "lucide-react";
import { api, type ApiInfo, type Job } from "./api";
import { useStore, type Page } from "./store";
import { Logo, LogoLockup } from "./components/Logo";
import { ProgressBar, StatusDot, cx, modKey } from "./components/ui";
import { Studio } from "./pages/Studio";
import { Design } from "./pages/Design";
import { Library } from "./pages/Library";
import { Clone } from "./pages/Clone";
import { Jobs } from "./pages/Jobs";
import { Settings } from "./pages/Settings";
import { Setup } from "./pages/Setup";
import { Transcribe } from "./pages/Transcribe";
import { TimedNarration } from "./pages/TimedNarration";
import { formatDuration } from "./languages";
import { JOB_KIND } from "./components/JobProgress";

interface MenuEntry {
  page: Page;
  label: string;
  icon: ReactNode;
  /** Job kinds whose queued/running jobs are counted on this entry. */
  kinds: Job["kind"][];
}

const GROUPS: { title: string; items: MenuEntry[] }[] = [
  {
    title: "Oluştur",
    items: [
      { page: "studio", label: "Stüdyo", icon: <AudioWaveform size={16} />, kinds: ["read", "line", "review"] },
      { page: "timed", label: "Altyazıdan seslendirme", icon: <Captions size={16} />, kinds: ["timed"] },
    ],
  },
  {
    title: "Sesler",
    items: [
      { page: "library", label: "Kitaplık", icon: <LibraryIcon size={16} />, kinds: [] },
      { page: "design", label: "Ses tasarımı", icon: <PencilRuler size={16} />, kinds: ["design"] },
      { page: "clone", label: "Ses klonla", icon: <Mic size={16} />, kinds: ["clone"] },
    ],
  },
  { title: "Araçlar", items: [{ page: "transcribe", label: "Transkript", icon: <ScrollText size={16} />, kinds: ["transcribe"] }] },
  { title: "Arşiv", items: [{ page: "jobs", label: "Üretimler", icon: <History size={16} />, kinds: [] }] },
];

const MENU = GROUPS.flatMap((g) => g.items);

function NavItem({ entry, index, active, disabled, busy }: { entry: MenuEntry; index: number; active: boolean; disabled: boolean; busy: number }) {
  const navigate = useStore((s) => s.navigate);
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => navigate(entry.page)}
      title={`${entry.label} (${modKey}${index + 1})`}
      aria-current={active ? "page" : undefined}
      className={cx(
        "group flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] transition-colors disabled:opacity-40",
        active ? "bg-surface font-medium text-text shadow-[0_1px_2px_rgb(0_0_0/0.06)]" : "text-text-2 hover:bg-surface/60 hover:text-text",
      )}
    >
      <span className={cx("flex shrink-0", active ? "text-text" : "text-muted group-hover:text-text-2")}>{entry.icon}</span>
      <span className="min-w-0 flex-1 truncate">{entry.label}</span>
      {busy > 0 && (
        <span className="font-mono text-[12px] text-muted tabular-nums" aria-label={`${busy} iş`}>
          {busy}
        </span>
      )}
    </button>
  );
}

function QueueCard() {
  const { queue, live, set, navigate } = useStore();
  if (!queue.length) return null;
  const running = queue.find((j) => j.status === "running");
  const l = running ? live[running.id] : undefined;
  const waiting = queue.filter((j) => j.status === "queued").length;
  const elapsed = l ? (Date.now() - l.startedAt) / 1000 : 0;
  const progress =
    l && running?.estimated_s
      ? Math.min(0.97, Math.max(l.completed / Math.max(1, l.total), elapsed / running.estimated_s))
      : l
        ? l.completed / Math.max(1, l.total)
        : 0.02;
  const remaining = running?.estimated_s && l ? Math.max(0, Math.round(running.estimated_s - elapsed)) : null;
  return (
    <button
      type="button"
      onClick={() => {
        set({ selectedJob: (running ?? queue[0]).id });
        navigate("jobs");
      }}
      className="block w-full rounded-lg border border-border bg-surface p-3 text-left transition-colors hover:border-border-strong"
      aria-label="Kuyruğu göster"
    >
      <div className="flex items-center gap-2 text-[12px]">
        {running ? <Loader2 size={13} className="shrink-0 animate-spin text-accent" /> : <StatusDot color="gray" />}
        <span className="min-w-0 flex-1 truncate font-medium">{running ? (JOB_KIND[running.kind]?.label ?? "İş") : "Sırada bekliyor"}</span>
        {waiting > 0 && <span className="shrink-0 text-[11px] text-muted tabular-nums">{running ? `+${waiting} sırada` : waiting}</span>}
      </div>
      {running && (
        <>
          <div className="mt-1 truncate text-[12px] text-text-2">{running.title}</div>
          <ProgressBar value={progress} className="mt-2" />
          <div className="mt-1.5 flex justify-between gap-2 text-[12px] text-muted tabular-nums">
            <span className="min-w-0 truncate">{l?.message ?? (l ? `${l.completed}/${l.total}` : "başlıyor…")}</span>
            {remaining !== null && <span className="shrink-0 whitespace-nowrap">~{formatDuration(remaining)}</span>}
          </div>
        </>
      )}
    </button>
  );
}

function useApiInfo() {
  const enabled = useStore((s) => s.settings?.api_enabled ?? false);
  const port = useStore((s) => s.settings?.api_port);
  const [info, setInfo] = useState<ApiInfo | null>(null);
  useEffect(() => {
    if (!enabled) return setInfo(null);
    const t = setTimeout(() => {
      api
        .getApiInfo()
        .then(setInfo)
        .catch(() => setInfo(null));
    }, 400);
    return () => clearTimeout(t);
  }, [enabled, port]);
  return enabled ? info : null;
}

function StatusFooter() {
  const { status, queue, page, navigate, set } = useStore();
  const apiInfo = useApiInfo();
  const apiEnabled = useStore((s) => s.settings?.api_enabled ?? false);
  if (!status) return null;
  const running = queue.some((j) => j.status === "running");
  const engine = running
    ? { color: "accent" as const, label: "Motor çalışıyor" }
    : status.engine.loaded
      ? { color: "green" as const, label: "Motor bellekte" }
      : { color: "gray" as const, label: "Motor boşta" };
  const openSettings = (section: string | null) => {
    set({ settingsSection: section });
    navigate("settings");
  };
  return (
    <div className="space-y-2 px-3 pb-3">
      <QueueCard />
      <div className="flex items-center gap-1.5 px-1">
        <button
          type="button"
          onClick={() => openSettings("engine")}
          className="flex min-w-0 flex-1 items-center gap-2 rounded-md py-1 text-left text-[12px] text-text-2 hover:text-text"
          title={status.engine.loaded ? "Model bellekte; sonraki iş hemen başlar" : "Model bellekte değil; ilk iş ~20 sn yükleme yapar"}
        >
          <StatusDot color={engine.color} />
          <span className="truncate">{engine.label}</span>
        </button>
        {apiEnabled && (
          <button
            type="button"
            onClick={() => openSettings("api")}
            title={apiInfo?.error ? `API çalışmıyor: ${apiInfo.error}` : apiInfo?.url ? `Yerel API: ${apiInfo.url}` : "Yerel API"}
            className="flex shrink-0 items-center gap-1.5 rounded-md px-1.5 py-1 font-mono text-[12px] text-text-2 hover:text-text"
          >
            <StatusDot color={apiInfo?.running ? "green" : apiInfo?.error ? "red" : "gray"} />
            API
          </button>
        )}
      </div>
      <button
        type="button"
        onClick={() => openSettings(null)}
        title={`Ayarlar (${modKey},)`}
        aria-current={page === "settings" ? "page" : undefined}
        className={cx(
          "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] transition-colors",
          page === "settings" ? "bg-surface font-medium shadow-[0_1px_2px_rgb(0_0_0/0.06)]" : "text-text-2 hover:bg-surface/60 hover:text-text",
        )}
      >
        <SettingsIcon size={16} className={page === "settings" ? "text-text" : "text-muted"} />
        Ayarlar
      </button>
    </div>
  );
}

function Sidebar({ needsSetup }: { needsSetup: boolean }) {
  const { page, queue } = useStore();
  const busy = (kinds: Job["kind"][]) => queue.filter((j) => kinds.includes(j.kind)).length;
  return (
    <nav aria-label="Ana menü" className="flex w-[228px] shrink-0 flex-col border-r border-border bg-sidebar">
      <div className="drag-region flex items-center gap-2.5 px-5 pt-11 pb-4">
        <LogoLockup size={26} />
      </div>
      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-3 pt-1 pb-3">
        {GROUPS.map((g) => (
          <div key={g.title}>
            <div className="px-2.5 pb-1.5 text-[12px] font-medium text-muted">{g.title}</div>
            <div className="space-y-0.5">
              {g.items.map((m) => (
                <NavItem key={m.page} entry={m} index={MENU.indexOf(m)} active={page === m.page && !needsSetup} disabled={needsSetup} busy={busy(m.kinds)} />
              ))}
            </div>
          </div>
        ))}
      </div>
      <StatusFooter />
    </nav>
  );
}

function Notifications() {
  const { notifications, dismissNotification } = useStore();
  return (
    <div aria-live="polite" className="pointer-events-none fixed right-5 bottom-5 z-[60] flex w-[360px] flex-col gap-2">
      {notifications.map((n) => (
        <div key={n.id} className="fade-in pointer-events-auto flex items-start gap-3 rounded-xl border border-border bg-surface px-4 py-3 shadow-overlay">
          <span className="mt-0.5">
            {n.kind === "success" ? (
              <CheckCircle2 size={16} className="text-green" />
            ) : n.kind === "error" ? (
              <XCircle size={16} className="text-red" />
            ) : (
              <Info size={16} className="text-muted" />
            )}
          </span>
          <div className="selectable min-w-0 flex-1 text-[13px] break-words">{n.text}</div>
          {n.action && (
            <button
              type="button"
              onClick={() => {
                n.action!.run();
                dismissNotification(n.id);
              }}
              className="text-[13px] font-medium text-accent-strong"
            >
              {n.action.label}
            </button>
          )}
          <button type="button" aria-label="Kapat" onClick={() => dismissNotification(n.id)} className="text-muted hover:text-text">
            <X size={14} />
          </button>
        </div>
      ))}
    </div>
  );
}

function useTheme() {
  const theme = useStore((s) => s.settings?.theme ?? "system");
  useEffect(() => {
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [theme]);
}

export default function App() {
  const { page, navigate, status, refreshStatus } = useStore();
  useTheme();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey) {
        const i = Number(e.key);
        if (i >= 1 && i <= MENU.length) {
          e.preventDefault();
          navigate(MENU[i - 1].page);
        } else if (e.key === ",") {
          e.preventDefault();
          navigate("settings");
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate]);

  // The engine unloads itself after idling; keep the sidebar's engine line current.
  useEffect(() => {
    const i = setInterval(() => document.visibilityState === "visible" && refreshStatus().catch(() => {}), 60_000);
    return () => clearInterval(i);
  }, [refreshStatus]);

  if (!status) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="fade-in">
          <Logo size={56} />
        </div>
      </div>
    );
  }
  const needsSetup = !status.model.ready || !status.engine.env_ready;

  return (
    <div className="flex h-full">
      <Sidebar needsSetup={needsSetup} />
      <main className="min-w-0 flex-1 bg-bg">
        {needsSetup && page !== "settings" ? (
          <Setup />
        ) : (
          <>
            {page === "studio" && <Studio />}
            {page === "design" && <Design />}
            {page === "library" && <Library />}
            {page === "clone" && <Clone />}
            {page === "timed" && <TimedNarration />}
            {page === "transcribe" && <Transcribe />}
            {page === "jobs" && <Jobs />}
            {page === "settings" && <Settings />}
          </>
        )}
      </main>
      <Notifications />
    </div>
  );
}

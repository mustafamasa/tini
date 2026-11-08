// Live state of a job: queued / running (progress, elapsed and estimated time) / failed; cancel.
import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import { AudioWaveform, Captions, Loader2, Mic, OctagonX, PencilRuler, RotateCcw, ScrollText, ShieldCheck, XCircle } from "lucide-react";
import { api, errorMessage, type Job } from "../api";
import { useStore } from "../store";
import { formatDuration } from "../languages";
import { Button, Card, ProgressBar, type BadgeColor } from "./ui";

export const JOB_STATUS: Record<Job["status"], { label: string; color: BadgeColor }> = {
  queued: { label: "sırada", color: "gray" },
  running: { label: "seslendiriliyor", color: "accent" },
  done: { label: "bitti", color: "green" },
  failed: { label: "hata", color: "red" },
  cancelled: { label: "iptal", color: "gray" },
  interrupted: { label: "yarım", color: "amber" },
};

/** Status label for a job: like JOB_STATUS, but a running transcript is "yazıya dökülüyor", not "seslendiriliyor". */
export function jobStatusLabel(job: Pick<Job, "kind" | "status">) {
  return job.kind === "transcribe" && job.status === "running" ? "yazıya dökülüyor" : JOB_STATUS[job.status].label;
}

/** Display name and icon of each job kind. */
export const JOB_KIND: Record<string, { label: string; icon: ReactNode }> = {
  read: { label: "Okuma", icon: <AudioWaveform size={15} /> },
  design: { label: "Ses tasarımı", icon: <PencilRuler size={15} /> },
  clone: { label: "Klon okuma", icon: <Mic size={15} /> },
  line: { label: "Satır yenileme", icon: <RotateCcw size={15} /> },
  transcribe: { label: "Transkript", icon: <ScrollText size={15} /> },
  review: { label: "Kalite kontrolü", icon: <ShieldCheck size={15} /> },
  timed: { label: "Altyazıdan seslendirme", icon: <Captions size={15} /> },
};

/** Watches a job record; reloads whenever a job finishes or the queue changes. */
export function useJob(id: string | null): Job | null {
  const revision = useStore((s) => s.revision);
  const queue = useStore((s) => s.queue);
  const [job, setJob] = useState<Job | null>(null);
  useEffect(() => {
    if (!id) {
      setJob(null);
      return;
    }
    let cancelled = false;
    api.getJob(id).then((j) => !cancelled && setJob(j));
    return () => {
      cancelled = true;
    };
  }, [id, revision, queue]);
  return job;
}

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

/** Re-renders once a second while active. */
function useTicker(active: boolean) {
  const [, setT] = useState(0);
  useEffect(() => {
    if (!active) return;
    const i = setInterval(() => setT((x) => x + 1), 1000);
    return () => clearInterval(i);
  }, [active]);
}

export function JobProgress({ job, onRetry }: { job: Job; onRetry?: () => void }) {
  const live = useStore((s) => s.live[job.id]);
  const queue = useStore((s) => s.queue);
  const notify = useStore((s) => s.notify);
  const running = job.status === "running" || !!live;
  useTicker(running);
  const cancel = async () => {
    try {
      await api.cancelJob(job.id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  if (job.status === "queued" && !live) {
    const position = queue.filter((j) => j.status === "queued").findIndex((j) => j.id === job.id) + 1;
    const busy = queue.some((j) => j.status === "running");
    return (
      <Card className="p-5">
        <div className="flex items-center gap-3">
          <Loader2 size={18} className="animate-spin text-muted" />
          <div className="flex-1">
            <div className="font-medium">Sırada</div>
            <div className="text-[13px] text-muted">
              {busy ? `Önünde ${position} iş var; GPU aynı anda tek iş yapar.` : "Başlamak üzere…"} Tahmini süre {formatDuration(job.estimated_s)}.
            </div>
          </div>
          <Button size="sm" variant="ghost" onClick={cancel} icon={<OctagonX size={14} />}>
            İptal
          </Button>
        </div>
      </Card>
    );
  }
  if (running) {
    const completed = live?.completed ?? job.completed;
    const total = Math.max(1, live?.total ?? job.total);
    const elapsed = live ? (Date.now() - live.startedAt) / 1000 : 0;
    const estimate = job.estimated_s ?? 0;
    const progress = estimate > 0 ? Math.min(0.97, Math.max(completed / total, elapsed / estimate)) : completed / total;
    const message =
      live?.message ??
      (completed === 0
        ? "Model yükleniyor, referans kodlanıyor…"
        : job.kind === "design"
          ? `${completed}/${total} adım hazır`
          : `${completed}/${total} satır hazır`);
    return (
      <Card className="p-5">
        <div className="mb-3 flex items-center gap-3">
          <div className="flex-1">
            <div className="font-medium">{job.kind === "transcribe" ? "Yazıya dökülüyor" : "Seslendiriliyor"}</div>
            <div className="text-[13px] text-muted">{message}</div>
          </div>
          <Button size="sm" variant="ghost" onClick={cancel} icon={<OctagonX size={14} />}>
            İptal
          </Button>
        </div>
        <ProgressBar value={progress} />
        <div className="mt-2 flex justify-between font-mono text-[12px] text-muted tabular-nums">
          <span>{formatDuration(Math.round(elapsed))}</span>
          <span>{estimate ? `~${formatDuration(Math.max(0, Math.round(estimate - elapsed)))} kaldı` : ""}</span>
        </div>
      </Card>
    );
  }
  if (job.status === "failed" || job.status === "cancelled" || job.status === "interrupted") {
    return (
      <Card className="p-5">
        <div className="flex items-start gap-3">
          <XCircle size={18} className={job.status === "failed" ? "mt-0.5 text-red" : "mt-0.5 text-muted"} />
          <div className="min-w-0 flex-1">
            <div className="font-medium">{job.status === "cancelled" ? "İptal edildi" : job.status === "interrupted" ? "Yarıda kaldı" : "Üretilemedi"}</div>
            {job.error && <div className="selectable mt-1 max-h-40 overflow-y-auto text-[13px] whitespace-pre-wrap text-text-2">{job.error}</div>}
          </div>
          {onRetry && (
            <Button size="sm" onClick={onRetry} icon={<RotateCcw size={13} />}>
              Yeniden dene
            </Button>
          )}
        </div>
      </Card>
    );
  }
  return null;
}

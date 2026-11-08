// Jobs: persistent history of every job, the queue, search and filters, details.
import { useEffect, useState } from "react";
import { Copy, History, PencilRuler, Search, Star, Trash2 } from "lucide-react";
import { TranscriptResult } from "./Transcribe";
import { api, errorMessage, type DesignVariant, type Job, type JobKind, type JobStatus } from "../api";
import { useStore } from "../store";
import { formatDate, formatDuration } from "../languages";
import { JOB_KIND, JOB_STATUS, JobProgress, jobStatusLabel, useDebounced, useJob } from "../components/JobProgress";
import { ReadingResult } from "../components/ReadingResult";
import { Select } from "../components/Select";
import { Badge, Button, ConfirmDialog, EmptyState, IconButton, Input, ProgressBar, TextArea, cx } from "../components/ui";

function JobRow({ job, selected, onSelect }: { job: Job; selected: boolean; onSelect: () => void }) {
  const live = useStore((s) => s.live[job.id]);
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      className={cx("flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition", selected ? "bg-surface-2 shadow-[inset_0_0_0_1px_var(--border)]" : "hover:bg-surface-2/60")}
    >
      <span className={cx("mt-0.5 flex", selected ? "text-text" : "text-muted")}>{JOB_KIND[job.kind]?.icon}</span>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className="truncate font-medium">{job.title || JOB_KIND[job.kind]?.label}</span>
          {job.starred && <Star size={11} className="shrink-0 text-amber" fill="currentColor" />}
        </div>
        <div className="truncate text-[12px] text-muted">
          {[JOB_KIND[job.kind]?.label, job.voice_name, formatDate(job.created_at), job.audio_s ? formatDuration(job.audio_s) : null].filter(Boolean).join(" · ")}
        </div>
        {live && <ProgressBar value={live.completed / Math.max(1, live.total)} className="mt-1.5" />}
      </div>
      {job.status !== "done" && <Badge color={JOB_STATUS[job.status].color}>{jobStatusLabel(job)}</Badge>}
    </button>
  );
}

function Details({ id, onDeleted }: { id: string; onDeleted: () => void }) {
  const job = useJob(id);
  const { set, navigate, notify } = useStore();
  const [title, setTitle] = useState("");
  const [notes, setNotes] = useState("");
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => {
    if (job) {
      setTitle(job.title);
      setNotes(job.notes);
    }
  }, [job?.id]); // eslint-disable-line react-hooks/exhaustive-deps
  if (!job) return null;
  const rerun = async () => {
    try {
      const next = await api.rerunJob(job.id);
      set({ selectedJob: next.id });
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  const deleteFiles = ["read", "clone", "transcribe", "timed"].includes(job.kind);
  const remove = async () => {
    try {
      if (job.kind === "design") await api.deleteDesign(job.id);
      else await api.deleteJob(job.id, deleteFiles);
      onDeleted();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  const active = job.status === "queued" || job.status === "running";
  const rtf = job.audio_s && job.duration_s ? (job.duration_s / job.audio_s).toFixed(2) : null;
  const variants = (job.result?.variants as DesignVariant[] | undefined) ?? [];
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="drag-region border-b border-border bg-surface px-6 pt-5 pb-4">
        <div className="mb-2 flex items-center gap-2">
          <Badge>{JOB_KIND[job.kind]?.label}</Badge>
          <Badge color={JOB_STATUS[job.status].color}>{jobStatusLabel(job)}</Badge>
          <div className="flex-1" />
          {job.kind !== "clone" && job.kind !== "line" && !active && (
            <Button size="sm" variant="ghost" icon={<Copy size={13} />} onClick={rerun}>
              Aynı ayarlarla yeniden
            </Button>
          )}
          <IconButton label={active ? "Sürerken silinemez" : "Sil"} onClick={() => setConfirmDelete(true)} disabled={active}>
            <Trash2 size={15} />
          </IconButton>
        </div>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title !== job.title && api.updateJob(job.id, { title })}
          aria-label="Başlık"
          className="-mx-1 w-full rounded-md bg-transparent px-1 py-0.5 text-[18px] font-semibold outline-none hover:bg-surface-2/60 focus:bg-surface-2"
        />
        <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-muted">
          <span>{formatDate(job.created_at)}</span>
          {job.voice_name && <span>{job.voice_name}</span>}
          {job.audio_s !== null && <span>{formatDuration(job.audio_s)} ses</span>}
          {job.duration_s !== null && <span>{formatDuration(job.duration_s)} üretim</span>}
          {rtf && <span title="Üretim süresi / ses süresi">RTF {rtf}</span>}
          {job.gpu_peak_mb !== null && <span>GPU belleği tepe {job.gpu_peak_mb} MB</span>}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-hidden bg-surface">
        {job.status !== "done" ? (
          <div className="h-full overflow-y-auto p-6">
            <JobProgress job={job} onRetry={job.kind !== "clone" ? rerun : undefined} />
            {job.text && <pre className="selectable mt-4 rounded-lg bg-surface-2 p-4 font-sans text-[13px] whitespace-pre-wrap text-text-2">{job.text}</pre>}
          </div>
        ) : job.kind === "read" || job.kind === "clone" || job.kind === "timed" ? (
          <ReadingResult id={job.id} hideTitle />
        ) : job.kind === "transcribe" ? (
          <TranscriptResult job={job} />
        ) : job.kind === "design" ? (
          <div className="p-6">
            <div className="mb-4 rounded-lg bg-surface-2 p-4 text-[13px]">
              <div className="mb-1 text-[12px] font-medium text-muted">Ses tarifi</div>
              <div className="selectable leading-relaxed text-text-2">{String(job.result?.description ?? job.text)}</div>
            </div>
            <div className="mb-4 text-[13px] text-muted">
              {variants.length} çeşit · {variants.filter((v) => v.library_voice_id).length} tanesi kitaplıkta
            </div>
            <Button
              variant="soft"
              icon={<PencilRuler size={14} />}
              onClick={() => {
                set({ designResult: job.id });
                navigate("design");
              }}
            >
              Tasarımı aç
            </Button>
          </div>
        ) : (
          <div className="p-6">
            <Button onClick={() => set({ selectedJob: job.parent_id })}>Okumayı aç</Button>
          </div>
        )}
      </div>
      <div className="border-t border-border bg-surface px-6 py-3">
        <TextArea
          rows={2}
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          onBlur={() => notes !== job.notes && api.updateJob(job.id, { notes })}
          placeholder="Not ekle…"
          aria-label="Not"
          className="text-[13px]"
        />
      </div>
      <ConfirmDialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        destructive
        title="Üretimi sil"
        confirmLabel="Sil"
        description={
          job.kind === "design"
            ? "Tasarım ve çeşitleri silinecek; kitaplığa eklediğiniz sesler kalır. Bu işlem geri alınamaz."
            : deleteFiles
              ? "Kayıt ve ses dosyaları silinecek. Bu işlem geri alınamaz."
              : "Kayıt geçmişten silinecek."
        }
        onConfirm={remove}
      />
    </div>
  );
}

export function Jobs() {
  const { selectedJob, set, revision, queue } = useStore();
  const [list, setList] = useState<Job[] | null>(null);
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState("");
  const [status, setStatus] = useState("");
  const [starred, setStarred] = useState(false);
  const debouncedSearch = useDebounced(search, 250);

  useEffect(() => {
    api
      .listJobs({ search: debouncedSearch || null, kind: (kind || null) as JobKind | null, status: (status || null) as JobStatus | null, starred })
      .then((l) => setList(l.filter((j) => !["line", "review"].includes(j.kind) || kind === j.kind)))
      .catch(() => setList([]));
  }, [debouncedSearch, kind, status, starred, revision, queue]);

  // Never leave the detail pane empty when there is something to show.
  useEffect(() => {
    if (list?.length && !selectedJob) set({ selectedJob: list[0].id });
  }, [list, selectedJob, set]);

  const filtered = !!(search || kind || status || starred);

  return (
    <div className="flex h-full min-h-0">
      <div className="flex w-[380px] shrink-0 flex-col border-r border-border bg-surface">
        <div className="drag-region border-b border-border px-5 pt-5 pb-3">
          <h1 className="text-[20px] leading-tight font-semibold">Üretimler</h1>
          <p className="mt-0.5 mb-3 text-[13px] text-muted">Bütün okumalar, tasarımlar, klonlar ve transkriptler</p>
          <div className="space-y-2">
            <div className="relative">
              <Search size={14} className="pointer-events-none absolute top-2.5 left-3 text-muted" />
              <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Metinde, başlıkta, notta ara…" className="pl-8" aria-label="Ara" />
            </div>
            <div className="flex gap-2">
              <Select
                value={kind}
                onChange={setKind}
                size="sm"
                aria-label="Tür"
                menuWidth={220}
                options={[
                  { value: "", label: "Bütün türler" },
                  { value: "read", label: "Okumalar", icon: JOB_KIND.read.icon },
                  { value: "design", label: "Ses tasarımları", icon: JOB_KIND.design.icon },
                  { value: "clone", label: "Klon okumaları", icon: JOB_KIND.clone.icon },
                  { value: "timed", label: "Altyazıdan seslendirmeler", icon: JOB_KIND.timed.icon },
                  { value: "transcribe", label: "Transkriptler", icon: JOB_KIND.transcribe.icon },
                  { value: "line", label: "Satır yenilemeleri", icon: JOB_KIND.line.icon },
                  { value: "review", label: "Kalite kontrolleri", icon: JOB_KIND.review.icon },
                ]}
              />
              <Select
                value={status}
                onChange={setStatus}
                size="sm"
                aria-label="Durum"
                options={[
                  { value: "", label: "Her durum" },
                  { value: "done", label: "Bitenler" },
                  { value: "failed", label: "Hatalılar" },
                  { value: "cancelled", label: "İptaller" },
                  { value: "interrupted", label: "Yarım kalanlar" },
                ]}
              />
              <button
                type="button"
                onClick={() => setStarred(!starred)}
                aria-pressed={starred}
                aria-label="Yalnız yıldızlılar"
                title="Yalnız yıldızlılar"
                className={cx(
                  "flex h-8 shrink-0 items-center rounded-lg border px-2.5 shadow-xs transition",
                  starred ? "border-amber/50 bg-amber-soft text-amber" : "border-border bg-surface text-muted hover:border-border-strong",
                )}
              >
                <Star size={14} fill={starred ? "currentColor" : "none"} />
              </button>
            </div>
          </div>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {list?.map((j) => (
            <JobRow key={j.id} job={j} selected={j.id === selectedJob} onSelect={() => set({ selectedJob: j.id })} />
          ))}
          {list && !list.length && (
            <div className="px-4 py-10 text-center text-[13px] text-muted">
              {filtered ? (
                <>
                  Eşleşen kayıt yok.{" "}
                  <button
                    type="button"
                    className="font-medium text-accent-strong hover:underline"
                    onClick={() => {
                      setSearch("");
                      setKind("");
                      setStatus("");
                      setStarred(false);
                    }}
                  >
                    Süzgeçleri temizle
                  </button>
                </>
              ) : (
                "Henüz üretim yok."
              )}
            </div>
          )}
        </div>
      </div>
      <div className="min-w-0 flex-1">
        {selectedJob ? (
          <Details key={selectedJob} id={selectedJob} onDeleted={() => set({ selectedJob: null })} />
        ) : (
          <div className="flex h-full items-center justify-center">
            <EmptyState
              icon={<History size={22} />}
              title="Üretim arşivi"
              description="Yaptığınız her okuma, tasarım, klon ve transkript burada kalır; arayabilir, yeniden üretebilir, dışa aktarabilirsiniz"
            />
          </div>
        )}
      </div>
    </div>
  );
}

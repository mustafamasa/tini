// Transcribe: turns an audio or video file into text (whisper.cpp); exports subtitles or sends them to timed narration.
// Until the whisper model is downloaded the page is an onboarding view that explains what it does.
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { AlertTriangle, Captions, Download, FileAudio, ScrollText, Upload, X } from "lucide-react";
import { api, errorMessage, type Job } from "../api";
import { useStore } from "../store";
import { LANGUAGES, formatBytes, formatClock, formatDate, formatDuration } from "../languages";
import { JobProgress, jobStatusLabel, useJob } from "../components/JobProgress";
import { ExportMenu } from "../components/ReadingResult";
import { AudioPlayer, type AudioPlayerHandle } from "../components/AudioPlayer";
import { HistoryList } from "../components/HistoryList";
import { Select } from "../components/Select";
import { Button, Card, EmptyState, Field, Flag, IconButton, PageHeader, ProgressBar, SectionLabel, cx } from "../components/ui";

const MEDIA_EXTENSIONS = ["wav", "mp3", "flac", "ogg", "m4a", "aac", "aiff", "aif", "caf", "mp4", "mov", "m4v"];

// What the page produces, shown before the model is downloaded: a real-looking transcript with the spoken word marked.
const SAMPLE_LINES: { t: string; text: string; word?: string }[] = [
  { t: "0:00", text: "Kasabanın tek feneri yüz yıldır her gece yanıyor." },
  { t: "0:04", text: "Hasan Usta bu ışığı kırk yıl boyunca bir gece bile söndürmedi.", word: "ışığı" },
  { t: "0:09", text: "Fırtınalı gecelerde merdivenleri saatte bir çıkıp camları silerdi." },
  { t: "0:15", text: "Şimdi fener otomatik, ama kasaba onu hâlâ Hasan Usta'nın feneri diye anıyor." },
];

const CAPABILITIES: { title: string; description: string }[] = [
  { title: "Kelime zamanları", description: "Oynatırken her kelime söylendiği anda vurgulanır" },
  { title: "SRT, VTT, TXT", description: "Sonucu altyazı ya da düz metin olarak dışa aktarın" },
  { title: "99 dil", description: "Konuşmanın dilini kendiliğinden tanır, Türkçe dahil" },
  { title: "Altyazıdan seslendirme", description: "Altyazıyı başka bir sesle, zamanlarına oturtarak yeniden okutun" },
];

function SampleTranscript() {
  return (
    <figure className="mt-6">
      <Card className="overflow-hidden">
        <div className="flex items-center gap-2 border-b border-border px-4 py-2.5 text-[12px] text-muted">
          <FileAudio size={14} className="shrink-0" />
          <span className="min-w-0 flex-1 truncate text-text-2">fener-belgeseli.m4a</span>
          <span className="font-mono tabular-nums">Türkçe · 0:21</span>
        </div>
        <div className="space-y-0.5 px-2 py-2" aria-hidden>
          {SAMPLE_LINES.map((l) => (
            <div key={l.t} className={cx("flex gap-3 rounded-lg px-3 py-1.5", l.word && "bg-accent-soft/70")}>
              <span className="w-9 shrink-0 pt-px text-right font-mono text-[12px] text-muted tabular-nums">{l.t}</span>
              <span className="text-[14px] leading-relaxed">
                {l.word
                  ? l.text.split(l.word).flatMap((part, i) => (i ? [<span key={i} className="playing-word">{l.word}</span>, part] : [part]))
                  : l.text}
              </span>
            </div>
          ))}
        </div>
      </Card>
      <figcaption className="mt-2 px-1 text-[12px] text-muted">Örnek sonuç: zamanlı satırlar, çalan kelime vurgulu</figcaption>
    </figure>
  );
}

function Onboarding() {
  const { status, whisper, notify, set } = useStore();
  if (!status) return null;
  const busy = whisper && (whisper.stage === "downloading" || whisper.stage === "verifying");
  const download = () => {
    set({ whisper: { stage: "downloading", current: 0, total: status.whisper.size, speed_mb_s: 0, message: null } });
    api.downloadWhisperModel().catch((e) => notify(errorMessage(e), "error"));
  };
  const progress = busy && whisper!.total ? whisper!.current / whisper!.total : 0;
  const remaining = busy && whisper!.stage === "downloading" && whisper!.speed_mb_s > 0 ? (whisper!.total - whisper!.current) / 1e6 / whisper!.speed_mb_s : null;
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto grid max-w-[1040px] items-start gap-10 px-8 py-10 lg:grid-cols-[1fr_340px]">
        <div className="min-w-0">
          <h2 className="text-[22px] leading-tight font-semibold">Konuşmayı yazıya ve altyazıya dökün</h2>
          <p className="mt-2 max-w-xl text-[14px] leading-relaxed text-text-2">
            Bir ses ya da video dosyasını bu bilgisayarda, cümle cümle ve kelime zamanlarıyla yazıya döker. Model bir kez indirilir; sonrasında internet
            gerekmez.
          </p>
          <SampleTranscript />
          <dl className="mt-8 grid gap-x-8 gap-y-4 sm:grid-cols-2">
            {CAPABILITIES.map((c) => (
              <div key={c.title}>
                <dt className="text-[13px] font-medium">{c.title}</dt>
                <dd className="mt-0.5 text-[13px] leading-snug text-muted">{c.description}</dd>
              </div>
            ))}
          </dl>
        </div>
        <Card className="lg:sticky lg:top-0">
          <div className="px-5 pt-5 pb-4">
            <div className="text-[15px] font-semibold">Whisper large-v3-turbo</div>
            <div className="mt-0.5 text-[12px] text-muted">Konuşma tanıma modeli · {formatBytes(status.whisper.size)}</div>
            <ul className="mt-4 space-y-2 text-[13px] leading-snug text-text-2">
              <li>Sesleriniz bu bilgisayardan çıkmaz</li>
              <li>İndirme yarıda kalırsa kaldığı yerden sürer</li>
              <li>Stüdyo okumalarının kalite kontrolü ve kelime zamanları da bu modelle yapılır</li>
            </ul>
          </div>
          <div className="border-t border-border px-5 py-4">
            {!status.whisper.binary_present ? (
              <div className="flex gap-2 text-[13px] text-red">
                <AlertTriangle size={14} className="mt-0.5 shrink-0" /> Transkript motoru (whisper) uygulama paketinde yok; uygulamayı yeniden kurun
              </div>
            ) : busy ? (
              <>
                <div className="mb-2 flex items-baseline justify-between text-[13px]">
                  <span className="font-medium">{whisper!.stage === "verifying" ? "Doğrulanıyor…" : "İndiriliyor…"}</span>
                  <span className="text-muted tabular-nums">%{Math.round(progress * 100)}</span>
                </div>
                <ProgressBar value={progress} />
                <div className="mt-2 flex items-center justify-between gap-2 text-[12px] text-muted tabular-nums">
                  <span className="truncate">
                    {formatBytes(whisper!.current)} / {formatBytes(whisper!.total)}
                    {whisper!.speed_mb_s ? ` · ${whisper!.speed_mb_s} MB/sn` : ""}
                    {remaining ? ` · ~${formatDuration(Math.round(remaining))}` : ""}
                  </span>
                  <Button size="sm" variant="ghost" icon={<X size={13} />} onClick={() => api.cancelWhisperDownload()}>
                    İptal
                  </Button>
                </div>
              </>
            ) : (
              <>
                <Button variant="primary" size="lg" className="w-full" icon={<Download size={16} />} onClick={download}>
                  Modeli indir
                </Button>
                {whisper?.stage === "error" && <div className="mt-2 text-[13px] text-red">{whisper.message}</div>}
                {whisper?.stage === "cancelled" && <div className="mt-2 text-[12px] text-muted">İndirme durduruldu; yeniden başlatınca kaldığı yerden sürer</div>}
              </>
            )}
          </div>
        </Card>
      </div>
    </div>
  );
}

export function TranscriptResult({ job }: { job: Job }) {
  const { set, navigate } = useStore();
  const [time, setTime] = useState(0);
  const player = useRef<AudioPlayerHandle>(null);
  const lines = (job.result?.lines as { line: number; text: string; t0: number; t1: number }[]) ?? [];
  const language = String(job.result?.language ?? "");
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-border px-6 py-4">
        <div className="mb-3 flex items-center gap-2">
          <div className="min-w-0 flex-1">
            <div className="truncate text-[15px] font-semibold">{job.title}</div>
            <div className="flex items-center gap-1.5 text-[12px] text-muted">
              {formatDate(job.finished_at)} · {formatDuration(job.audio_s)} ses · {formatDuration(job.duration_s)} işlem
              {language && (
                <>
                  {" "}
                  · <Flag language={language} size={9} /> {language.toUpperCase()}
                </>
              )}
            </div>
          </div>
          <Button
            size="sm"
            variant="soft"
            icon={<Captions size={14} />}
            onClick={() => {
              set({ timedCues: lines.map((l) => ({ t0: l.t0, t1: l.t1, text: l.text })), timedResult: null });
              navigate("timed");
            }}
          >
            Bu altyazıyı seslendir
          </Button>
          <ExportMenu job={job} hasWords={((job.result?.words as unknown[]) ?? []).length > 0} hasText />
        </div>
        <AudioPlayer ref={player} path={job.result?.wav as string} version={job.id} onTime={setTime} />
      </div>
      <div className="selectable min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {lines.map((l, i) => {
          const playing = time >= l.t0 && time < (lines[i + 1]?.t0 ?? l.t1 + 0.5);
          return (
            <div key={l.line} className={cx("flex gap-3 rounded-lg px-3 py-1.5", playing && "bg-accent-soft/70")}>
              <button
                type="button"
                onClick={() => player.current?.play(l.t0)}
                className="w-12 shrink-0 text-right font-mono text-[12px] text-muted hover:text-accent"
                title="Buradan çal"
              >
                {formatClock(l.t0)}
              </button>
              <div className="text-[14px] leading-relaxed">{l.text}</div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function Transcribe() {
  const { status, transcribeResult, set, notify, revision, whisper } = useStore();
  const [file, setFile] = useState<string | null>(null);
  const [language, setLanguage] = useState("tr");
  const [history, setHistory] = useState<Job[]>([]);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const job = useJob(transcribeResult);
  const downloading = whisper && (whisper.stage === "downloading" || whisper.stage === "verifying");
  const ready = !!status?.whisper.model_ready && !!status?.whisper.binary_present && !downloading;

  useEffect(() => {
    api
      .listJobs({ kind: "transcribe", limit: 50 })
      .then(setHistory)
      .catch(() => {});
  }, [revision, transcribeResult]);

  useEffect(() => {
    if (!ready) return;
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          const path = e.payload.paths.find((p) => MEDIA_EXTENSIONS.includes(p.split(".").pop()?.toLowerCase() ?? ""));
          if (path) {
            setFile(path);
            setError(null);
          } else notify("Ses ya da video dosyası bırakın", "error");
        }
      })
      .then((f) => (unlisten = f))
      .catch(() => {});
    return () => unlisten?.();
  }, [ready, notify]);

  const start = async () => {
    if (!file) return setError("Önce bir ses ya da video dosyası seçin");
    try {
      const j = await api.enqueueJob({ kind: "transcribe", file, language: language === "auto" ? null : language });
      set({ transcribeResult: j.id });
      setFile(null);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  if (!ready) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <PageHeader title="Transkript" description="Ses ya da videoyu yazıya ve altyazıya dökün" />
        <Onboarding />
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="Transkript" description="Ses ya da videoyu yazıya ve altyazıya dökün; SRT/VTT olarak dışa aktarın ya da başka bir sesle seslendirin" />
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[360px] shrink-0 flex-col border-r border-border bg-surface">
          <div className="space-y-4 border-b border-border px-5 py-5">
            <Field label="Dosya" extra="ses ya da video" error={error}>
              <div
                className={cx(
                  "rounded-xl border border-dashed p-4 transition-colors",
                  dragging ? "border-text-2 bg-surface-2" : error ? "border-red/50" : "border-border-strong",
                )}
              >
                {file ? (
                  <div className="flex items-center gap-2">
                    <FileAudio size={16} className="shrink-0 text-muted" />
                    <span className="min-w-0 flex-1 truncate font-medium" title={file}>
                      {file.split(/[\\/]/).pop()}
                    </span>
                    <IconButton label="Dosyayı kaldır" onClick={() => setFile(null)}>
                      <X size={14} />
                    </IconButton>
                  </div>
                ) : (
                  <div className="flex flex-col items-center gap-2 py-1 text-center">
                    <Upload size={18} className="text-muted" />
                    <div className="text-[13px] text-muted">Dosyayı buraya bırakın ya da</div>
                    <Button
                      size="sm"
                      onClick={async () => {
                        const f = await open({ filters: [{ name: "Ses / video", extensions: MEDIA_EXTENSIONS }] });
                        if (typeof f === "string") {
                          setFile(f);
                          setError(null);
                        }
                      }}
                    >
                      Dosya seç
                    </Button>
                  </div>
                )}
              </div>
            </Field>
            <Field label="Konuşmanın dili">
              <Select
                value={language}
                onChange={setLanguage}
                aria-label="Konuşmanın dili"
                options={[
                  { value: "auto", label: "Otomatik tanı" },
                  ...LANGUAGES.map((l) => ({ value: l.code, label: l.name, icon: <Flag language={l.code} size={10} /> })),
                ]}
              />
            </Field>
            <Button variant="primary" size="lg" className="w-full" onClick={start} icon={<ScrollText size={16} />}>
              Yazıya dök
            </Button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-2 py-3">
            <SectionLabel className="px-3">Önceki transkriptler</SectionLabel>
            <HistoryList
              jobs={history}
              selected={transcribeResult}
              onSelect={(id) => set({ transcribeResult: id })}
              meta={(h) => `${formatDate(h.created_at)} · ${h.status === "done" ? formatDuration(h.audio_s) : jobStatusLabel(h)}`}
              empty={<div className="px-3 text-[13px] text-muted">Henüz transkript yok</div>}
            />
          </div>
        </div>
        <div className="min-w-0 flex-1 bg-surface">
          {job ? (
            job.status === "done" ? (
              <TranscriptResult job={job} />
            ) : (
              <div className="p-6">
                <JobProgress job={job} />
              </div>
            )
          ) : (
            <div className="flex h-full items-center justify-center">
              <EmptyState
                icon={<ScrollText size={22} />}
                title="Bir dosyayı yazıya dökün"
                description="Soldan bir ses ya da video dosyası seçin. Sonuç cümle cümle, zamanlarıyla burada görünür; tıkladığınız yerden çalar."
              />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

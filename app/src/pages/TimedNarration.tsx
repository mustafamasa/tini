// Timed narration: reads SRT/VTT cues with the chosen voice, placing each cue at its own time; optionally muxes the
// result into a video.
import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Captions, FileVideo, FolderOpen, Plus, ScrollText, SpellCheck, Trash2, X } from "lucide-react";
import { api, errorIssues, errorMessage, fileUrl, type Cue, type Issue, type Job } from "../api";
import { useStore } from "../store";
import { formatClock, formatDuration, voiceName } from "../languages";
import { JobProgress, useJob } from "../components/JobProgress";
import { ReadingResult } from "../components/ReadingResult";
import { VoicePicker } from "../components/VoicePicker";
import { HistoryList } from "../components/HistoryList";
import { Select } from "../components/Select";
import { Badge, Button, EmptyState, Field, IconButton, Input, IssueList, PageHeader, Segmented, cx, inputClass } from "../components/ui";

/** m:ss.mmm */
function formatTimestamp(seconds: number) {
  const ms = Math.round((seconds % 1) * 1000);
  return `${formatClock(seconds)}.${ms.toString().padStart(3, "0")}`;
}

function parseTimestamp(s: string): number | null {
  const p = s.trim().replace(",", ".").split(":").map(Number);
  if (p.some((x) => isNaN(x))) return null;
  return p.length === 3 ? p[0] * 3600 + p[1] * 60 + p[2] : p.length === 2 ? p[0] * 60 + p[1] : p[0];
}

function TimeInput({ value, onChange, label }: { value: number; onChange: (v: number) => void; label: string }) {
  const [text, setText] = useState(formatTimestamp(value));
  useEffect(() => setText(formatTimestamp(value)), [value]);
  return (
    <input
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const v = parseTimestamp(text);
        if (v !== null) onChange(v);
        else setText(formatTimestamp(value));
      }}
      aria-label={label}
      className="h-7 w-[78px] rounded-md bg-transparent px-1.5 font-mono text-[12px] text-text-2 outline-none hover:bg-surface-2 focus:bg-surface-2"
    />
  );
}

function TimedResult({ job }: { job: Job }) {
  const r = job.result ?? {};
  return (
    <div className="flex h-full min-h-0 flex-col">
      {(r.video || r.video_error || r.sped_up_count > 0 || r.shifted_count > 0) && (
        <div className="space-y-2 border-b border-border px-6 py-3">
          {r.video && (
            <div className="flex items-center gap-3">
              <video src={fileUrl(r.video as string)} controls className="max-h-56 rounded-lg bg-black" />
              <Button size="sm" icon={<FolderOpen size={13} />} onClick={() => revealItemInDir(r.video as string)}>
                Videoyu göster
              </Button>
            </div>
          )}
          {r.video_error && <div className="text-[13px] text-amber">{String(r.video_error)}</div>}
          {(r.sped_up_count > 0 || r.shifted_count > 0) && (
            <div className="text-[13px] text-text-2">
              {r.sped_up_count > 0 && `${r.sped_up_count} altyazı satırı sığması için hızlandırıldı (en çok ×${r.max_speedup}). `}
              {r.shifted_count > 0 && `${r.shifted_count} altyazı satırı aralığına sığmadı ve geç başladı; metni kısaltmayı ya da hızlandırma sınırını artırmayı deneyin.`}
            </div>
          )}
        </div>
      )}
      <div className="min-h-0 flex-1">
        <ReadingResult id={job.id} />
      </div>
    </div>
  );
}

export function TimedNarration() {
  const { timedCues, timedResult, set, voices, studioVoice, notify, status, revision, navigate } = useStore();
  const [cues, setCues] = useState<Cue[]>(() => {
    try {
      return JSON.parse(localStorage.getItem("tini.timedCues") ?? "[]");
    } catch {
      return [];
    }
  });
  const [voice, setVoice] = useState<string | null>(studioVoice);
  const [speakerVoices, setSpeakerVoices] = useState<Record<string, string>>({});
  const [maxSpeedup, setMaxSpeedup] = useState(1.25);
  const [video, setVideo] = useState<string | null>(null);
  const [originalAudio, setOriginalAudio] = useState<"duck" | "remove">("duck");
  const [title, setTitle] = useState("");
  const [issues, setIssues] = useState<Issue[]>([]);
  const [history, setHistory] = useState<Job[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [tab, setTab] = useState<"result" | "history">(timedResult ? "result" : "history");
  const job = useJob(timedResult);

  useEffect(() => {
    if (timedCues) {
      setCues(timedCues);
      set({ timedCues: null });
    }
  }, [timedCues, set]);
  useEffect(() => localStorage.setItem("tini.timedCues", JSON.stringify(cues)), [cues]);
  useEffect(() => {
    api
      .listJobs({ kind: "timed", limit: 30 })
      .then(setHistory)
      .catch(() => {});
  }, [revision, timedResult]);
  useEffect(() => {
    if (timedResult) setTab("result");
  }, [timedResult]);

  const speakers = useMemo(() => [...new Set(cues.map((c) => c.speaker).filter(Boolean) as string[])], [cues]);
  const totalDuration = cues.length ? Math.max(...cues.map((c) => c.t1)) : 0;

  const updateCue = (i: number, p: Partial<Cue>) => setCues(cues.map((c, j) => (j === i ? { ...c, ...p } : c)));

  const openSubtitles = async () => {
    const path = await open({ filters: [{ name: "Altyazı", extensions: ["srt", "vtt"] }] });
    if (typeof path !== "string") return;
    try {
      setCues(await api.parseSubtitles(path));
      setIssues([]);
      if (!title)
        setTitle(
          path
            .split(/[\\/]/)
            .pop()
            ?.replace(/\.(srt|vtt)$/i, "") ?? "",
        );
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  const autoFix = async () => {
    let changed = 0;
    const fixed = await Promise.all(
      cues.map(async (c) => {
        const r = await api.fixText(c.text, null, voice, { numbers: true, abbreviations: true, citations: true, parentheses: true, quotes: true, sentences: false });
        if (r.text !== c.text) changed++;
        return { ...c, text: r.text.replace(/\n+/g, " ") };
      }),
    );
    setCues(fixed);
    notify(changed ? `${changed} satır düzeltildi` : "Düzeltilecek bir şey yok", changed ? "success" : "info");
  };

  const start = async () => {
    if (!cues.length) return setIssues([{ code: "missing", message: "Önce bir SRT / VTT dosyası açın ya da Transkript sayfasından altyazı gönderin", data: {} }]);
    if (!voice) return setIssues([{ code: "missing", message: "Okuyacak sesi seçin", data: {} }]);
    setIssues([]);
    setSubmitting(true);
    try {
      const j = await api.enqueueJob({
        kind: "timed",
        voice,
        cues,
        speaker_voices: speakerVoices,
        max_speedup: maxSpeedup,
        video,
        original_audio: originalAudio,
        title: title || null,
      });
      set({ timedResult: j.id });
    } catch (e) {
      const found = errorIssues(e);
      if (found.length) setIssues(found);
      else notify(errorMessage(e), "error");
    } finally {
      setSubmitting(false);
    }
  };

  const showResult = tab === "result" && job;
  const voiceOptions = [{ value: "", label: "Ana ses" }, ...voices.map((v) => ({ value: v.id, label: voiceName(v) }))];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Altyazıdan seslendirme"
        description="SRT / VTT altyazıyı seçtiğiniz sesle, her satırı kendi zamanına oturtarak okutun; isterseniz videoya bindirin"
        actions={
          <Button icon={<Captions size={15} />} onClick={openSubtitles}>
            SRT / VTT aç
          </Button>
        }
      />
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[560px] shrink-0 flex-col border-r border-border bg-surface">
          <div className="space-y-4 border-b border-border px-6 py-4">
            <div className="grid grid-cols-[1fr_190px] items-start gap-4">
              <Field label="Ses">
                <VoicePicker value={voice} onChange={setVoice} wide />
              </Field>
              <Field label="En çok hızlandırma" extra={`×${maxSpeedup.toLocaleString("tr-TR")}`} hint="Sığmayan satır perdesi değişmeden hızlanır">
                <input
                  type="range"
                  min={1}
                  max={1.5}
                  step={0.05}
                  value={maxSpeedup}
                  aria-label="En çok hızlandırma"
                  onChange={(e) => setMaxSpeedup(Number(e.target.value))}
                  className="mt-2.5 w-full accent-[var(--accent)]"
                />
              </Field>
            </div>
            {speakers.length > 0 && (
              <Field label="Konuşmacılar" hint="Seçilmeyen konuşmacıları ana ses okur">
                <div className="space-y-1.5">
                  {speakers.map((sp) => (
                    <div key={sp} className="flex items-center gap-2">
                      <span className="w-28 truncate text-[13px] font-medium">{sp}</span>
                      <Select
                        value={speakerVoices[sp] ?? ""}
                        onChange={(v) => setSpeakerVoices({ ...speakerVoices, [sp]: v })}
                        options={voiceOptions}
                        size="sm"
                        aria-label={`${sp} sesi`}
                      />
                    </div>
                  ))}
                </div>
              </Field>
            )}
            <div className="flex items-center gap-2 rounded-lg bg-surface-2 px-3 py-2">
              <FileVideo size={15} className="shrink-0 text-muted" />
              {video ? (
                <>
                  <span className="min-w-0 flex-1 truncate text-[13px] font-medium" title={video}>
                    {video.split(/[\\/]/).pop()}
                  </span>
                  <Segmented
                    size="sm"
                    value={originalAudio}
                    onChange={setOriginalAudio}
                    aria-label="Videonun kendi sesi"
                    options={[
                      { value: "duck", label: "Orijinali kıs" },
                      { value: "remove", label: "Kaldır" },
                    ]}
                  />
                  <IconButton label="Videoyu kaldır" onClick={() => setVideo(null)}>
                    <X size={14} />
                  </IconButton>
                </>
              ) : (
                <>
                  <span className="flex-1 text-[13px] text-muted">
                    {status?.ffmpeg ? "İsteğe bağlı: sesi bir videoya bindirin" : "Videoya bindirmek için ffmpeg gerekir (bulunamadı)"}
                  </span>
                  <Button
                    size="sm"
                    disabled={!status?.ffmpeg}
                    onClick={async () => {
                      const v = await open({ filters: [{ name: "Video", extensions: ["mp4", "mov", "m4v", "mkv", "webm"] }] });
                      if (typeof v === "string") setVideo(v);
                    }}
                  >
                    Video seç
                  </Button>
                </>
              )}
            </div>
          </div>
          <div className="flex items-center gap-1 px-6 pt-3 pb-2">
            <div className="text-[12px] font-medium text-text-2">
              {cues.length} satır · {formatDuration(totalDuration)}
            </div>
            <div className="flex-1" />
            {cues.length > 0 && (
              <>
                <Button size="sm" variant="ghost" icon={<SpellCheck size={13} />} onClick={autoFix} title="Sayıları yazıya çevir, kısaltmaları aç…">
                  Metni düzelt
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  icon={<Plus size={13} />}
                  onClick={() => {
                    const last = cues[cues.length - 1];
                    setCues([...cues, { t0: last ? last.t1 + 0.3 : 0, t1: last ? last.t1 + 3.3 : 3, text: "" }]);
                  }}
                >
                  Satır ekle
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setCues([])}>
                  Temizle
                </Button>
              </>
            )}
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-3">
            {cues.map((c, i) => (
              <div key={i} className="group flex items-start gap-1 rounded-lg px-2 py-1 hover:bg-surface-2">
                <span className="w-6 pt-1.5 text-right font-mono text-[11px] text-muted">{i + 1}</span>
                <div className="flex shrink-0 flex-col">
                  <TimeInput value={c.t0} onChange={(v) => updateCue(i, { t0: v })} label={`Satır ${i + 1} başlangıç`} />
                  <TimeInput value={c.t1} onChange={(v) => updateCue(i, { t1: v })} label={`Satır ${i + 1} bitiş`} />
                </div>
                <textarea
                  value={c.text}
                  rows={Math.max(2, Math.ceil(c.text.length / 52))}
                  onChange={(e) => updateCue(i, { text: e.target.value })}
                  aria-label={`Satır ${i + 1} metni`}
                  className={cx(inputClass, "w-full resize-none py-1 text-[13px]")}
                />
                <IconButton label={`Satır ${i + 1} sil`} onClick={() => setCues(cues.filter((_, j) => j !== i))} className="mt-0.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100 hover:text-red">
                  <Trash2 size={13} />
                </IconButton>
              </div>
            ))}
            {!cues.length && (
              <EmptyState
                icon={<Captions size={22} />}
                title="Altyazı yükleyin"
                description="Her satır kendi başlangıç zamanında okunur. Aralığına sığmayan ses hızlandırılır; yine sığmazsa sonraki satırlar kayar ve size bildirilir."
                action={
                  <>
                    <Button variant="primary" icon={<Captions size={15} />} onClick={openSubtitles}>
                      SRT / VTT aç
                    </Button>
                    <Button icon={<ScrollText size={15} />} onClick={() => navigate("transcribe")}>
                      Transkriptten oluştur
                    </Button>
                  </>
                }
              />
            )}
          </div>
          <div className="space-y-2 border-t border-border px-6 py-3">
            <IssueList issues={issues} />
            <div className="flex items-center gap-3">
              <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Başlık (isteğe bağlı)" aria-label="Başlık" className="h-10 flex-1" />
              <Button variant="primary" size="lg" icon={<Captions size={16} />} loading={submitting} onClick={start}>
                Seslendir
              </Button>
            </div>
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col bg-surface">
          <div className="flex min-h-12 items-center gap-3 border-b border-border px-5 py-2.5">
            <Segmented
              size="sm"
              value={showResult ? "result" : "history"}
              onChange={setTab}
              aria-label="Sonuç paneli"
              options={[
                { value: "result", label: "Sonuç" },
                { value: "history", label: `Önceki seslendirmeler${history.length ? ` (${history.length})` : ""}` },
              ]}
            />
            <div className="flex-1" />
            {job && (job.status === "queued" || job.status === "running") && <Badge color="accent">{job.status === "running" ? "seslendiriliyor" : "sırada"}</Badge>}
          </div>
          <div className="min-h-0 flex-1">
            {showResult ? (
              job.status === "done" ? (
                <TimedResult job={job} />
              ) : (
                <div className="p-6">
                  <JobProgress job={job} />
                </div>
              )
            ) : (
              <div className="h-full overflow-y-auto p-3">
                <HistoryList
                  jobs={history}
                  selected={timedResult}
                  onSelect={(id) => {
                    set({ timedResult: id });
                    setTab("result");
                  }}
                  empty={
                    <EmptyState
                      icon={<Captions size={22} />}
                      title="Henüz seslendirme yok"
                      description="Altyazıyı yükleyip sesi seçin ve seslendirin. Sonuç burada satır satır çalınır; videoya bindirdiyseniz video da burada görünür."
                    />
                  }
                />
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

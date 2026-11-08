// Consent-based cloning in three steps: recording → its exact text → text to read (optionally saved to the library).
// A single consent checkbox is the only consent input.
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Check, CircleStop, FileAudio, Lock, Mic, ScrollText, Upload, X } from "lucide-react";
import { api, errorIssues, errorMessage, type Issue, type Job } from "../api";
import { useStore } from "../store";
import { formatDuration } from "../languages";
import { JobProgress, useJob } from "../components/JobProgress";
import { ReadingResult } from "../components/ReadingResult";
import { AudioPlayer } from "../components/AudioPlayer";
import { HistoryList } from "../components/HistoryList";
import { Badge, Button, Checkbox, Disclosure, EmptyState, Field, IconButton, Input, IssueList, PageHeader, Segmented, TextArea, cx } from "../components/ui";

const CONSENT_TEXT = "Bu ses bana ait ya da sahibinden açık izin aldım; taklit, dolandırıcılık ya da yanıltma için kullanmayacağım.";
const AUDIO_EXTENSIONS = ["wav", "mp3", "flac", "ogg", "m4a", "aac", "aiff", "aif", "caf"];

/** Encodes mono float chunks as 16-bit PCM WAV. */
function encodeWav(chunks: Float32Array[], sampleRate: number): Uint8Array {
  const n = chunks.reduce((t, c) => t + c.length, 0);
  const buf = new ArrayBuffer(44 + n * 2);
  const v = new DataView(buf);
  const writeAscii = (o: number, s: string) => [...s].forEach((c, i) => v.setUint8(o + i, c.charCodeAt(0)));
  writeAscii(0, "RIFF");
  v.setUint32(4, 36 + n * 2, true);
  writeAscii(8, "WAVEfmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, sampleRate, true);
  v.setUint32(28, sampleRate * 2, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  writeAscii(36, "data");
  v.setUint32(40, n * 2, true);
  let o = 44;
  for (const c of chunks) for (let i = 0; i < c.length; i++, o += 2) v.setInt16(o, Math.max(-1, Math.min(1, c[i])) * 0x7fff, true);
  return new Uint8Array(buf);
}

function Recorder({ onDone }: { onDone: (path: string) => void }) {
  const notify = useStore((s) => s.notify);
  const [recording, setRecording] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [level, setLevel] = useState(0);
  const ref = useRef<{ ctx: AudioContext; stream: MediaStream; chunks: Float32Array[]; node: ScriptProcessorNode; startedAt: number } | null>(null);
  const timer = useRef<number | null>(null);

  const start = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false, channelCount: 1 } });
      const ctx = new AudioContext();
      const source = ctx.createMediaStreamSource(stream);
      const node = ctx.createScriptProcessor(4096, 1, 1);
      const chunks: Float32Array[] = [];
      node.onaudioprocess = (e) => {
        const d = new Float32Array(e.inputBuffer.getChannelData(0));
        chunks.push(d);
        let m = 0;
        for (let i = 0; i < d.length; i += 16) m = Math.max(m, Math.abs(d[i]));
        setLevel(m);
      };
      source.connect(node);
      node.connect(ctx.destination);
      ref.current = { ctx, stream, chunks, node, startedAt: Date.now() };
      setRecording(true);
      setSeconds(0);
      timer.current = window.setInterval(() => {
        const elapsed = (Date.now() - (ref.current?.startedAt ?? Date.now())) / 1000;
        setSeconds(elapsed);
        if (elapsed >= 40) stop();
      }, 200);
    } catch (e) {
      notify(`Mikrofon açılamadı: ${errorMessage(e)}. Sistem ayarlarından mikrofon izni verin.`, "error");
    }
  };

  const stop = async () => {
    const r = ref.current;
    if (!r) return;
    ref.current = null;
    if (timer.current) clearInterval(timer.current);
    r.node.disconnect();
    r.stream.getTracks().forEach((t) => t.stop());
    const sampleRate = r.ctx.sampleRate;
    await r.ctx.close();
    setRecording(false);
    setLevel(0);
    const wav = encodeWav(r.chunks, sampleRate);
    try {
      onDone(await api.saveRecording(Array.from(wav)));
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  useEffect(() => () => void (ref.current && stop()), []); // eslint-disable-line react-hooks/exhaustive-deps

  return recording ? (
    <div className="flex items-center gap-3 rounded-xl border border-red/40 bg-red-soft px-4 py-3">
      <span className="rec-pulse size-2.5 rounded-full bg-red" />
      <span className="font-mono text-[13px] tabular-nums">{seconds.toFixed(1)} sn</span>
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-surface-3">
        <div className="h-full bg-red transition-[width] duration-75" style={{ width: `${Math.min(100, level * 140)}%` }} />
      </div>
      <Button size="sm" variant="danger" icon={<CircleStop size={14} />} onClick={stop}>
        Durdur
      </Button>
    </div>
  ) : (
    <Button icon={<Mic size={14} />} onClick={start}>
      Mikrofonla kaydet
    </Button>
  );
}

function Step({ n, title, done, extra, last, children }: { n: number; title: string; done: boolean; extra?: React.ReactNode; last?: boolean; children: React.ReactNode }) {
  return (
    <section className="relative pb-6 pl-10">
      {!last && <div className="absolute top-8 bottom-1 left-3.5 w-px bg-border" />}
      <div
        className={cx(
          "absolute top-0 left-0 flex size-7 items-center justify-center rounded-full text-[13px] font-medium transition",
          done ? "bg-green-soft text-green" : "bg-surface-3 text-text-2",
        )}
        aria-hidden
      >
        {done ? <Check size={14} strokeWidth={2.5} /> : n}
      </div>
      <div className="mb-2 flex min-h-7 items-center justify-between gap-3">
        <h3 className="text-[13px] font-medium">{title}</h3>
        {extra}
      </div>
      {children}
    </section>
  );
}

const FIELDS = ["recording", "recording_text", "text", "consent", "library_name"];

export function Clone() {
  const { cloneResult, set, notify, status, navigate, revision } = useStore();
  const [recording, setRecording] = useState<string | null>(null);
  const [recordingSeconds, setRecordingSeconds] = useState<number | null>(null);
  const [recordingText, setRecordingText] = useState("");
  const [text, setText] = useState("");
  const [seed, setSeed] = useState("42");
  const [consent, setConsent] = useState(false);
  const [keep, setKeep] = useState(false);
  const [name, setName] = useState("");
  const [issues, setIssues] = useState<Issue[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [tab, setTab] = useState<"result" | "history">("result");
  const [history, setHistory] = useState<Job[]>([]);
  const job = useJob(cloneResult);

  const [transcribeId, setTranscribeId] = useState<string | null>(null);
  const transcribeJob = useJob(transcribeId);
  useEffect(() => {
    if (!transcribeJob) return;
    if (transcribeJob.status === "done") {
      const t = String(transcribeJob.result?.text ?? "")
        .replace(/\n+/g, " ")
        .trim();
      if (t) setRecordingText(t);
      setTranscribeId(null);
      notify("Kayıt metni çıkarıldı; kayıtla birebir aynı olduğunu kontrol edin", "success");
    } else if (["failed", "cancelled", "interrupted"].includes(transcribeJob.status)) {
      setTranscribeId(null);
    }
  }, [transcribeJob, notify]);

  useEffect(() => {
    api
      .listJobs({ kind: "clone", limit: 30 })
      .then(setHistory)
      .catch(() => {});
  }, [revision, cloneResult]);

  useEffect(() => {
    if (cloneResult) setTab("result");
  }, [cloneResult]);

  useEffect(() => {
    if (recording) api.allowFile(recording);
  }, [recording]);

  useEffect(() => {
    if (!recording) return setRecordingSeconds(null);
    api
      .audioDuration(recording)
      .then(setRecordingSeconds)
      .catch((e) => {
        setRecordingSeconds(null);
        notify(errorMessage(e), "error");
      });
  }, [recording, notify]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type === "over" || e.payload.type === "enter") setDragging(true);
        else if (e.payload.type === "leave") setDragging(false);
        else if (e.payload.type === "drop") {
          setDragging(false);
          const path = e.payload.paths.find((p) => AUDIO_EXTENSIONS.includes(p.split(".").pop()?.toLowerCase() ?? ""));
          if (path) setRecording(path);
          else notify("Ses dosyası bırakın (wav, mp3, flac, m4a…)", "error");
        }
      })
      .then((f) => (unlisten = f))
      .catch(() => {});
    return () => unlisten?.();
  }, [notify]);

  const clearIssue = (field: string) => setIssues((list) => list.filter((s) => s.data?.field !== field));

  const extractText = async () => {
    if (!recording) return setIssues([{ code: "missing", message: "Önce bir kayıt seçin ya da kaydedin", data: { field: "recording" } }]);
    if (!status?.whisper.model_ready) {
      return notify("Kayıttan metin çıkarmak için önce transkript modelini indirin", "info", { label: "Transkript", run: () => navigate("transcribe") });
    }
    try {
      const j = await api.enqueueJob({ kind: "transcribe", file: recording, language: null });
      setTranscribeId(j.id);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  const clone = async () => {
    const missing: Issue[] = [];
    if (!recording) missing.push({ code: "missing", message: "Bir kayıt seçin ya da mikrofonla kaydedin", data: { field: "recording" } });
    if (!recordingText.trim()) missing.push({ code: "missing", message: "Kayıtta söyleneni kelimesi kelimesine yazın", data: { field: "recording_text" } });
    if (!text.trim()) missing.push({ code: "missing", message: "Bu sesle okunacak metni yazın", data: { field: "text" } });
    if (keep && !name.trim()) missing.push({ code: "missing", message: "Kitaplıkta görünecek adı yazın", data: { field: "library_name" } });
    if (!consent) missing.push({ code: "consent", message: "Kaydı kullanma izni onaylanmadı", data: { field: "consent" } });
    if (missing.length) return setIssues(missing);
    setIssues([]);
    setSubmitting(true);
    try {
      const j = await api.enqueueJob({
        kind: "clone",
        recording: recording!,
        recording_text: recordingText,
        text,
        seed: Number(seed) || 42,
        consent,
        library_name: keep ? name.trim() : null,
      });
      set({ cloneResult: j.id });
    } catch (e) {
      const found = errorIssues(e);
      if (found.length) setIssues(found);
      else notify(errorMessage(e), "error");
    } finally {
      setSubmitting(false);
    }
  };

  const fieldError = (field: string) =>
    issues
      .filter((s) => s.data?.field === field)
      .map((s) => s.message)
      .join(" ") || undefined;
  const otherIssues = issues.filter((s) => !FIELDS.includes(String(s.data?.field ?? "")));

  const durationNote =
    recordingSeconds === null ? null : recordingSeconds < 3 ? (
      <span className="text-red">Çok kısa: en az 3 sn gerekir</span>
    ) : recordingSeconds > 36 ? (
      <span className="text-amber">İlk 36 sn kullanılır; kayıt metni yalnız o kısmı içermeli</span>
    ) : recordingSeconds < 5 || recordingSeconds > 30 ? (
      <span className="text-amber">En iyi sonuç 5-30 sn temiz kayıtla</span>
    ) : (
      <span className="text-green">Süre iyi</span>
    );

  const showResult = tab === "result" && job;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="Ses klonla" description="Kendi sesinizden ya da sahibinden izin aldığınız bir kayıttan, birkaç saniyelik örnekle okuma yapın" />
      <div className="flex min-h-0 flex-1">
        <div className="flex w-[540px] shrink-0 flex-col border-r border-border bg-surface">
          <div className="min-h-0 flex-1 overflow-y-auto px-6 pt-6 pb-2">
            <Step n={1} title="Kayıt" done={!!recording} extra={<span className="text-[12px] text-muted">wav, mp3, flac, m4a · 5-30 sn</span>}>
              <div
                className={cx(
                  "rounded-xl border border-dashed p-4 transition-colors",
                  dragging ? "border-text-2 bg-surface-2" : fieldError("recording") ? "border-red/50" : "border-border-strong",
                )}
              >
                {recording ? (
                  <div className="space-y-3">
                    <div className="flex items-center gap-2">
                      <FileAudio size={16} className="text-muted" />
                      <span className="min-w-0 flex-1 truncate font-medium">{recording.split(/[\\/]/).pop()}</span>
                      <span className="text-[12px] text-muted">{formatDuration(recordingSeconds)}</span>
                      <IconButton label="Kaydı kaldır" onClick={() => setRecording(null)}>
                        <X size={15} />
                      </IconButton>
                    </div>
                    <AudioPlayer path={recording} compact label="Kayıt" />
                    {durationNote && <div className="text-[12px]">{durationNote}</div>}
                  </div>
                ) : (
                  <div className="flex flex-col items-center gap-3 py-2 text-center">
                    <Upload size={20} className="text-muted" />
                    <div className="text-[13px] text-muted">Ses dosyasını buraya bırakın ya da</div>
                    <div className="flex gap-2">
                      <Button
                        icon={<FileAudio size={14} />}
                        onClick={async () => {
                          const f = await open({ filters: [{ name: "Ses", extensions: AUDIO_EXTENSIONS }] });
                          if (typeof f === "string") {
                            setRecording(f);
                            clearIssue("recording");
                          }
                        }}
                      >
                        Dosya seç
                      </Button>
                      <Recorder
                        onDone={(p) => {
                          setRecording(p);
                          clearIssue("recording");
                        }}
                      />
                    </div>
                  </div>
                )}
              </div>
              {fieldError("recording") ? (
                <div className="mt-1.5 text-[12px] text-red">{fieldError("recording")}</div>
              ) : (
                <div className="mt-1.5 flex items-center gap-1.5 text-[12px] text-muted">
                  <Lock size={11} /> Kayıt yalnız bu bilgisayarda işlenir, ağa gönderilmez.
                </div>
              )}
            </Step>
            <Step
              n={2}
              title="Kayıtta söylenen metin"
              done={!!recordingText.trim()}
              extra={
                status?.whisper.binary_present && (
                  <Button size="sm" variant="ghost" icon={<ScrollText size={13} />} loading={!!transcribeId} onClick={extractText}>
                    {transcribeId ? "Çıkarılıyor…" : "Kayıttan çıkar"}
                  </Button>
                )
              }
            >
              <Field error={fieldError("recording_text")} hint="Kelimesi kelimesine aynı olmalı; model sesi bu eşleşmeden öğrenir">
                <TextArea
                  rows={3}
                  value={recordingText}
                  invalid={!!fieldError("recording_text")}
                  onChange={(e) => {
                    setRecordingText(e.target.value);
                    clearIssue("recording_text");
                  }}
                  placeholder="Kayıtta ne söyleniyorsa aynısı"
                  aria-label="Kayıtta söylenen metin"
                />
              </Field>
            </Step>
            <Step n={3} title="Okunacak metin" done={!!text.trim()} last>
              <Field error={fieldError("text")}>
                <TextArea
                  rows={4}
                  value={text}
                  invalid={!!fieldError("text")}
                  onChange={(e) => {
                    setText(e.target.value);
                    clearIssue("text");
                  }}
                  placeholder="Satır başına bir cümle; rakamları yazıyla yazın"
                  aria-label="Okunacak metin"
                />
              </Field>
              <div className="mt-4 rounded-lg border border-border p-3.5">
                <Checkbox checked={keep} onChange={setKeep}>
                  <span className="font-medium">Kitaplığa kaydet</span>
                  <span className="mt-0.5 block text-[12px] text-muted">Bu sesle sonra Stüdyo'dan okuma yapın; izin bir daha sorulmaz</span>
                </Checkbox>
                {keep && (
                  <Field className="mt-3 pl-6.5" error={fieldError("library_name")}>
                    <Input
                      autoFocus
                      value={name}
                      invalid={!!fieldError("library_name")}
                      onChange={(e) => {
                        setName(e.target.value);
                        clearIssue("library_name");
                      }}
                      placeholder="Sesin adı (ör. Benim sesim)"
                      aria-label="Sesin adı"
                    />
                  </Field>
                )}
              </div>
              <Disclosure title="Gelişmiş" className="mt-4">
                <Field label="Tohum" hint="Aynı kayıt, metin ve tohum aynı okumayı verir. Okumayı beğenmezseniz başka bir sayı deneyin.">
                  <Input value={seed} onChange={(e) => setSeed(e.target.value.replace(/\D/g, ""))} className="w-32 font-mono" aria-label="Tohum" />
                </Field>
              </Disclosure>
            </Step>
            <IssueList issues={otherIssues} className="mb-4" />
          </div>
          <div className="space-y-3 border-t border-border bg-surface px-6 py-4">
            <div className={cx("rounded-lg px-3 py-2.5 transition", fieldError("consent") ? "bg-red-soft" : "bg-surface-2")}>
              <Checkbox
                checked={consent}
                invalid={!!fieldError("consent")}
                onChange={(v) => {
                  setConsent(v);
                  clearIssue("consent");
                }}
              >
                <span className="text-[13px]">{CONSENT_TEXT}</span>
              </Checkbox>
              {fieldError("consent") && <div className="mt-1.5 pl-6.5 text-[12px] font-medium text-red">{fieldError("consent")}</div>}
            </div>
            <div className="flex justify-end">
              <Button variant="primary" size="lg" icon={<Mic size={16} />} loading={submitting} onClick={clone}>
                Klonla ve oku
              </Button>
            </div>
          </div>
        </div>
        <div className="flex min-w-0 flex-1 flex-col bg-surface">
          <div className="flex min-h-12 items-center gap-3 border-b border-border px-5 py-2.5">
            <Segmented
              size="sm"
              value={tab}
              onChange={setTab}
              aria-label="Sonuç paneli"
              options={[
                { value: "result", label: "Sonuç" },
                { value: "history", label: `Son klonlar${history.length ? ` (${history.length})` : ""}` },
              ]}
            />
            <div className="flex-1" />
            {job && (job.status === "queued" || job.status === "running") && <Badge color="accent">{job.status === "running" ? "seslendiriliyor" : "sırada"}</Badge>}
          </div>
          <div className="min-h-0 flex-1">
            {showResult ? (
              job.status === "done" ? (
                <ReadingResult id={job.id} />
              ) : (
                <div className="p-6">
                  <JobProgress job={job} />
                </div>
              )
            ) : (
              <div className="h-full overflow-y-auto p-3">
                <HistoryList
                  jobs={history}
                  selected={cloneResult}
                  onSelect={(id) => {
                    set({ cloneResult: id });
                    setTab("result");
                  }}
                  empty={
                    <EmptyState
                      icon={<Mic size={22} />}
                      title="Henüz klon okuma yok"
                      description="Soldaki üç adımı doldurun: kaydı seçin, kayıtta söyleneni yazın, okunacak metni girin. Sonuç burada çalınır ve dışa aktarılır."
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

// Result of a reading: player, lines (with the playing line and word), per-line regeneration, export.
import { useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { AlertTriangle, Check, Download, FolderOpen, Pencil, Play, RefreshCw, ShieldCheck, Star, X } from "lucide-react";
import { api, errorMessage, joinPath, type Job, type LineRecord, type Word } from "../api";
import { useStore } from "../store";
import { formatDate, formatDuration } from "../languages";
import { AudioPlayer, type AudioPlayerHandle } from "./AudioPlayer";
import { Badge, Button, IconButton, MenuDivider, MenuItem, Popover, cx, inputClass } from "./ui";

const EXPORT_FORMATS: { format: string; label: string; ext: string }[] = [
  { format: "wav", label: "WAV ses (48 kHz, 16 bit)", ext: "wav" },
  { format: "mp3", label: "MP3 ses (192 kbps)", ext: "mp3" },
  { format: "srt", label: "Altyazı SRT (cümle)", ext: "srt" },
  { format: "vtt", label: "Altyazı VTT (cümle)", ext: "vtt" },
  { format: "word_srt", label: "Altyazı SRT (kelime)", ext: "srt" },
  { format: "word_vtt", label: "Altyazı VTT (kelime)", ext: "vtt" },
  { format: "json", label: "Kelime zamanları (JSON)", ext: "json" },
];

export function ExportMenu({ job, hasWords, hasText }: { job: Job; hasWords: boolean; hasText?: boolean }) {
  const notify = useStore((s) => s.notify);
  const settings = useStore((s) => s.settings);
  const exportAs = async (format: string, ext: string) => {
    try {
      const name = await api.suggestedFileName(job.id);
      const dir = settings?.export_dir;
      const target = await save({
        defaultPath: dir ? joinPath(dir, `${name}.${ext}`) : `${name}.${ext}`,
        filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
      });
      if (!target) return;
      const path = await api.exportJob(job.id, format, target);
      notify(`Kaydedildi: ${path.split(/[\\/]/).pop()}`, "success", { label: "Göster", run: () => revealItemInDir(path) });
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  return (
    <Popover
      align="right"
      width={250}
      trigger={(toggle) => (
        <Button size="sm" icon={<Download size={14} />} onClick={toggle}>
          Dışa aktar
        </Button>
      )}
    >
      {(close) => (
        <div className="py-1">
          {[...EXPORT_FORMATS, ...(hasText ? [{ format: "txt", label: "Düz metin (TXT)", ext: "txt" }] : [])]
            .filter((f) => hasWords || (!f.format.startsWith("word_") && f.format !== "json"))
            .map((f) => (
              <MenuItem
                key={f.format}
                onClick={() => {
                  close();
                  exportAs(f.format, f.ext);
                }}
              >
                {f.label}
              </MenuItem>
            ))}
          {job.folder && (
            <>
              <MenuDivider />
              <MenuItem
                icon={<FolderOpen size={14} />}
                onClick={() => {
                  close();
                  revealItemInDir(job.result?.wav ?? job.folder!);
                }}
              >
                Klasörde göster
              </MenuItem>
            </>
          )}
        </div>
      )}
    </Popover>
  );
}

function LineText({ line, words, time }: { line: LineRecord; words: Word[]; time: number | null }) {
  if (!words.length || time === null) return <span>{line.spoken !== line.text ? line.text : line.spoken}</span>;
  return (
    <span>
      {words.map((w, i) => (
        <span key={i}>
          <span className={cx("transition-colors", time >= w.t0 && time < w.t1 + 0.02 ? "playing-word" : time > w.t1 ? "text-text" : "text-text-2")}>{w.word}</span>{" "}
        </span>
      ))}
    </span>
  );
}

function LineRow({
  job,
  line,
  words,
  playing,
  time,
  onPlay,
  pending,
}: {
  job: Job;
  line: LineRecord;
  words: Word[];
  playing: boolean;
  time: number | null;
  onPlay: () => void;
  pending: boolean;
}) {
  const notify = useStore((s) => s.notify);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(line.text);
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (playing) box.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [playing]);
  useEffect(() => setText(line.text), [line.text]);
  const regenerate = async (newText?: string) => {
    try {
      await api.enqueueJob({ kind: "line", job_id: job.id, line: line.line, text: newText ?? null, seed: null });
      setEditing(false);
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  const timed = job.result?.kind === "timed";
  return (
    <div
      ref={box}
      className={cx("group relative flex items-start gap-3 rounded-lg px-3 py-2 transition-colors", playing ? "bg-accent-soft/70" : "hover:bg-surface-2", pending && "opacity-60")}
    >
      <button onClick={onPlay} className="mt-0.5 w-6 shrink-0 text-right font-mono text-[12px] text-muted hover:text-accent" title="Buradan çal">
        {line.line}
      </button>
      <div className="min-w-0 flex-1">
        {editing ? (
          <div className="flex items-center gap-1.5">
            <input
              autoFocus
              value={text}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") regenerate(text);
                if (e.key === "Escape") setEditing(false);
              }}
              className={cx(inputClass, "h-8 w-full py-1")}
            />
            <IconButton label="Bu metinle yeniden üret" onClick={() => regenerate(text)}>
              <Check size={15} />
            </IconButton>
            <IconButton label="Vazgeç" onClick={() => setEditing(false)}>
              <X size={15} />
            </IconButton>
          </div>
        ) : (
          <div className="selectable text-[14px] leading-relaxed">
            <LineText line={line} words={words} time={playing ? time : null} />
            {line.spoken !== line.text && (
              <div className="mt-0.5 text-[12px] text-muted" title="Okunuş sözlüğü uygulandı">
                okunuş: {line.spoken}
              </div>
            )}
            {line.wer !== undefined && line.wer > 0.2 && <div className="mt-0.5 text-[12px] text-red">duyulan: {line.heard || "—"}</div>}
          </div>
        )}
        <div className="mt-1 flex flex-wrap items-center gap-1.5">
          {line.speed_warning && !timed && (
            <Badge color="amber">
              <AlertTriangle size={11} /> hızı farklı, dinleyin
            </Badge>
          )}
          {(line.speed ?? 1) > 1.01 && <Badge>×{line.speed!.toLocaleString("tr-TR")} hızlandırıldı</Badge>}
          {(line.drift ?? 0) > 0.3 && <Badge color="amber">{line.drift!.toLocaleString("tr-TR")} sn geç başladı</Badge>}
          {line.wer !== undefined && (
            <span title={line.heard ? `Duyulan: ${line.heard}` : "Hiçbir şey duyulmadı"}>
              <Badge color={line.wer > 0.2 ? "red" : line.wer > 0.05 ? "amber" : "green"}>
                {line.wer > 0.2 ? "okunuş farkı" : "doğru"} · %{Math.round(line.wer * 100)}
              </Badge>
            </span>
          )}
          {line.version > 1 && <Badge>yenilendi ×{line.version - 1}</Badge>}
          {pending && <Badge color="accent">yeniden seslendiriliyor…</Badge>}
        </div>
      </div>
      {!editing && !timed && (
        <div className="absolute top-1 right-1.5 flex items-center gap-0.5 rounded-lg border border-border bg-surface p-0.5 opacity-0 shadow-card transition group-hover:opacity-100">
          <IconButton label="Buradan çal" onClick={onPlay}>
            <Play size={13} />
          </IconButton>
          <IconButton label="Yeni tohumla yeniden üret" onClick={() => regenerate()} disabled={pending}>
            <RefreshCw size={13} />
          </IconButton>
          <IconButton label="Düzelt ve yeniden üret" onClick={() => setEditing(true)} disabled={pending}>
            <Pencil size={13} />
          </IconButton>
        </div>
      )}
    </div>
  );
}

export function ReadingResult({ id, compact, hideTitle }: { id: string; compact?: boolean; hideTitle?: boolean }) {
  const revision = useStore((s) => s.revision);
  const [job, setJob] = useState<Job | null>(null);
  const [time, setTime] = useState<number | null>(null);
  const player = useRef<AudioPlayerHandle>(null);

  useEffect(() => {
    api
      .getJob(id)
      .then(setJob)
      .catch(() => setJob(null));
  }, [id, revision]);

  const lines: LineRecord[] = useMemo(() => (job?.result?.lines as LineRecord[]) ?? [], [job]);
  const words: Word[] = useMemo(() => (job?.result?.words as Word[]) ?? [], [job]);
  const playingLine = time === null ? null : (lines.find((l, i) => time >= l.t0 && time < (lines[i + 1]?.t0 ?? Infinity))?.line ?? null);
  const pendingLines = usePendingLines(id);

  if (!job) return <div className="p-6 text-muted">Yükleniyor…</div>;
  if (!job.result) return <div className="p-6 text-muted">Bu üretimin sonucu yok</div>;
  const result = job.result;
  const wav = result.wav as string;
  const fileVersion = `${job.id}-${lines.map((l) => l.version).join(".")}`;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className={cx("border-b border-border", compact ? "px-5 py-4" : "px-6 py-5")}>
        <div className={cx("mb-3 flex items-start justify-between gap-3", hideTitle && "mb-2 justify-end")}>
          <div className={cx("min-w-0", hideTitle && "hidden")}>
            <div className="truncate text-[15px] font-semibold">{job.title || "Okuma"}</div>
            <div className="mt-0.5 text-[12px] text-muted">
              {job.voice_name} · {formatDate(job.finished_at ?? job.created_at)} · {formatDuration(job.audio_s)} ses
              {job.duration_s ? ` · ${formatDuration(job.duration_s)} üretim` : ""}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-1">
            <IconButton
              label={job.starred ? "Yıldızı kaldır" : "Yıldızla"}
              onClick={async () => {
                await api.updateJob(job.id, { starred: !job.starred });
                setJob({ ...job, starred: !job.starred });
              }}
              className={job.starred ? "text-amber" : ""}
            >
              <Star size={15} fill={job.starred ? "currentColor" : "none"} />
            </IconButton>
            <ReviewButton job={job} />
            <ExportMenu job={job} hasWords={words.length > 0} />
          </div>
        </div>
        <AudioPlayer ref={player} path={wav} version={fileVersion} onTime={setTime} />
        {(() => {
          const t = result.tone_match as { center_hz: number[]; gain_db: number[] } | null | undefined;
          if (!t) return null;
          const i = t.gain_db.indexOf(Math.min(...t.gain_db));
          return t.gain_db[i] < -0.5 ? (
            <div className="mt-2 text-[12px] text-muted" title={t.center_hz.map((hz, k) => `${(hz / 1000).toFixed(1)} kHz: ${t.gain_db[k]} dB`).join("\n")}>
              Ton düzeltildi: {(t.center_hz[i] / 1000).toLocaleString("tr-TR")} kHz civarı {t.gain_db[i].toLocaleString("tr-TR")} dB
            </div>
          ) : null;
        })()}
        {result.review && (
          <div className="mt-2 text-[12px] text-text-2">
            Kalite kontrolü: ortalama kelime hata oranı %{Math.round(Number(result.review.mean_wer) * 100)}
            {(result.review.flagged as number[]).length
              ? ` · ${(result.review.flagged as number[]).length} satırda okunuş farkı (aşağıda kırmızı); yeniden üretebilirsiniz`
              : " · bütün satırlar doğru okunmuş"}
          </div>
        )}
        {result.word_timing_error && <div className="mt-2 text-[12px] text-amber">Kelime zamanı çıkarılamadı: {String(result.word_timing_error)}</div>}
        {(result.notes as string[] | undefined)?.map((n, i) => (
          <div key={i} className="mt-2 text-[12px] text-amber">
            {n}
          </div>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {lines.map((l) => (
          <LineRow
            key={l.line + "-" + l.version}
            job={job}
            line={l}
            words={words.filter((w) => w.line === l.line)}
            playing={playingLine === l.line}
            time={time}
            pending={pendingLines.has(l.line)}
            onPlay={() => player.current?.play(l.t0)}
          />
        ))}
      </div>
    </div>
  );
}

/** Transcribes the reading back and compares it line by line with the script (needs whisper). */
function ReviewButton({ job }: { job: Job }) {
  const { status, queue, notify } = useStore();
  const pending = queue.some((j) => j.kind === "review" && j.parent_id === job.id);
  if (!status?.whisper.binary_present) return null;
  return (
    <IconButton
      label={status.whisper.model_ready ? "Kalite kontrolü (geri yazıya dök ve karşılaştır)" : "Kalite kontrolü için Transkript sayfasından modeli indirin"}
      disabled={!status.whisper.model_ready || pending}
      className={pending ? "text-accent" : ""}
      onClick={async () => {
        try {
          await api.enqueueJob({ kind: "review", job_id: job.id });
        } catch (e) {
          notify(errorMessage(e), "error");
        }
      }}
    >
      <ShieldCheck size={15} />
    </IconButton>
  );
}

/** Line regenerations of this reading that are queued or running. */
function usePendingLines(parentId: string): Set<number> {
  const queue = useStore((s) => s.queue);
  return useMemo(
    () => new Set(queue.filter((j) => j.kind === "line" && j.parent_id === parentId).map((j) => Number((j.request as { line?: number }).line))),
    [queue, parentId],
  );
}

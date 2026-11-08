// Studio: pick a voice, write the script, narrate; the result (or recent readings with this voice) is on the right.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, AudioWaveform, BookOpen, ChevronDown, Eye, FileText, Mic2, Pause, PencilRuler, Save, SpellCheck, Trash2, X } from "lucide-react";
import { api, errorIssues, errorMessage, type Draft, type Issue, type Job, type TextCheck } from "../api";
import { useStore } from "../store";
import { LANGUAGES, formatDate, formatDuration, languageName, voiceName } from "../languages";
import { PAUSES, ScriptEditor, type ScriptEditorHandle } from "../components/ScriptEditor";
import { JobProgress, useDebounced, useJob } from "../components/JobProgress";
import { ReadingResult } from "../components/ReadingResult";
import { VoicePicker } from "../components/VoicePicker";
import { FixTextDialog } from "../components/FixTextDialog";
import { HistoryList } from "../components/HistoryList";
import { Select } from "../components/Select";
import { Badge, Button, EmptyState, Flag, Kbd, MenuDivider, MenuItem, MenuLabel, Modal, Popover, PromptDialog, Segmented, cx, modKey } from "../components/ui";

const SAMPLE_TEXT = `Merhaba, Tınıya hoş geldiniz.
Her satır bir cümledir ve sırayla okunur.
Satır sonuna durak ekleyebilirsiniz. || paragraf

Boş satır da paragraf durağı demektir.
Rakamları yazıyla yazın: iki bin yirmi altı.`;

/** Human descriptions of the pause markers for the insert menu. */
const PAUSE_INFO: Record<string, { title: string; description: string }> = {
  bagli: { title: "Kısa durak", description: "Cümleler birbirine bağlanır" },
  normal: { title: "Normal durak", description: "Cümle arası nefes" },
  paragraf: { title: "Paragraf durağı", description: "Konu ya da paragraf değişirken" },
  agir: { title: "Uzun durak", description: "Vurgulu, ağır geçişler" },
};

function DraftsMenu({ text, onLoad }: { text: string; onLoad: (text: string) => void }) {
  const notify = useStore((s) => s.notify);
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [saveOpen, setSaveOpen] = useState(false);
  const refresh = () => api.listDrafts().then(setDrafts);
  return (
    <>
      <Popover
        width={300}
        trigger={(toggle, open) => (
          <Button
            size="sm"
            variant="ghost"
            icon={<FileText size={14} />}
            aria-expanded={open}
            onClick={() => {
              refresh();
              toggle();
            }}
          >
            Taslaklar
            <ChevronDown size={13} className="-ml-0.5 text-muted" />
          </Button>
        )}
      >
        {(close) => (
          <div className="py-1">
            <MenuItem
              icon={<Save size={14} />}
              onClick={() => {
                close();
                setSaveOpen(true);
              }}
            >
              Taslak olarak kaydet…
            </MenuItem>
            {drafts.length > 0 && (
              <>
                <MenuDivider />
                <MenuLabel>Kayıtlı taslaklar</MenuLabel>
              </>
            )}
            <div className="max-h-72 overflow-y-auto">
              {drafts.map((d) => (
                <div key={d.id} className="group flex items-center hover:bg-surface-2">
                  <button
                    type="button"
                    className="min-w-0 flex-1 px-3 py-2 text-left"
                    onClick={() => {
                      close();
                      onLoad(d.text);
                      notify(`“${d.name}” yüklendi (${modKey}+Z ile geri alabilirsiniz)`);
                    }}
                  >
                    <div className="truncate font-medium">{d.name}</div>
                    <div className="truncate text-[12px] text-muted">
                      {formatDate(d.updated_at)} · {d.text.split("\n").filter((l) => l.trim()).length} satır
                    </div>
                  </button>
                  <button
                    type="button"
                    title="Üzerine kaydet"
                    aria-label={`${d.name} taslağının üzerine kaydet`}
                    className="px-1.5 text-muted opacity-0 group-hover:opacity-100 hover:text-text focus-visible:opacity-100"
                    onClick={async () => {
                      await api.saveDraft(d.name, text, d.id);
                      refresh();
                      notify(`“${d.name}” güncellendi`, "success");
                    }}
                  >
                    <Save size={13} />
                  </button>
                  <button
                    type="button"
                    title="Sil"
                    aria-label={`${d.name} taslağını sil`}
                    className="px-2 text-muted opacity-0 group-hover:opacity-100 hover:text-red focus-visible:opacity-100"
                    onClick={async () => {
                      await api.deleteDraft(d.id);
                      refresh();
                    }}
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              ))}
              {!drafts.length && <div className="px-3 py-3 text-[13px] text-muted">Henüz taslak yok</div>}
            </div>
          </div>
        )}
      </Popover>
      <PromptDialog
        open={saveOpen}
        onClose={() => setSaveOpen(false)}
        title="Taslak olarak kaydet"
        label="Taslak adı"
        confirmLabel="Kaydet"
        onSubmit={async (name) => {
          await api.saveDraft(name, text);
          notify("Taslak kaydedildi", "success");
        }}
      />
    </>
  );
}

function PauseMenu({ onInsert }: { onInsert: (name: string | null) => void }) {
  return (
    <Popover
      width={300}
      trigger={(toggle, open) => (
        <Button size="sm" variant="ghost" icon={<Pause size={14} />} onClick={toggle} aria-expanded={open} title="İmlecin olduğu satırın sonuna durak ekle">
          Durak ekle
          <ChevronDown size={13} className="-ml-0.5 text-muted" />
        </Button>
      )}
    >
      {(close) => (
        <div className="py-1">
          <div className="px-3 pt-1.5 pb-2 text-[12px] leading-snug text-muted">
            İmlecin olduğu satırın sonuna eklenir. Metinde <code className="rounded bg-surface-3 px-1 font-mono whitespace-nowrap text-text-2">|| paragraf</code> gibi görünür.
          </div>
          <MenuDivider />
          {PAUSES.map((p) => (
            <MenuItem
              key={p.name}
              icon={<span className="font-mono text-[11px] text-muted">||</span>}
              description={PAUSE_INFO[p.name]?.description}
              hint={`${p.seconds.toLocaleString("tr-TR")} sn`}
              onClick={() => {
                close();
                onInsert(p.name);
              }}
            >
              {PAUSE_INFO[p.name]?.title ?? p.label}
            </MenuItem>
          ))}
          <MenuDivider />
          <MenuItem
            icon={<X size={13} />}
            description="Satır varsayılan 0,35 sn durakla biter"
            onClick={() => {
              close();
              onInsert(null);
            }}
          >
            Satırın durağını kaldır
          </MenuItem>
        </div>
      )}
    </Popover>
  );
}

/** Problems found in the script, shown right under the editor; click jumps to the line. */
function IssueStrip({
  check,
  issues,
  onGoto,
  onFix,
}: {
  check: TextCheck | null;
  issues: Issue[];
  onGoto: (line: number, column: number) => void;
  onFix: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const findings = check?.findings ?? [];
  if (!findings.length && !issues.length) return null;
  const shown = expanded ? findings : findings.slice(0, 2);
  const blocking = issues.length > 0;
  return (
    <div className={cx("border-t px-6 py-2.5", blocking ? "border-red/25 bg-red-soft/60" : "border-amber/25 bg-amber-soft/50")} role="status">
      <div className="flex items-start gap-3">
        <AlertTriangle size={15} className={cx("mt-0.5 shrink-0", blocking ? "text-red" : "text-amber")} />
        <div className="min-w-0 flex-1 space-y-1 text-[13px]">
          {issues.map((s, i) => (
            <div key={`i${i}`} className="font-medium text-red">
              {s.message}
            </div>
          ))}
          {findings.length > 0 && (
            <>
              <div className="font-medium text-text">
                {findings.length} yerde düzeltme önerisi var
                {findings.length > 2 && (
                  <button type="button" onClick={() => setExpanded(!expanded)} className="ml-2 font-normal text-text-2 underline-offset-2 hover:underline">
                    {expanded ? "daha az göster" : "tümünü göster"}
                  </button>
                )}
              </div>
              <div className={cx("space-y-0.5", expanded && "max-h-36 overflow-y-auto")}>
                {shown.map((f, i) => (
                  <button
                    key={i}
                    type="button"
                    onClick={() => onGoto(f.line, f.column)}
                    className="flex w-full items-baseline gap-2 rounded px-1 -mx-1 text-left text-text-2 hover:bg-surface/60 hover:text-text"
                  >
                    <span className="shrink-0 font-mono text-[11px] text-muted">Satır {f.line}</span>
                    <span className="min-w-0 truncate">{f.message}</span>
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
        {findings.length > 0 && (
          <Button size="sm" variant="soft" icon={<SpellCheck size={13} />} onClick={onFix}>
            Otomatik düzelt
          </Button>
        )}
      </div>
    </div>
  );
}

function ResultPanel({ id, voiceId, tab, setTab }: { id: string | null; voiceId: string | null; tab: "result" | "history"; setTab: (t: "result" | "history") => void }) {
  const job = useJob(id);
  const { set, revision, voices } = useStore();
  const [history, setHistory] = useState<Job[]>([]);
  const voice = voices.find((v) => v.id === voiceId) ?? null;
  useEffect(() => {
    if (!voiceId) return setHistory([]);
    api
      .listJobs({ voice: voiceId, kind: "read", limit: 30 })
      .then(setHistory)
      .catch(() => setHistory([]));
  }, [voiceId, revision, id]);

  const active = job && (job.status === "queued" || job.status === "running");
  const showResult = tab === "result" && job;
  return (
    <aside aria-label="Sonuç" className="flex w-[400px] shrink-0 flex-col border-l border-border bg-surface">
      <div className="drag-region flex items-center gap-3 border-b border-border px-5 pt-5 pb-4">
        <Segmented
          size="sm"
          value={showResult ? "result" : "history"}
          onChange={setTab}
          aria-label="Sonuç paneli"
          options={[
            { value: "result", label: "Sonuç" },
            { value: "history", label: `Son okumalar${history.length ? ` (${history.length})` : ""}` },
          ]}
        />
        <div className="flex-1" />
        {active && <Badge color="accent">{job.status === "running" ? "seslendiriliyor" : "sırada"}</Badge>}
      </div>
      <div className="min-h-0 flex-1">
        {showResult ? (
          job.status === "done" ? (
            <ReadingResult id={job.id} compact />
          ) : (
            <div className="p-4">
              <JobProgress
                job={job}
                onRetry={async () => {
                  const next = await api.rerunJob(job.id);
                  set({ studioResult: next.id });
                }}
              />
            </div>
          )
        ) : (
          <div className="h-full overflow-y-auto p-3">
            {voice && history.length > 0 && <div className="px-3 pt-1 pb-2 text-[12px] text-muted">{voiceName(voice)} ile yapılan okumalar</div>}
            <HistoryList
              jobs={history}
              selected={id}
              onSelect={(jid) => {
                set({ studioResult: jid });
                setTab("result");
              }}
              empty={
                <EmptyState
                  icon={<AudioWaveform size={22} />}
                  title={voice ? `${voiceName(voice)} ile henüz okuma yok` : "Henüz okuma yok"}
                  description={`Metni yazın ve ${modKey}+Enter ile seslendirin. Sonuç burada satır satır çalınır; beğenmediğiniz satırı tek başına yeniden üretebilirsiniz.`}
                />
              }
            />
          </div>
        )}
      </div>
      {voice && (voice.warning || voice.preview_error || voice.noisy) && (
        <div className="flex gap-2 border-t border-border px-5 py-3 text-[12px] text-amber">
          <AlertTriangle size={13} className="mt-px shrink-0" />
          <span>{voice.warning ?? voice.preview_error ?? "Bu sesin kaydında arka plan gürültüsü var; okumalar da taşır."}</span>
        </div>
      )}
    </aside>
  );
}

export function Studio() {
  const { studioVoice, studioResult, set, voices, notify, navigate } = useStore();
  const [text, setText] = useState(() => localStorage.getItem("tini.studioText") ?? SAMPLE_TEXT);
  const [title, setTitle] = useState("");
  const [language, setLanguage] = useState("auto");
  const [check, setCheck] = useState<TextCheck | null>(null);
  const [issues, setIssues] = useState<Issue[]>([]);
  const [pronunciation, setPronunciation] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [fixOpen, setFixOpen] = useState(false);
  const [tab, setTab] = useState<"result" | "history">(studioResult ? "result" : "history");
  const editor = useRef<ScriptEditorHandle>(null);
  const debouncedText = useDebounced(text, 350);
  const voice = voices.find((v) => v.id === studioVoice) ?? null;

  useEffect(() => {
    if (voices.length && !voices.some((v) => v.id === studioVoice)) set({ studioVoice: voices[0].id });
  }, [studioVoice, voices, set]);

  useEffect(() => {
    localStorage.setItem("tini.studioText", debouncedText);
    api
      .checkText(debouncedText, language === "auto" ? null : language, studioVoice)
      .then(setCheck)
      .catch(() => {});
  }, [debouncedText, language, studioVoice]);

  useEffect(() => {
    if (issues.length) setIssues([]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, studioVoice]);

  const narrate = useCallback(async () => {
    if (!studioVoice) {
      setIssues([{ code: "voice", message: voices.length ? "Önce bir ses seçin" : "Kitaplıkta ses yok; önce bir ses tasarlayın ya da klonlayın", data: {} }]);
      return;
    }
    if (!text.trim()) {
      setIssues([{ code: "text", message: "Okunacak metni yazın", data: {} }]);
      editor.current?.focusLine(1);
      return;
    }
    setSubmitting(true);
    setIssues([]);
    try {
      const job = await api.enqueueJob({ kind: "read", voice: studioVoice, text, language: language === "auto" ? null : language, title: title.trim() || null });
      set({ studioResult: job.id });
      setTab("result");
    } catch (e) {
      const found = errorIssues(e);
      if (found.length) setIssues(found);
      else notify(errorMessage(e), "error");
    } finally {
      setSubmitting(false);
    }
  }, [studioVoice, text, language, title, set, notify, voices.length]);

  const languageOptions = useMemo(
    () => [
      { value: "auto", label: check ? `Otomatik (${languageName(check.language)})` : "Otomatik" },
      ...LANGUAGES.map((l) => ({ value: l.code, label: l.name, icon: <Flag language={l.code} size={10} /> })),
    ],
    [check],
  );
  const languageMismatch = voice && check && check.language !== voice.language;
  const findings = check?.findings.length ?? 0;

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="drag-region flex items-center gap-2 border-b border-border bg-surface px-6 pt-5 pb-4">
          <VoicePicker value={studioVoice} onChange={(id) => set({ studioVoice: id })} />
          <Select value={language} onChange={setLanguage} options={languageOptions} className="w-[190px]" aria-label="Metnin dili" />
          <input
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="Başlık (isteğe bağlı)"
            aria-label="Başlık"
            className="h-9 min-w-0 flex-1 rounded-lg bg-transparent px-2 text-[15px] font-semibold outline-none placeholder:font-normal placeholder:text-muted hover:bg-surface-2/60 focus:bg-surface-2"
          />
          {!voices.length && (
            <Button size="sm" variant="soft" icon={<PencilRuler size={13} />} onClick={() => navigate("design")}>
              Ses tasarla
            </Button>
          )}
        </div>
        <div role="toolbar" aria-label="Metin araçları" className="flex items-center gap-1 border-b border-border bg-surface px-4 py-1.5">
          <DraftsMenu text={text} onLoad={(t) => editor.current?.setText(t)} />
          <PauseMenu onInsert={(name) => editor.current?.setPause(name)} />
          <Button
            size="sm"
            variant="ghost"
            icon={<Eye size={14} />}
            onClick={async () => setPronunciation(await api.previewPronunciation(text, check?.language ?? "tr"))}
            title="Okunuş sözlüğü uygulanmış, motora gidecek metni göster"
          >
            Okunuş önizleme
          </Button>
          <div className="flex-1" />
          <Button
            size="sm"
            variant={findings > 0 ? "soft" : "ghost"}
            onClick={() => setFixOpen(true)}
            icon={<SpellCheck size={14} />}
            title="Sayıları yazıya çevir, kısaltmaları aç, tırnakları kaldır, cümlelere böl"
          >
            Metni düzelt
            {findings > 0 && <span className="font-mono text-[12px] text-muted tabular-nums">{findings}</span>}
          </Button>
        </div>
        <div className="relative min-h-0 flex-1 overflow-hidden bg-surface">
          <ScriptEditor
            ref={editor}
            text={text}
            onChange={setText}
            check={check}
            onSubmit={narrate}
            placeholder="Okunacak metni yazın. Her satır bir cümle; rakamları yazıyla yazın."
            className="h-full"
          />
        </div>
        <IssueStrip check={check} issues={issues} onGoto={(l, c) => editor.current?.focusLine(l, c)} onFix={() => setFixOpen(true)} />
        <div className="flex items-center gap-4 border-t border-border bg-surface px-6 py-3">
          <div className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-[13px] text-muted">
            <span>
              <b className="font-medium text-text-2 tabular-nums">{check?.line_count ?? 0}</b> satır
            </span>
            <span>
              ~<b className="font-medium text-text-2">{formatDuration(check?.audio_s)}</b> ses
            </span>
            <span title={check?.estimate_samples ? `Bu makinedeki ${check.estimate_samples} işten öğrenildi` : "Henüz ölçüm yok; ilk işlerden sonra iyileşir"}>
              ~<b className="font-medium text-text-2">{formatDuration(check?.job_s)}</b> üretim
            </span>
            {check && (
              <span className="inline-flex items-center gap-1.5">
                <Flag language={check.language} size={10} /> {languageName(check.language)}
              </span>
            )}
            {languageMismatch && (
              <Badge color="amber">
                Ses {languageName(voice!.language)}; metin {languageName(check!.language)}
              </Badge>
            )}
          </div>
          <div className="flex-1" />
          <Button variant="primary" size="lg" onClick={narrate} loading={submitting} icon={<Mic2 size={16} />} title={`${modKey}+Enter`}>
            Seslendir
            <Kbd className="ml-1">{modKey}↵</Kbd>
          </Button>
        </div>
      </div>
      <ResultPanel id={studioResult} voiceId={studioVoice} tab={tab} setTab={setTab} />
      <FixTextDialog
        open={fixOpen}
        onClose={() => setFixOpen(false)}
        text={text}
        language={language === "auto" ? null : language}
        voice={studioVoice}
        onApply={(fixed) => {
          editor.current?.setText(fixed);
          setIssues([]);
          notify(`Metin düzeltildi (${modKey}+Z ile geri alabilirsiniz)`, "success");
        }}
      />
      <Modal open={pronunciation !== null} onClose={() => setPronunciation(null)} title="Okunuş önizleme" description="Motora gidecek metin" width="max-w-2xl">
        <div className="p-5">
          <div className="mb-3 flex items-center gap-2 text-[13px] text-muted">
            <BookOpen size={14} /> Okunuş sözlüğü uygulandı, tırnaklar atıldı. Sözlüğü Ayarlar'dan düzenleyebilirsiniz.
          </div>
          <pre className="selectable rounded-lg bg-surface-2 p-4 font-sans text-[14px] leading-relaxed whitespace-pre-wrap">{pronunciation}</pre>
        </div>
      </Modal>
    </div>
  );
}

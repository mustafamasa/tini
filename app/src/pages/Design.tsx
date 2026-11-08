// Voice design: each run is a separate study (description + design text → variants) kept under designs/.
// Nothing enters the library until the user adds a favourite variant with a name.
import { useEffect, useMemo, useState } from "react";
import { AlertTriangle, BookOpen, Check, Dices, History, Lightbulb, Loader2, Mic2, MoreHorizontal, PencilRuler, Plus, RotateCcw, Trash2, X } from "lucide-react";
import { api, errorIssues, errorMessage, type DesignCheck, type DesignVariant, type Issue, type Job, type VoicePrompt } from "../api";
import { useStore } from "../store";
import { LANGUAGES, formatDate, formatDuration, languageName } from "../languages";
import { JOB_STATUS, JobProgress, useDebounced } from "../components/JobProgress";
import { AudioPlayer } from "../components/AudioPlayer";
import { Select } from "../components/Select";
import {
  Badge,
  Button,
  Card,
  ConfirmDialog,
  Disclosure,
  EmptyState,
  Field,
  Flag,
  IconButton,
  Input,
  IssueList,
  Kbd,
  MenuDivider,
  MenuItem,
  Modal,
  PageHeader,
  Popover,
  ProgressBar,
  PromptDialog,
  SectionLabel,
  Segmented,
  Stepper,
  TextArea,
  cx,
  modKey,
} from "../components/ui";

interface Form {
  name: string;
  description: string;
  text: string;
  language: string;
  variants: number;
  seedMode: "auto" | "manual";
  seeds: string;
  previewText: string;
}

const EMPTY_FORM: Form = { name: "", description: "", text: "", language: "auto", variants: 3, seedMode: "auto", seeds: "", previewText: "" };

export const GENDER_OPTIONS = [
  { value: "all", label: "Kadın ve erkek" },
  { value: "female", label: "Kadın" },
  { value: "male", label: "Erkek" },
];

const TIPS = [
  "İngilizce, 1-3 cümle: yaş ve cinsiyet, tını (derin/parlak, sıcak, genizden), ton ve rol, tempo.",
  "Milliyet ve yerli aksanı yazın: “a native Turkish speaker from Istanbul”.",
  "Sonuna “clean studio recording, no background noise” ekleyin.",
  "Senaryo değil tarif yazın; parantez kullanmayın.",
  "3-5 çeşit isteyip dinleyin: aynı tarif her çeşitte farklı bir ses verir.",
];

function parseSeeds(text: string): number[] {
  return text
    .split(/[\s,;]+/)
    .map((x) => Number(x))
    .filter((n) => Number.isInteger(n) && n >= 0 && n < 2 ** 32);
}

function variantsOf(job: Job): DesignVariant[] {
  return (job.result?.variants as DesignVariant[] | undefined) ?? [];
}

/** Library voice of a variant. Records written before design studies kept the voice id in `id`. */
function libraryIdOf(v: DesignVariant): string | null {
  return v.library_voice_id ?? (v as { id?: string }).id ?? null;
}

function studyName(job: Job): string {
  return String(job.result?.display_name ?? job.result?.name ?? job.request?.name ?? job.title ?? "");
}

// ---------------------------------------------------------------------------------------------------------------

function VoicePromptGallery({ open, onClose, onPick }: { open: boolean; onClose: () => void; onPick: (p: VoicePrompt) => void }) {
  const [prompts, setPrompts] = useState<VoicePrompt[]>([]);
  const [language, setLanguage] = useState("tr");
  const [gender, setGender] = useState("all");
  const [query, setQuery] = useState("");
  useEffect(() => {
    if (open && !prompts.length) api.listVoicePrompts().then(setPrompts);
  }, [open, prompts.length]);
  const languages = useMemo(() => [...new Set(prompts.map((p) => p.language))], [prompts]);
  const list = prompts.filter(
    (p) =>
      p.language === language &&
      (gender === "all" || p.gender === gender) &&
      (!query || (p.name_tr + p.description).toLocaleLowerCase("tr").includes(query.toLocaleLowerCase("tr"))),
  );
  return (
    <Modal
      open={open}
      onClose={onClose}
      title="Örnek tarifler"
      description={`${prompts.length} hazır tarif; birini seçince form doldurulur, istediğiniz gibi değiştirebilirsiniz.`}
      width="max-w-4xl"
    >
      <div className="sticky top-0 z-10 flex flex-wrap items-center gap-2 border-b border-border bg-surface px-5 py-3">
        <div className="flex flex-wrap gap-1.5">
          {languages.map((l) => (
            <button
              key={l}
              type="button"
              onClick={() => setLanguage(l)}
              aria-pressed={l === language}
              className={cx(
                "flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-[13px] transition-colors",
                l === language ? "border-border-strong bg-surface-2 text-text" : "border-border text-text-2 hover:bg-surface-2",
              )}
            >
              <Flag language={l} size={10} /> {languageName(l)}
            </button>
          ))}
        </div>
        <div className="flex-1" />
        <Select value={gender} onChange={setGender} options={GENDER_OPTIONS} size="sm" className="w-36" aria-label="Cinsiyet" />
        <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Ara…" className="h-8 w-40" aria-label="Tariflerde ara" />
      </div>
      <div className="grid grid-cols-2 gap-3 p-5">
        {list.map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={() => {
              onPick(p);
              onClose();
            }}
            className="rounded-xl border border-border p-4 text-left transition-colors hover:border-border-strong hover:bg-surface-2"
          >
            <div className="mb-1 flex items-center gap-2">
              <span className="font-medium">{p.name_tr}</span>
              <Badge>{p.age}</Badge>
            </div>
            <div className="line-clamp-3 text-[13px] text-text-2">{p.description}</div>
            <div className="mt-2 line-clamp-2 text-[12px] text-muted">{p.text}</div>
          </button>
        ))}
        {!list.length && <div className="col-span-2 py-10 text-center text-muted">Eşleşen tarif yok</div>}
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Studies rail

function StudyItem({ job, selected, onSelect }: { job: Job; selected: boolean; onSelect: () => void }) {
  const live = useStore((s) => s.live[job.id]);
  const variants = variantsOf(job);
  const count = variants.length || Number(job.request?.variants ?? 0);
  const noisy = variants.filter((v) => v.noisy).length;
  const added = variants.filter((v) => libraryIdOf(v)).length;
  const active = job.status === "queued" || job.status === "running";
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      className={cx("block w-full rounded-lg px-3 py-2.5 text-left transition", selected ? "bg-surface-2 shadow-[inset_0_0_0_1px_var(--border)]" : "hover:bg-surface-2/60")}
    >
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate font-medium">{studyName(job) || "Adsız tasarım"}</span>
        {job.status === "running" && <Loader2 size={13} className="shrink-0 animate-spin text-accent" />}
      </div>
      <div className="mt-0.5 flex items-center gap-1.5 text-[12px] text-muted">
        <span className="truncate">{formatDate(job.created_at)}</span>
        {count > 0 && <span className="shrink-0">· {count} çeşit</span>}
      </div>
      {job.status === "running" ? (
        <ProgressBar value={live ? live.completed / Math.max(1, live.total) : 0.03} className="mt-2" />
      ) : (active || job.status !== "done" || noisy > 0 || added > 0) && (
        <div className="mt-1.5 flex flex-wrap gap-1">
          {job.status !== "done" && <Badge color={JOB_STATUS[job.status].color}>{JOB_STATUS[job.status].label}</Badge>}
          {added > 0 && (
            <Badge color="green">
              {added} kitaplıkta
            </Badge>
          )}
          {noisy > 0 && <Badge color="amber">{noisy} gürültülü</Badge>}
        </div>
      )}
    </button>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Selected study

function VariantCard({ v, index, job, onReuseSeed, onChanged }: { v: DesignVariant; index: number; job: Job; onReuseSeed: () => void; onChanged: () => void }) {
  const { voices, set, navigate, refreshVoices, notify } = useStore();
  const [adding, setAdding] = useState(false);
  const number = v.variant ?? index + 1;
  const libraryId = libraryIdOf(v);
  const voice = libraryId ? voices.find((x) => x.id === libraryId) : undefined;
  const readWith = (id: string) => {
    set({ studioVoice: id, studioResult: null });
    navigate("studio");
  };

  if (v.error) {
    return (
      <Card className="flex flex-col justify-center gap-1.5 border-dashed p-5 text-center shadow-none">
        <div className="font-medium text-red">Çeşit {number} üretilemedi</div>
        <div className="text-[13px] text-muted">{v.error}</div>
      </Card>
    );
  }
  return (
    <Card className="flex flex-col overflow-hidden">
      <div className="flex items-center gap-2 px-4 pt-3.5 pb-2.5">
        <span className="font-medium">Çeşit {number}</span>
        {v.design_seed !== undefined && (
          <span className="font-mono text-[11px] text-muted" title="Tohum: aynı tarif ve tohum aynı sesi verir">
            #{v.design_seed}
          </span>
        )}
        <div className="flex-1" />
        {voice && (
          <Badge color="green">
            Kitaplıkta
          </Badge>
        )}
        {v.noisy && (
          <Badge color="amber" title={v.noise_margin_db != null ? `Gürültü zemini konuşmanın ${Math.round(v.noise_margin_db)} dB altında` : undefined}>
            Gürültülü
          </Badge>
        )}
      </div>
      <div className="space-y-3 px-4 pb-4">
        <div>
          <div className="mb-1 text-[12px] text-muted">Tasarım kaydı · {formatDuration(v.reference_s)}</div>
          <AudioPlayer path={v.reference_path} compact label={`Çeşit ${number} tasarım kaydı`} />
        </div>
        {v.preview_path ? (
          <div className="rounded-lg bg-surface-2 p-2.5">
            <div className="mb-1 text-[12px] font-medium text-text-2">Dinlemek istediğiniz metin · okumalarda bu ses duyulur</div>
            <AudioPlayer path={v.preview_path} compact label={`Çeşit ${number} örnek okuma`} />
          </div>
        ) : v.preview_error ? (
          <div className="text-[12px] text-amber">Örnek okuma yapılamadı: {v.preview_error}</div>
        ) : null}
        {v.noisy && (
          <div className="flex gap-1.5 text-[12px] leading-snug text-amber">
            <AlertTriangle size={13} className="mt-px shrink-0" />
            <span>Kayıtta sürekli bir arka plan uğultusu var; bu sesle yapılan her okuma da taşır</span>
          </div>
        )}
        {v.warning && <div className="text-[12px] text-amber">{v.warning}</div>}
      </div>
      <div className="mt-auto flex items-center gap-1.5 border-t border-border px-3 py-2.5">
        {voice ? (
          <>
            <Button
              size="sm"
              variant="soft"
              icon={<Check size={13} />}
              onClick={() => {
                set({ selectedVoice: voice.id });
                navigate("library");
              }}
              title="Kitaplıkta göster"
            >
              Kitaplıkta
            </Button>
            <Button size="sm" icon={<Mic2 size={13} />} onClick={() => readWith(voice.id)}>
              Oku
            </Button>
          </>
        ) : (
          <Button size="sm" variant="primary" icon={<Plus size={13} />} onClick={() => setAdding(true)}>
            Kitaplığa ekle
          </Button>
        )}
        <div className="flex-1" />
        <Button size="sm" variant="ghost" icon={<Dices size={13} />} onClick={onReuseSeed} title="Bu çeşidin tohumuyla yeni bir tasarım başlat">
          Aynı tohumla yeniden
        </Button>
      </div>
      <PromptDialog
        open={adding}
        onClose={() => setAdding(false)}
        title="Kitaplığa ekle"
        label="Sesin adı"
        hint="Kitaplıkta ve ses seçicide bu adla görünür; sonra değiştirebilirsiniz"
        initialValue={studyName(job)}
        confirmLabel="Kitaplığa ekle"
        onSubmit={async (name) => {
          const card = await api.addDesignVariant(job.id, number, name);
          await refreshVoices();
          onChanged();
          notify(`“${name}” kitaplığa eklendi`, "success", { label: "Stüdyoda oku", run: () => readWith(card.id) });
        }}
      />
    </Card>
  );
}

function StudyView({ id, onReuse, onDeleted }: { id: string; onReuse: (job: Job, v?: DesignVariant) => void; onDeleted: () => void }) {
  const { set, notify, revision, queue, voices } = useStore();
  const [job, setJob] = useState<Job | null>(null);
  const [tick, setTick] = useState(0);
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => {
    let cancelled = false;
    api
      .getJob(id)
      .then((j) => !cancelled && setJob(j))
      .catch(() => !cancelled && setJob(null));
    return () => {
      cancelled = true;
    };
  }, [id, revision, queue, voices, tick]);
  if (!job || job.id !== id) return <div className="p-7 text-muted">Yükleniyor…</div>;
  const variants = variantsOf(job);
  const active = job.status === "queued" || job.status === "running";
  const description = String(job.result?.description ?? job.request?.description ?? "");
  const designText = String(job.result?.design_text ?? job.request?.text ?? "");
  const previewText = String(job.request?.preview_text ?? "");
  const language = (job.result?.language ?? job.request?.language ?? job.language) as string | null;
  const rerun = async () => {
    try {
      const next = await api.rerunJob(job.id);
      set({ designResult: next.id });
      notify("Aynı ayarlarla yeni bir tasarım sıraya eklendi", "success");
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  return (
    <div className="mx-auto max-w-[1080px] px-7 py-6">
      <div className="mb-4 flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-[18px] font-semibold">{studyName(job) || "Adsız tasarım"}</h2>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[13px] text-muted">
            <span>{formatDate(job.created_at)}</span>
            {language && (
              <span className="inline-flex items-center gap-1.5">
                · <Flag language={language} size={10} /> {languageName(language)}
              </span>
            )}
            {(variants.length || Number(job.request?.variants ?? 0)) > 0 && <span>· {variants.length || Number(job.request?.variants)} çeşit</span>}
            {job.duration_s ? <span>· {formatDuration(job.duration_s)} üretim</span> : null}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          {!active && (
            <Button size="sm" icon={<RotateCcw size={13} />} onClick={rerun} title="Aynı tarif ve ayarlarla yeni bir tasarım">
              Yeniden çalıştır
            </Button>
          )}
          <Button size="sm" icon={<PencilRuler size={13} />} onClick={() => onReuse(job)} title="Bu tasarımın tarifini yeni bir formda aç">
            Düzenleyip yeniden
          </Button>
          <Popover
            align="right"
            width={220}
            trigger={(toggle) => (
              <IconButton label="Diğer" onClick={toggle}>
                <MoreHorizontal size={16} />
              </IconButton>
            )}
          >
            {(close) => (
              <div className="py-1">
                <MenuItem
                  icon={<History size={14} />}
                  onClick={() => {
                    close();
                    set({ selectedJob: job.id });
                    useStore.getState().navigate("jobs");
                  }}
                >
                  Üretimlerde göster
                </MenuItem>
                <MenuDivider />
                <MenuItem
                  danger
                  icon={<Trash2 size={14} />}
                  disabled={active}
                  description={active ? "Sürerken silinemez; önce iptal edin" : undefined}
                  onClick={() => {
                    close();
                    setConfirmDelete(true);
                  }}
                >
                  Tasarımı sil
                </MenuItem>
              </div>
            )}
          </Popover>
        </div>
      </div>

      <div className="mb-5 grid gap-3 rounded-xl border border-border bg-surface p-4 md:grid-cols-2">
        <div className="min-w-0">
          <div className="mb-1 text-[12px] font-medium text-muted">Ses tarifi</div>
          <p className="selectable text-[13px] leading-relaxed text-text-2">{description || "–"}</p>
        </div>
        <div className="min-w-0">
          <div className="mb-1 text-[12px] font-medium text-muted">Okutulan metin</div>
          <p className="selectable text-[13px] leading-relaxed text-text-2">{designText || "–"}</p>
          {previewText && (
            <>
              <div className="mt-3 mb-1 text-[12px] font-medium text-muted">Dinlemek istediğiniz metin</div>
              <p className="selectable text-[13px] leading-relaxed text-text-2">{previewText}</p>
            </>
          )}
        </div>
      </div>

      {job.status !== "done" && (
        <div className="mb-5">
          <JobProgress job={job} onRetry={rerun} />
        </div>
      )}

      {active && Number(job.request?.variants ?? 0) > 0 && (
        <>
          <SectionLabel>Çeşitler</SectionLabel>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-4">
            {Array.from({ length: Number(job.request?.variants) }, (_, i) => (
              <div key={i} className="flex h-24 flex-col justify-between rounded-xl border border-dashed border-border p-4">
                <div className="flex items-center justify-between">
                  <span className="font-medium text-text-2">Çeşit {i + 1}</span>
                  <span className="text-[12px] text-muted">{job.status === "running" ? "hazırlanıyor" : "sırada"}</span>
                </div>
                <div className="h-px bg-border" />
              </div>
            ))}
          </div>
        </>
      )}

      {variants.length > 0 && (
        <>
          <SectionLabel extra={<span className="text-muted">Beğendiğinizi adlandırıp kitaplığa ekleyin</span>}>Çeşitler</SectionLabel>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-4">
            {variants.map((v, i) => (
              <VariantCard key={v.variant ?? i} v={v} index={i} job={job} onReuseSeed={() => onReuse(job, v)} onChanged={() => setTick((t) => t + 1)} />
            ))}
          </div>
        </>
      )}

      <ConfirmDialog
        open={confirmDelete}
        onClose={() => setConfirmDelete(false)}
        destructive
        title="Tasarımı sil"
        confirmLabel="Sil"
        description={
          <>
            “{studyName(job) || "Adsız tasarım"}” ve bütün çeşitleri silinecek. Kitaplığa eklediğiniz sesler kitaplıkta kalır. Bu işlem geri alınamaz.
          </>
        }
        onConfirm={async () => {
          try {
            await api.deleteDesign(job.id);
            onDeleted();
            notify("Tasarım silindi", "success");
          } catch (e) {
            notify(errorMessage(e), "error");
          }
        }}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// New design form

function DesignForm({
  form,
  setForm,
  onClose,
  onCreated,
  onOpenGallery,
}: {
  form: Form;
  setForm: (f: Form | ((f: Form) => Form)) => void;
  onClose: (() => void) | null;
  onCreated: (job: Job) => void;
  onOpenGallery: () => void;
}) {
  const notify = useStore((s) => s.notify);
  const [check, setCheck] = useState<DesignCheck | null>(null);
  const [issues, setIssues] = useState<Issue[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const debounced = useDebounced(form, 300);
  const update = (p: Partial<Form>) => {
    setForm((f) => ({ ...f, ...p }));
    const fields = Object.keys(p);
    setIssues((list) => list.filter((s) => !fields.includes(fieldOf(s))));
  };
  const seeds = form.seedMode === "manual" ? parseSeeds(form.seeds) : [];

  useEffect(() => {
    localStorage.setItem("tini.designForm", JSON.stringify(debounced));
    api
      .checkDesign(
        debounced.description,
        debounced.text,
        debounced.language === "auto" ? null : debounced.language,
        debounced.seedMode === "manual" ? parseSeeds(debounced.seeds).length || 1 : debounced.variants,
        debounced.previewText || null,
      )
      .then(setCheck)
      .catch(() => {});
  }, [debounced]);

  const submit = async () => {
    const missing: Issue[] = [];
    if (!form.description.trim()) missing.push({ code: "missing", message: "Sesi İngilizce tarif edin", data: { field: "description" } });
    if (!form.text.trim()) missing.push({ code: "missing", message: "Ses yaratılırken okunacak metni yazın", data: { field: "text" } });
    if (form.seedMode === "manual" && !seeds.length) missing.push({ code: "missing", message: "En az bir tohum yazın ya da “Rastgele” seçin", data: { field: "seeds" } });
    if (missing.length) return setIssues(missing);
    setIssues([]);
    setSubmitting(true);
    try {
      const job = await api.enqueueJob({
        kind: "design",
        name: form.name,
        description: form.description,
        text: form.text,
        language: form.language === "auto" ? null : form.language,
        variants: seeds.length || form.variants,
        seeds: seeds.length ? seeds : null,
        preview_text: form.previewText.trim() || null,
      });
      notify("Tasarım sıraya eklendi; bitince çeşitleri dinleyebilirsiniz", "success");
      onCreated(job);
    } catch (e) {
      const found = errorIssues(e);
      if (found.length) setIssues(found);
      else notify(errorMessage(e), "error");
    } finally {
      setSubmitting(false);
    }
  };

  const ratio = check ? check.seconds / check.limit : 0;
  const fieldError = (field: string) =>
    issues
      .filter((s) => fieldOf(s) === field)
      .map((s) => s.message)
      .join(" ") || undefined;
  const otherIssues = issues.filter((s) => !["name", "description", "text", "preview_text", "seeds"].includes(fieldOf(s)));
  const meterColor = ratio > 1.1 ? "bg-red" : ratio > 1 ? "bg-amber" : ratio > 0.6 ? "bg-green" : "bg-amber";

  return (
    <div
      className="mx-auto max-w-[760px] px-7 py-6"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
          e.preventDefault();
          submit();
        }
      }}
    >
      <Card>
        <div className="flex items-center gap-3 border-b border-border px-6 py-4">
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-semibold">Yeni ses tasarımı</div>
            <div className="text-[13px] text-muted">Tarif ve okutulacak metinden birkaç çeşit üretilir; hiçbiri siz eklemeden kitaplığa girmez</div>
          </div>
          {onClose && (
            <IconButton label="Formu kapat" onClick={onClose}>
              <X size={16} />
            </IconButton>
          )}
        </div>
        <div className="space-y-5 px-6 py-5">
          <Field label="Tasarım adı" extra="isteğe bağlı" hint="Kitaplığa eklerken önerilen ad olur" error={fieldError("name")}>
            <Input value={form.name} onChange={(e) => update({ name: e.target.value })} placeholder="ör. Deniz feneri anlatıcısı" invalid={!!fieldError("name")} />
          </Field>
          <Field
            label="Ses tarifi"
            extra={
              <button type="button" onClick={onOpenGallery} className="inline-flex items-center gap-1 font-medium text-accent-strong hover:underline">
                <BookOpen size={12} /> Örnek tarifler
              </button>
            }
            error={fieldError("description")}
          >
            <TextArea
              rows={4}
              value={form.description}
              onChange={(e) => update({ description: e.target.value })}
              invalid={!!fieldError("description")}
              placeholder="A calm Turkish woman in her forties with a warm, clear mezzo voice, a native Turkish speaker from Istanbul, measured pace, clean studio recording, no background noise"
            />
            <Disclosure
              className="mt-1.5"
              title={
                <span className="inline-flex items-center gap-1.5 text-muted">
                  <Lightbulb size={13} /> İyi bir tarif nasıl yazılır? (İngilizce yazın)
                </span>
              }
            >
              <ul className="list-inside list-disc space-y-1 rounded-lg bg-surface-2 px-4 py-3 text-[13px] text-text-2">
                {TIPS.map((t) => (
                  <li key={t}>{t}</li>
                ))}
              </ul>
            </Disclosure>
          </Field>
          <Field
            label="Ses yaratılırken okunacak metin"
            extra={check ? `${check.seconds.toLocaleString("tr-TR", { maximumFractionDigits: 1 })} / ${check.limit} sn` : undefined}
            error={fieldError("text")}
          >
            <TextArea
              rows={3}
              value={form.text}
              onChange={(e) => update({ text: e.target.value })}
              invalid={!!fieldError("text")}
              placeholder="Sesin dilinde 1-3 doğal cümle. Bu kayıt sesin referansı olur; 20-30 sn ideal."
            />
            <div className="mt-2 flex items-center gap-3">
              <div className="h-1 flex-1 overflow-hidden rounded-full bg-surface-3">
                <div className={cx("h-full rounded-full transition-[width,background-color]", meterColor)} style={{ width: `${Math.min(100, ratio * 100)}%` }} />
              </div>
              <span className="shrink-0 text-[12px] text-muted">
                {!form.text.trim() ? "20-30 sn ideal" : ratio > 1.1 ? "Çok uzun; kısaltın" : ratio < 0.3 ? "Kısa; 8 sn altı zayıf kalır" : "İyi bir uzunluk"}
              </span>
            </div>
          </Field>
          <div className="grid grid-cols-2 gap-4">
            <Field label="Dil">
              <Select
                value={form.language}
                onChange={(language) => update({ language })}
                aria-label="Dil"
                options={[
                  { value: "auto", label: check ? `Otomatik (${languageName(check.language)})` : "Otomatik" },
                  ...LANGUAGES.map((l) => ({ value: l.code, label: l.name, icon: <Flag language={l.code} size={10} /> })),
                ]}
              />
            </Field>
            <Field label="Çeşit sayısı" hint={form.seedMode === "manual" ? "Verdiğiniz tohum sayısı kadar" : "3-5 çeşit önerilir"}>
              {form.seedMode === "manual" ? (
                <div className="flex h-9 items-center rounded-lg border border-border bg-surface-2 px-3 font-medium tabular-nums">{seeds.length}</div>
              ) : (
                <Stepper value={form.variants} onChange={(variants) => update({ variants })} min={1} max={10} aria-label="Çeşit sayısı" />
              )}
            </Field>
          </div>
          <Field
            label="Dinlemek istediğiniz metin"
            extra="isteğe bağlı"
            hint="Her çeşit, tasarımdan sonra bu metni okur; çeşitleri kendi metninizle karşılaştırırsınız"
            error={fieldError("preview_text")}
          >
            <TextArea rows={2} value={form.previewText} onChange={(e) => update({ previewText: e.target.value })} placeholder="Satır başına bir cümle" />
          </Field>
          <Disclosure title="Gelişmiş" defaultOpen={form.seedMode === "manual"}>
            <Field
              label="Tohum"
              hint="Aynı tarif ve tohum aynı sesi verir. Beğendiğiniz bir çeşidi küçük değişikliklerle yeniden üretmek için kullanın."
              error={fieldError("seeds")}
            >
              <div className="flex items-center gap-2">
                <Segmented
                  value={form.seedMode}
                  onChange={(seedMode) => update({ seedMode })}
                  aria-label="Tohum"
                  options={[
                    { value: "auto", label: "Rastgele" },
                    { value: "manual", label: "Kendim vereceğim" },
                  ]}
                />
                {form.seedMode === "manual" && (
                  <Input
                    value={form.seeds}
                    onChange={(e) => update({ seeds: e.target.value })}
                    placeholder="7, 42, 2692139539"
                    className="font-mono"
                    invalid={!!fieldError("seeds")}
                    aria-label="Tohumlar"
                  />
                )}
              </div>
            </Field>
          </Disclosure>
          <IssueList issues={otherIssues} />
        </div>
        <div className="sticky bottom-0 z-10 flex items-center gap-3 rounded-b-xl border-t border-border bg-surface px-6 py-3 shadow-[0_-6px_12px_-10px_rgb(0_0_0/0.25)]">
          <div className="text-[13px] text-muted">{check ? `~${formatDuration(check.job_s)} sürer · sıraya eklenir` : "Sıraya eklenir"}</div>
          <div className="flex-1" />
          <Button variant="ghost" onClick={() => setForm({ ...EMPTY_FORM })}>
            Temizle
          </Button>
          <Button variant="primary" size="lg" icon={<PencilRuler size={16} />} onClick={submit} loading={submitting} title={`${modKey}+Enter`}>
            Tasarla <Kbd className="ml-1">{modKey}↵</Kbd>
          </Button>
        </div>
      </Card>
    </div>
  );
}

function fieldOf(issue: Issue): string {
  return String(issue.data?.field ?? "");
}

// ---------------------------------------------------------------------------------------------------------------

export function Design() {
  const { designResult, set, revision, queue, voices } = useStore();
  const [studies, setStudies] = useState<Job[] | null>(null);
  const [composing, setComposing] = useState(false);
  const [galleryOpen, setGalleryOpen] = useState(false);
  const [form, setForm] = useState<Form>(() => {
    try {
      return { ...EMPTY_FORM, ...JSON.parse(localStorage.getItem("tini.designForm") ?? "{}") };
    } catch {
      return EMPTY_FORM;
    }
  });

  useEffect(() => {
    api
      .listJobs({ kind: "design", limit: 200 })
      .then(setStudies)
      .catch(() => setStudies([]));
  }, [revision, queue, voices, designResult]);

  // Keep a study selected (the newest) unless the form is open.
  useEffect(() => {
    if (!studies || composing) return;
    if (!designResult || !studies.some((j) => j.id === designResult)) {
      if (studies.length) set({ designResult: studies[0].id });
    }
  }, [studies, designResult, composing, set]);

  const openForm = (patch?: Partial<Form>) => {
    if (patch) setForm((f) => ({ ...f, ...patch }));
    setComposing(true);
  };

  const reuse = (job: Job, v?: DesignVariant) =>
    openForm({
      name: studyName(job),
      description: String(job.result?.description ?? job.request?.description ?? ""),
      text: String(job.result?.design_text ?? job.request?.text ?? ""),
      language: String(job.result?.language ?? job.request?.language ?? "auto") || "auto",
      previewText: String(job.request?.preview_text ?? ""),
      ...(v?.design_seed !== undefined ? { seedMode: "manual" as const, seeds: String(v.design_seed), variants: 1 } : { seedMode: "auto" as const, seeds: "" }),
    });

  const selected = studies?.find((j) => j.id === designResult) ?? null;
  const activeCount = studies?.filter((j) => j.status === "queued" || j.status === "running").length ?? 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Ses tasarımı"
        description="Sesi İngilizce tarif edin, birkaç çeşit dinleyin; beğendiğinizi adlandırıp kitaplığa ekleyin"
        actions={
          <>
            <Button variant="ghost" icon={<BookOpen size={15} />} onClick={() => setGalleryOpen(true)}>
              Örnek tarifler
            </Button>
            <Button variant="primary" icon={<Plus size={15} />} onClick={() => openForm()}>
              Yeni tasarım
            </Button>
          </>
        }
      />
      <div className="flex min-h-0 flex-1">
        <aside aria-label="Tasarımlar" className="flex w-[272px] shrink-0 flex-col border-r border-border bg-surface">
          <div className="px-3 pt-4">
            <SectionLabel extra={activeCount > 0 ? <span>{activeCount} sürüyor</span> : studies?.length ? <span>{studies.length}</span> : undefined}>
              Tasarımlar
            </SectionLabel>
          </div>
          <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-2 pb-3">
            {composing && (
              <div className="flex items-center gap-2 rounded-lg bg-surface-2 px-3 py-2.5 text-[13px] font-medium shadow-[inset_0_0_0_1px_var(--border)]">
                <PencilRuler size={14} /> Yeni tasarım
              </div>
            )}
            {studies?.map((j) => (
              <StudyItem
                key={j.id}
                job={j}
                selected={!composing && j.id === designResult}
                onSelect={() => {
                  setComposing(false);
                  set({ designResult: j.id });
                }}
              />
            ))}
            {studies && !studies.length && !composing && (
              <div className="px-2 py-3 text-[13px] leading-relaxed text-muted">Her tasarım burada ayrı durur; aynı anda birkaçını sıraya ekleyebilirsiniz</div>
            )}
          </div>
        </aside>
        <div className="min-w-0 flex-1 overflow-y-auto">
          {composing || (studies && !studies.length) ? (
            studies && !studies.length && !composing ? (
              <div className="flex min-h-full items-center justify-center">
                <EmptyState
                  icon={<PencilRuler size={22} />}
                  title="İlk sesinizi tasarlayın"
                  description="Sesi İngilizce birkaç cümleyle tarif edin ve ses yaratılırken okunacak metni yazın. Model birkaç çeşit üretir; dinleyip beğendiğinizi kitaplığa eklersiniz."
                  action={
                    <>
                      <Button variant="primary" icon={<Plus size={15} />} onClick={() => openForm()}>
                        Yeni tasarım
                      </Button>
                      <Button icon={<BookOpen size={15} />} onClick={() => setGalleryOpen(true)}>
                        Örnek tarifler
                      </Button>
                    </>
                  }
                />
              </div>
            ) : (
              <DesignForm
                form={form}
                setForm={setForm}
                onOpenGallery={() => setGalleryOpen(true)}
                onClose={studies?.length ? () => setComposing(false) : null}
                onCreated={(job) => {
                  setComposing(false);
                  setStudies((list) => [job, ...(list ?? [])]);
                  set({ designResult: job.id });
                }}
              />
            )
          ) : selected ? (
            <StudyView
              key={selected.id}
              id={selected.id}
              onReuse={reuse}
              onDeleted={() => {
                setStudies((list) => list?.filter((j) => j.id !== selected.id) ?? null);
                set({ designResult: null });
              }}
            />
          ) : (
            <div className="p-7 text-muted">Yükleniyor…</div>
          )}
        </div>
      </div>
      <VoicePromptGallery
        open={galleryOpen}
        onClose={() => setGalleryOpen(false)}
        onPick={(p) =>
          openForm({
            name: p.name_tr + (p.language !== "tr" ? ` (${p.language})` : ""),
            description: p.description,
            text: p.text,
            language: p.language,
          })
        }
      />
    </div>
  );
}

// Automatic text fixing: options, list of changes and a preview of the new text.
import { useEffect, useMemo, useState } from "react";
import { ArrowRight, CheckCircle2, SpellCheck } from "lucide-react";
import { api, type FixOptions, type FixResult } from "../api";
import { useDebounced } from "./JobProgress";
import { Badge, Button, Checkbox, Modal, cx } from "./ui";

const DEFAULT_OPTIONS: FixOptions = { numbers: true, abbreviations: true, citations: true, parentheses: true, quotes: true, sentences: true };

const OPTIONS: { key: keyof FixOptions; kind: string; label: string; description: string; turkishOnly?: boolean }[] = [
  { key: "numbers", kind: "number", label: "Sayıları yazıya çevir", description: "1994'te → bin dokuz yüz doksan dörtte; %50, 3,5, 14:30, 19. yüzyıl", turkishOnly: true },
  { key: "abbreviations", kind: "abbreviation", label: "Kısaltmaları aç", description: "Hz. → Hazreti, s. 240 → sayfa iki yüz kırk, Bk. → bakınız, vb.", turkishOnly: true },
  { key: "citations", kind: "citation", label: "Kaynak gösterimlerini çıkar", description: "(Korkmaz, 1994, s. 240) gibi yıl ya da sayfa içeren parantezler okunmaz" },
  { key: "parentheses", kind: "parenthesis", label: "Parantezleri kaldır", description: "İçerik kalır. Satır başındaki parantezi model ses tarifi sanabilir" },
  { key: "quotes", kind: "quote", label: "Tırnak ve kesme işaretlerini kaldır", description: "Ali'ye → Aliye. Türkçe dışındaki dillerde kesme işareti kalır" },
  { key: "sentences", kind: "sentence", label: "Cümleleri satırlara böl", description: "Model cümle cümle okur; boş satırlar paragraf durağı olarak kalır" },
];

const KIND_LABEL: Record<string, string> = {
  number: "Sayı",
  abbreviation: "Kısaltma",
  citation: "Kaynak",
  parenthesis: "Parantez",
  quote: "Tırnak",
  sentence: "Cümle",
};

function loadOptions(): FixOptions {
  try {
    return { ...DEFAULT_OPTIONS, ...JSON.parse(localStorage.getItem("tini.fixOptions") ?? "{}") };
  } catch {
    return DEFAULT_OPTIONS;
  }
}

export function FixTextDialog({
  open,
  onClose,
  text,
  language,
  voice,
  onApply,
}: {
  open: boolean;
  onClose: () => void;
  text: string;
  language: string | null;
  voice: string | null;
  onApply: (text: string) => void;
}) {
  const [options, setOptions] = useState<FixOptions>(loadOptions);
  const [result, setResult] = useState<FixResult | null>(null);
  const [tab, setTab] = useState<"changes" | "text">("changes");
  const debounced = useDebounced(options, 150);

  useEffect(() => {
    localStorage.setItem("tini.fixOptions", JSON.stringify(debounced));
    if (open) api.fixText(text, language, voice, debounced).then(setResult).catch(() => setResult(null));
  }, [open, text, language, voice, debounced]);

  const counts = useMemo(() => {
    const m: Record<string, number> = {};
    for (const c of result?.changes ?? []) m[c.kind] = (m[c.kind] ?? 0) + 1;
    return m;
  }, [result]);
  const turkish = (result?.language ?? "tr") === "tr";
  const changed = result && result.text !== text;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={
        <span className="flex items-center gap-2">
          <SpellCheck size={16} className="text-muted" /> Metni okunabilir hale getir
        </span>
      }
      width="max-w-4xl"
      footer={
        <div className="flex w-full items-center gap-3">
          <div className="text-[13px] text-muted">
            {result &&
              (result.remaining.length ? `${result.remaining.length} sorun elle düzeltilmeli` : <span className="text-green">Düzeltmeden sonra sorun kalmıyor</span>)}
          </div>
          <div className="flex-1" />
          <Button variant="ghost" onClick={onClose}>
            Vazgeç
          </Button>
          <Button
            variant="primary"
            icon={changed ? <CheckCircle2 size={15} /> : undefined}
            onClick={() => {
              if (result && changed) onApply(result.text);
              onClose();
            }}
          >
            {changed ? "Uygula" : "Kapat"}
          </Button>
        </div>
      }
    >
      <div className="grid min-h-[420px] grid-cols-[300px_1fr]">
        <div className="space-y-3 border-r border-border p-5">
          {OPTIONS.map((o) => (
            <div key={o.key} className={cx(o.turkishOnly && !turkish && "opacity-45")}>
              <Checkbox checked={options[o.key]} onChange={(v) => setOptions({ ...options, [o.key]: v })}>
                <span className="font-medium">{o.label}</span>
                {(counts[o.kind] ?? 0) > 0 && (
                  <Badge color="accent" className="ml-1.5">
                    {counts[o.kind]}
                  </Badge>
                )}
                <span className="mt-0.5 block text-[12px] text-muted">{o.turkishOnly && !turkish ? "Yalnız Türkçe metinde" : o.description}</span>
              </Checkbox>
            </div>
          ))}
        </div>
        <div className="flex min-h-0 flex-col">
          <div className="flex gap-1 border-b border-border px-4 py-2">
            {(
              [
                ["changes", `Değişiklikler (${result?.changes.length ?? 0})`],
                ["text", "Yeni metin"],
              ] as const
            ).map(([k, label]) => (
              <button
                key={k}
                onClick={() => setTab(k)}
                className={cx("h-8 rounded-md px-3 text-[13px] font-medium transition-colors", tab === k ? "bg-surface-2 text-text shadow-[inset_0_0_0_1px_var(--border)]" : "text-text-2 hover:bg-surface-2/60 hover:text-text")}
              >
                {label}
              </button>
            ))}
          </div>
          <div className="selectable max-h-[52vh] min-h-0 flex-1 overflow-y-auto">
            {tab === "changes" ? (
              result?.changes.length ? (
                <div className="divide-y divide-border">
                  {result.changes.map((c, i) => (
                    <div key={i} className="flex items-center gap-3 px-5 py-2 text-[13px]">
                      <Badge className="w-[72px] justify-center">{KIND_LABEL[c.kind] ?? c.kind}</Badge>
                      <span
                        className={cx(
                          "min-w-0 flex-1 truncate",
                          c.kind !== "sentence" && c.kind !== "quote" && "font-mono text-[13px] text-red line-through decoration-red/40",
                        )}
                      >
                        {c.before}
                      </span>
                      <ArrowRight size={13} className="shrink-0 text-muted" />
                      <span className="min-w-0 flex-1 truncate text-green">{c.after || <i className="text-muted">kaldırıldı</i>}</span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="p-8 text-center text-muted">Düzeltilecek bir şey yok</div>
              )
            ) : (
              <ol className="py-3">
                {(result?.text ?? "").split("\n").map((l, i) => (
                  <li key={i} className="flex gap-3 px-5 py-0.5 text-[14px] leading-relaxed">
                    <span className="w-6 shrink-0 text-right font-mono text-[11px] leading-6 text-muted">{i + 1}</span>
                    <span className={cx(!l.trim() && "h-3")}>{l}</span>
                  </li>
                ))}
              </ol>
            )}
          </div>
        </div>
      </div>
    </Modal>
  );
}

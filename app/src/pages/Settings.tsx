// Settings: general, engine, model, pronunciation dictionary, data, about.
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { BookA, Cpu, Database, ExternalLink, Eye, EyeOff, FolderOpen, HardDrive, Info, Play, Plug, Plus, RefreshCw, SlidersHorizontal, Trash2 } from "lucide-react";
import { api, errorMessage, type ApiInfo } from "../api";
import { useStore } from "../store";
import { formatBytes, formatDuration } from "../languages";
import { EngineEnvCard, ModelCard } from "./Setup";
import { Select } from "../components/Select";
import { VoicePicker } from "../components/VoicePicker";
import { AudioPlayer } from "../components/AudioPlayer";
import { JOB_STATUS, useJob } from "../components/JobProgress";
import { Badge, Button, Card, ConfirmDialog, CopyButton, Input, Segmented, StatusDot, Switch, cx } from "../components/ui";
import { Logo, Wordmark } from "../components/Logo";

type Section = "general" | "engine" | "model" | "dictionary" | "api" | "data" | "about";

const SECTIONS: { id: Section; label: string; description: string; icon: React.ReactNode }[] = [
  { id: "general", label: "Genel", description: "Görünüm ve dışa aktarma.", icon: <SlidersHorizontal size={15} /> },
  { id: "engine", label: "Motor", description: "Ses motorunun cihazı, kalitesi ve bellek kullanımı.", icon: <Cpu size={15} /> },
  { id: "model", label: "Model", description: "Ses motoru ortamı ve VoxCPM2 model dosyaları.", icon: <HardDrive size={15} /> },
  { id: "dictionary", label: "Okunuş sözlüğü", description: "Yabancı kelime ve kısaltmaların nasıl okunacağı.", icon: <BookA size={15} /> },
  { id: "api", label: "Yerel API", description: "Başka uygulamaların Tını'yı bu bilgisayardan kullanması için HTTP arayüzü.", icon: <Plug size={15} /> },
  { id: "data", label: "Veri", description: "Seslerin, üretimlerin ve modelin durduğu klasör.", icon: <Database size={15} /> },
  { id: "about", label: "Hakkında", description: "Sürüm, lisanslar ve sorumlu kullanım.", icon: <Info size={15} /> },
];

function Row({ title, description, children }: { title: string; description?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[220px_1fr] items-start gap-6 border-b border-border py-4 last:border-none">
      <div>
        <div className="font-medium">{title}</div>
        {description && <div className="mt-0.5 text-[12px] text-muted">{description}</div>}
      </div>
      <div>{children}</div>
    </div>
  );
}

function General() {
  const { settings, saveSettings } = useStore();
  if (!settings) return null;
  return (
    <Card className="px-6">
      <Row title="Tema">
        <Segmented
          value={settings.theme}
          onChange={(theme) => saveSettings({ theme })}
          aria-label="Tema"
          options={[
            { value: "system", label: "Sistem" },
            { value: "light", label: "Açık" },
            { value: "dark", label: "Koyu" },
          ]}
        />
      </Row>
      <Row title="Dışa aktarma biçimi" description="Dışa aktar menüsünde her biçim ayrıca seçilebilir">
        <Select
          value={settings.export_format}
          onChange={(export_format) => saveSettings({ export_format })}
          options={[
            { value: "wav", label: "WAV (kayıpsız)" },
            { value: "mp3", label: "MP3 (192 kbps)" },
          ]}
          className="w-56"
          aria-label="Dışa aktarma biçimi"
        />
      </Row>
      <Row title="Dışa aktarma klasörü" description="Kaydet penceresi burada açılır">
        <div className="flex items-center gap-2">
          <Input readOnly value={settings.export_dir ?? ""} placeholder="Her seferinde sor" />
          <Button
            onClick={async () => {
              const dir = await open({ directory: true });
              if (typeof dir === "string") saveSettings({ export_dir: dir });
            }}
          >
            Seç
          </Button>
          {settings.export_dir && (
            <Button variant="ghost" onClick={() => saveSettings({ export_dir: null })}>
              Temizle
            </Button>
          )}
        </div>
      </Row>
      <Row title="Dosya adı şablonu" description={<>Kullanılabilir: {"{ses} {baslik} {tarih} {saat}"}</>}>
        <Input value={settings.file_name_template} onChange={(e) => saveSettings({ file_name_template: e.target.value })} className="font-mono" />
      </Row>
    </Card>
  );
}

function Engine() {
  const { settings, saveSettings, status, notify, refreshStatus } = useStore();
  const [cfgText, setCfgText] = useState<string | null>(null);
  if (!settings || !status) return null;
  const isDefault = settings.steps === 10 && settings.cfg === 2 && settings.precision === "bfloat16" && settings.device === "auto";
  return (
    <Card className="px-6">
      <Row title="Motor" description={status.engine.loaded ? "Model şu an bellekte" : "Model bellekte değil; ilk iş ~20 sn yükleme yapar"}>
        <div className="flex items-center gap-3">
          <div className="text-[13px] text-text-2">Resmi VoxCPM · VoxCPM2 · PyTorch · 48 kHz</div>
          <Button
            size="sm"
            disabled={!status.engine.loaded}
            onClick={async () => {
              try {
                await api.unloadEngine();
                notify("Model bellekten çıkarıldı", "success");
                refreshStatus();
              } catch (e) {
                notify(errorMessage(e), "error");
              }
            }}
          >
            Belleği boşalt
          </Button>
        </div>
      </Row>
      <Row title="Cihaz" description="Otomatik: Apple GPU (MPS). CPU yalnız sorun ayıklamak için; çok yavaştır.">
        <Select
          value={settings.device}
          onChange={async (device) => {
            await saveSettings({ device });
            refreshStatus();
          }}
          options={[
            { value: "auto", label: "Otomatik (Apple GPU)" },
            { value: "mps", label: "Apple GPU (MPS)" },
            { value: "cpu", label: "CPU", description: "Çok yavaş; yalnız sorun ayıklamak için" },
          ]}
          className="w-64"
          aria-label="Cihaz"
        />
      </Row>
      <Row
        title="Hassasiyet"
        description="bfloat16: ~5,5 GB bellek, gerçek zamanın ~2 katı sürede üretim. float32: ~10 GB, 16 GB Mac'te takasa düşer ve 5-10 kat yavaşlar; yalnız bfloat16'da bozuk çıktı alırsanız deneyin."
      >
        <Select
          value={settings.precision}
          onChange={(precision) => saveSettings({ precision })}
          options={[
            { value: "bfloat16", label: "bfloat16 (önerilen)", description: "~5,5 GB bellek, hızlı" },
            { value: "float32", label: "float32", description: "~10 GB bellek, en kararlı ama yavaş" },
          ]}
          className="w-64"
          aria-label="Hassasiyet"
        />
      </Row>
      <Row title="Çözücü adımı" description="VoxCPM inference_timesteps. Çok adım ayrıntıyı artırır ama yavaşlatır; önerilen 10, kullanışlı aralık 4-30.">
        <Select
          value={String(settings.steps)}
          onChange={(v) => saveSettings({ steps: Number(v) })}
          options={[
            { value: "6", label: "6 · hızlı taslak" },
            { value: "10", label: "10 · önerilen" },
            { value: "16", label: "16 · ayrıntılı" },
            { value: "25", label: "25 · en ayrıntılı (yavaş)" },
            ...(![6, 10, 16, 25].includes(settings.steps) ? [{ value: String(settings.steps), label: `${settings.steps} · özel` }] : []),
          ]}
          className="w-64"
          aria-label="Çözücü adımı"
        />
      </Row>
      <Row
        title="Yönlendirme (CFG)"
        description="VoxCPM cfg_value: metne ve ses tarifine bağlılık. Önerilen 2.0. Çıktı gürültülü ya da vızıltılı gelirse 1,5-1,6'ya indirin; ses tarife uymuyorsa 2,5-3'e çıkarın."
      >
        <div className="flex items-center gap-3">
          <input
            type="range"
            min={1}
            max={3}
            step={0.1}
            value={settings.cfg}
            onChange={(e) => saveSettings({ cfg: Math.round(Number(e.target.value) * 10) / 10 })}
            aria-label="Yönlendirme (CFG)"
            className="w-48 accent-[var(--accent)]"
          />
          <Input
            value={cfgText ?? settings.cfg.toFixed(1)}
            onChange={(e) => setCfgText(e.target.value)}
            onBlur={() => {
              const v = Number((cfgText ?? "").replace(",", "."));
              if (cfgText !== null && v >= 1 && v <= 4) saveSettings({ cfg: Math.round(v * 10) / 10 });
              setCfgText(null);
            }}
            aria-label="CFG değeri"
            className="w-16 font-mono"
          />
        </div>
      </Row>
      <Row title="Bellekte tut" description="Model son işten bu kadar sonra bellekten çıkar; sonraki iş yeniden yükler (~20 sn)">
        <Select
          value={String(settings.idle_unload_min)}
          onChange={(v) => saveSettings({ idle_unload_min: Number(v) })}
          options={[
            { value: "3", label: "3 dakika" },
            { value: "10", label: "10 dakika" },
            { value: "30", label: "30 dakika" },
            { value: "120", label: "2 saat" },
            { value: "0", label: "Uygulama açık kaldıkça" },
          ]}
          className="w-64"
          aria-label="Bellekte tut"
        />
      </Row>
      {!isDefault && (
        <Row title="Varsayılanlar">
          <Button onClick={() => saveSettings({ device: "auto", precision: "bfloat16", steps: 10, cfg: 2 })}>VoxCPM önerilenlerine dön</Button>
        </Row>
      )}
      <Row title="Kelime zamanı">
        <Switch
          checked={settings.word_timing}
          onChange={(word_timing) => saveSettings({ word_timing })}
          label="Okumada kelime zamanı ve satır kırpma"
          description="Okuma whisper ile geri dinlenip kelimeler hizalanır, sonra sesin enerjisine oturtulur (transkript modeli yoksa sesten tahmin edilir). Oynatırken kelime vurgusu, kelime altyazısı ve satır arası sessizliğin tam durak kadar olması için."
        />
      </Row>
      <Row title="Ton eşitleme">
        <Switch
          checked={settings.tone_match}
          onChange={(tone_match) => saveSettings({ tone_match })}
          label="Okumanın tınısını referans kayda yaklaştır"
          description="Okumanın spektrumu sesin kendi referans kaydıyla karşılaştırılır, fazla bantlar en çok 15 dB kısılır. VoxCPM çıktısı temiz olduğundan genelde gerekmez."
        />
      </Row>
      <Row title="Hata ayıklama">
        <Switch
          checked={settings.keep_intermediate}
          onChange={(keep_intermediate) => saveSettings({ keep_intermediate })}
          label="Motor ara dosyalarını sakla"
          description="Varsayılan: başarılı işten sonra satır satır ham motor çıktıları silinir"
        />
      </Row>
      <Row title="Önbellek" description="Seslerin referans kodlamaları (istem önbelleği) ve çalışma klasörü. Silinirse her sesin ilk okuması birkaç saniye uzar.">
        <Button
          icon={<Trash2 size={14} />}
          onClick={async () => {
            const freed = await api.clearCache();
            notify(`${formatBytes(freed)} boşaltıldı`, "success");
          }}
        >
          Önbelleği temizle
        </Button>
      </Row>
    </Card>
  );
}

function Dictionary() {
  const notify = useStore((s) => s.notify);
  const [rules, setRules] = useState<[string, string][]>([]);
  const [comments, setComments] = useState<string[]>([]);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    api.readDictionary().then((content) => {
      const r: [string, string][] = [];
      const c: string[] = [];
      for (const line of content.split("\n")) {
        if (!line.trim() || line.trim().startsWith("#")) c.push(line);
        else {
          const [written, ...spoken] = line.split("\t");
          r.push([written.trim(), spoken.join("\t").trim()]);
        }
      }
      setRules(r);
      setComments(c.filter((l) => l.trim()));
    });
  }, []);
  const save = async () => {
    const content =
      [...comments, ...rules.filter(([a, b]) => a.trim() && b.trim()).map(([a, b]) => `${a.trim()}\t${b.trim()}`)].join("\n") + "\n";
    try {
      const count = await api.saveDictionary(content);
      setDirty(false);
      notify(`Sözlük kaydedildi (${count} kural)`, "success");
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  const update = (i: number, rule: [string, string]) => {
    const next = [...rules];
    next[i] = rule;
    setRules(next);
    setDirty(true);
  };
  return (
    <Card className="p-6">
      <p className="mb-4 text-[13px] text-text-2">
        Türkçe metindeki yabancı kelime ve kısaltmaların okunuşu. Tam kelime eşleşir, büyük-küçük harf fark etmez, uzun ifade önce uygulanır.
        Yalnız Türkçe okumada kullanılır. Stüdyo'da eşleşen kelimelerin altı noktalı çizilir.
      </p>
      <div className="mb-2 grid grid-cols-[1fr_1fr_32px] gap-2 px-1 text-[12px] font-medium text-muted">
        <span>Yazılış</span>
        <span>Okunuş</span>
      </div>
      <div className="max-h-[50vh] space-y-1.5 overflow-y-auto pr-1">
        {rules.map(([a, b], i) => (
          <div key={i} className="grid grid-cols-[1fr_1fr_32px] gap-2">
            <Input value={a} onChange={(e) => update(i, [e.target.value, b])} aria-label={`Kural ${i + 1} yazılış`} />
            <Input value={b} onChange={(e) => update(i, [a, e.target.value])} aria-label={`Kural ${i + 1} okunuş`} />
            <button
              type="button"
              aria-label={`Kural ${i + 1} sil`}
              onClick={() => {
                setRules(rules.filter((_, j) => j !== i));
                setDirty(true);
              }}
              className="flex items-center justify-center rounded-md text-muted hover:bg-surface-2 hover:text-red"
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
      </div>
      <div className="mt-4 flex gap-2">
        <Button
          icon={<Plus size={14} />}
          onClick={() => {
            setRules([...rules, ["", ""]]);
            setDirty(true);
          }}
        >
          Kural ekle
        </Button>
        <div className="flex-1" />
        {dirty ? (
          <Button variant="primary" onClick={save}>
            Değişiklikleri kaydet
          </Button>
        ) : (
          <span className="self-center text-[13px] text-muted">Kaydedildi</span>
        )}
      </div>
    </Card>
  );
}

function Data() {
  const { status, notify } = useStore();
  const [pendingDir, setPendingDir] = useState<string | null>(null);
  if (!status) return null;
  return (
    <Card className="px-6">
      <Row title="Veri klasörü" description="Model, sesler, üretimler ve veritabanı burada">
        <div className="flex items-center gap-2">
          <Input readOnly value={status.data_dir} className="font-mono text-[12px]" />
          <Button icon={<FolderOpen size={14} />} onClick={() => revealItemInDir(status.data_dir)}>
            Göster
          </Button>
        </div>
        <Button
          className="mt-2"
          variant="ghost"
          onClick={async () => {
            const dir = await open({ directory: true, title: "Yeni veri klasörü" });
            if (typeof dir === "string") setPendingDir(dir);
          }}
        >
          Klasörü değiştir…
        </Button>
        <ConfirmDialog
          open={!!pendingDir}
          onClose={() => setPendingDir(null)}
          title="Veri klasörünü değiştir"
          confirmLabel="Değiştir ve yeniden başlat"
          description="Yeni klasör boş başlar; mevcut verileri taşımak isterseniz uygulamayı kapattıktan sonra eski klasörün içeriğini yenisine kopyalayın. Uygulama yeniden başlatılacak."
          onConfirm={async () => {
            try {
              await api.setDataDir(pendingDir!);
              await api.restartApp();
            } catch (e) {
              notify(errorMessage(e), "error");
            }
          }}
        />
      </Row>
      <Row title="İstatistik">
        <div className="text-[13px] text-text-2">
          {status.stats.completed} biten iş · toplam {formatDuration(status.stats.total_audio_s)} ses
        </div>
      </Row>
    </Card>
  );
}

function About() {
  const status = useStore((s) => s.status);
  return (
    <Card className="p-6">
      <div className="mb-5 flex items-center gap-4">
        <Logo size={56} />
        <div>
          <Wordmark height={22} className="mb-1.5" />
          <div className="text-muted">Sürüm {status?.version} · Türkçe ses tasarımı ve seslendirme stüdyosu</div>
        </div>
      </div>
      <div className="space-y-3 text-[13px] leading-relaxed text-text-2">
        <p>
          Model: <b>VoxCPM2</b> (OpenBMB, Apache-2.0). Motor: resmi <b>VoxCPM</b> kütüphanesi (OpenBMB, Apache-2.0) ve <b>PyTorch</b> (BSD-3), Apple GPU (MPS)
          üzerinde; Python ortamı <b>uv</b> (Astral, MIT/Apache-2.0) ile kurulur. Transkript ve kelime hizalama: <b>whisper.cpp</b> (MIT).
        </p>
        <p>144 örnek tarif voxcpm2-turkce-ses-tasarimi projesinden (Apache-2.0). Bayraklar: flag-icons (MIT).</p>
        <p>
          Sorumlu kullanım: üretilen sesler yapaydır ve öyle etiketlenmelidir. Klonlama yalnız kendi sesiniz ya da sahibinin açık izniyle yapılır. İzinsiz taklit,
          dolandırıcılık ve yanıltma yasaktır.
        </p>
        <p className="text-muted">Ayrıntılı lisans listesi uygulama paketindeki THIRD_PARTY_LICENSES.md dosyasındadır</p>
      </div>
    </Card>
  );
}

const EXAMPLE_TEXT = "Merhaba, dinlemiş olduğun bu ses Tını ile üretildi.";

/** Single-quoted for a POSIX shell. */
function shellQuote(s: string) {
  return `'${s.replace(/'/g, "'\\''")}'`;
}

/** The example request: pick a voice, edit the sentence, run it against the real local API and listen to the result. */
function ApiExample({ url, token, showToken, running }: { url: string; token: string; showToken: boolean; running: boolean }) {
  const voices = useStore((s) => s.voices);
  const notify = useStore((s) => s.notify);
  const [voice, setVoice] = useState<string | null>(null);
  const [text, setText] = useState(EXAMPLE_TEXT);
  const [busy, setBusy] = useState(false);
  const [response, setResponse] = useState<{ status: number; summary: string } | null>(null);
  const [jobId, setJobId] = useState<string | null>(null);
  const job = useJob(jobId);
  const voiceId = voice ?? voices[0]?.id ?? "builtin/cansu";
  const body = { kind: "read", voice: voiceId, text: text.trim() || EXAMPLE_TEXT };
  const command = (t: string) =>
    `curl -X POST ${url}/v1/jobs \\\n  -H "Authorization: Bearer ${t}" \\\n  -H "Content-Type: application/json" \\\n  -d ${shellQuote(JSON.stringify(body))}`;
  const run = async () => {
    setBusy(true);
    setResponse(null);
    setJobId(null);
    try {
      const r = await api.callLocalApi("POST", "/v1/jobs", body);
      const b = (r.body ?? {}) as { id?: string; error?: { message?: string } };
      if (r.status === 201 && b.id) {
        setJobId(b.id);
        setResponse({ status: r.status, summary: `iş ${b.id} kuyruğa eklendi` });
      } else {
        setResponse({ status: r.status, summary: b.error?.message ?? "beklenmeyen yanıt" });
      }
    } catch (e) {
      notify(errorMessage(e), "error");
    } finally {
      setBusy(false);
    }
  };
  const state = !job ? null : job.status === "done" ? "Hazır" : job.status === "running" ? "Seslendiriliyor…" : job.status === "queued" ? "Sırada" : JOB_STATUS[job.status].label;
  return (
    <Card className="p-5">
      <div className="mb-4 flex items-start justify-between gap-3">
        <div>
          <div className="font-medium">Örnek istek</div>
          <div className="text-[12px] text-muted">
            Bir okuma işi gönderir. Bütün uçlar ve şemalar: <code className="font-mono">{url}/docs</code>
          </div>
        </div>
        <CopyButton text={command(token)} label="Komutu kopyala" />
      </div>
      <div className="mb-3 grid gap-3 sm:grid-cols-[240px_1fr]">
        <VoicePicker value={voiceId} onChange={setVoice} />
        <Input value={text} onChange={(e) => setText(e.target.value)} aria-label="Okunacak metin" placeholder={EXAMPLE_TEXT} />
      </div>
      <pre className="selectable overflow-x-auto rounded-lg bg-surface-2 p-3.5 font-mono text-[12px] leading-relaxed text-text-2">{command(showToken ? token : "<token>")}</pre>
      <div className="mt-3 flex items-center gap-3">
        <Button variant="primary" icon={<Play size={14} />} loading={busy} disabled={!running} onClick={run} title={running ? undefined : "Önce Yerel API'yi açın"}>
          Çalıştır
        </Button>
        {!running && <span className="text-[12px] text-muted">Çalıştırmak için Yerel API açık olmalı</span>}
        {response && (
          <span className="min-w-0 truncate font-mono text-[12px] text-text-2">
            <span className={response.status < 300 ? "text-green" : "text-red"}>{response.status}</span> · {response.summary}
          </span>
        )}
        {state && <span className="ml-auto shrink-0 text-[12px] text-muted">{state}</span>}
      </div>
      {job?.status === "done" && typeof job.result?.wav === "string" && (
        <div className="mt-3">
          <AudioPlayer path={job.result.wav} version={job.id} label="Örnek istekle üretilen ses" />
        </div>
      )}
      {job && (job.status === "failed" || job.status === "cancelled" || job.status === "interrupted") && (
        <div className="mt-3 text-[12px] text-red">{job.error ?? JOB_STATUS[job.status].label}</div>
      )}
    </Card>
  );
}

function ApiSection() {
  const { settings, saveSettings, notify } = useStore();
  const [info, setInfo] = useState<ApiInfo | null>(null);
  const [portText, setPortText] = useState<string | null>(null);
  const [showToken, setShowToken] = useState(false);
  const [confirmRegenerate, setConfirmRegenerate] = useState(false);
  const enabled = settings?.api_enabled ?? false;
  const port = settings?.api_port;
  useEffect(() => {
    // The server (re)starts after settings are saved; read its state a moment later.
    const t = setTimeout(() => {
      api
        .getApiInfo()
        .then(setInfo)
        .catch(() => setInfo(null));
    }, 350);
    return () => clearTimeout(t);
  }, [enabled, port]);
  if (!settings) return null;
  const url = info?.url ?? `http://127.0.0.1:${settings.api_port}`;
  const token = info?.token ?? settings.api_token;
  const savePort = async () => {
    if (portText === null) return;
    const v = Number(portText);
    setPortText(null);
    if (!Number.isInteger(v) || v < 1024 || v > 65535) return notify("Port 1024 ile 65535 arasında bir sayı olmalı", "error");
    if (v !== settings.api_port) await saveSettings({ api_port: v }).catch(() => {});
  };
  const state = !enabled
    ? { color: "gray" as const, label: "Kapalı" }
    : info?.running
      ? { color: "green" as const, label: "Çalışıyor" }
      : info?.error
        ? { color: "red" as const, label: "Çalışmıyor" }
        : { color: "amber" as const, label: "Başlatılıyor…" };
  return (
    <div className="space-y-4">
      <Card className="px-6">
        <Row title="Yerel API" description="Yalnız bu bilgisayardan (127.0.0.1) erişilir; her istek erişim anahtarı ister">
          <div className="flex items-start justify-between gap-4">
            <Switch
              checked={enabled}
              onChange={(api_enabled) => saveSettings({ api_enabled }).catch(() => {})}
              label={enabled ? "Açık" : "Kapalı"}
              description="Komut satırından, betiklerden ya da başka uygulamalardan ses listesi alın, okuma işi gönderin, sonucu indirin"
            />
            <Badge color={state.color === "gray" ? "gray" : state.color}>
              <StatusDot color={state.color} /> {state.label}
            </Badge>
          </div>
          {enabled && info?.error && <div className="mt-2 rounded-lg bg-red-soft px-3 py-2 text-[13px] text-red">{info.error}</div>}
        </Row>
        <Row title="Port" description="1024-65535. Başka bir uygulama kullanıyorsa değiştirin.">
          <Input
            value={portText ?? String(settings.api_port)}
            onChange={(e) => setPortText(e.target.value.replace(/\D/g, ""))}
            onBlur={savePort}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
            className="w-28 font-mono"
            aria-label="Port"
          />
        </Row>
        <Row title="Adres">
          <div className="flex items-center gap-1">
            <code className="selectable min-w-0 flex-1 truncate rounded-lg bg-surface-2 px-3 py-2 font-mono text-[13px]">{url}</code>
            <CopyButton text={url} label="Adresi kopyala" />
            <Button size="sm" variant="ghost" icon={<ExternalLink size={13} />} disabled={!info?.running} onClick={() => openUrl(`${url}/docs`).catch(() => {})}>
              Belgeler
            </Button>
          </div>
        </Row>
        <Row title="Erişim anahtarı" description="İsteklerde Authorization: Bearer başlığıyla gönderilir. Paylaşmayın.">
          <div className="flex items-center gap-1">
            <code className="selectable min-w-0 flex-1 truncate rounded-lg bg-surface-2 px-3 py-2 font-mono text-[13px]">
              {showToken ? token : "•".repeat(32)}
            </code>
            <IconToggle shown={showToken} onToggle={() => setShowToken(!showToken)} />
            <CopyButton text={token} label="Anahtarı kopyala" />
            <Button size="sm" variant="ghost" icon={<RefreshCw size={13} />} onClick={() => setConfirmRegenerate(true)}>
              Yenile
            </Button>
          </div>
        </Row>
      </Card>
      <ApiExample url={url} token={token} showToken={showToken} running={!!info?.running} />
      <ConfirmDialog
        open={confirmRegenerate}
        onClose={() => setConfirmRegenerate(false)}
        title="Erişim anahtarını yenile"
        confirmLabel="Yenile"
        destructive
        description="Yeni bir anahtar üretilir; eski anahtarı kullanan betik ve uygulamalar yeni anahtarla güncellenene kadar çalışmaz"
        onConfirm={async () => {
          try {
            const next = await api.regenerateApiToken();
            setInfo(next);
            useStore.getState().set({ settings: { ...settings, api_token: next.token } });
            notify("Yeni erişim anahtarı üretildi", "success");
          } catch (e) {
            notify(errorMessage(e), "error");
          }
        }}
      />
    </div>
  );
}

function IconToggle({ shown, onToggle }: { shown: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-label={shown ? "Anahtarı gizle" : "Anahtarı göster"}
      title={shown ? "Gizle" : "Göster"}
      className="inline-flex size-7.5 items-center justify-center rounded-md text-text-2 hover:bg-surface-2 hover:text-text"
    >
      {shown ? <EyeOff size={14} /> : <Eye size={14} />}
    </button>
  );
}

export function Settings() {
  const { settingsSection, set } = useStore();
  const [section, setSection] = useState<Section>(() => (SECTIONS.some((s) => s.id === settingsSection) ? (settingsSection as Section) : "general"));
  useEffect(() => {
    if (settingsSection && SECTIONS.some((s) => s.id === settingsSection)) setSection(settingsSection as Section);
    if (settingsSection) set({ settingsSection: null });
  }, [settingsSection, set]);
  const current = SECTIONS.find((s) => s.id === section)!;
  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="drag-region shrink-0 border-b border-border bg-surface px-7 pt-5 pb-4">
        <h1 className="text-[20px] leading-tight font-semibold">Ayarlar</h1>
        <p className="mt-0.5 text-[13px] text-muted">Tını'nın görünümü, ses motoru, sözlüğü ve yerel API'si</p>
      </header>
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Ayar bölümleri" className="w-52 shrink-0 space-y-px border-r border-border bg-surface p-3">
          {SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              onClick={() => setSection(s.id)}
              aria-current={section === s.id ? "page" : undefined}
              className={cx(
                "flex h-8 w-full items-center gap-2.5 rounded-md px-2.5 text-left text-[13px] transition",
                section === s.id ? "bg-surface-2 font-medium text-text" : "text-text-2 hover:bg-surface-2/70 hover:text-text",
              )}
            >
              <span className={section === s.id ? "text-text" : "text-muted"}>{s.icon}</span>
              {s.label}
            </button>
          ))}
        </nav>
        <div className="min-h-0 flex-1 overflow-y-auto px-7 py-6">
          <div className="max-w-3xl">
            <div className="mb-4">
              <h2 className="text-[15px] font-semibold">{current.label}</h2>
              <p className="mt-0.5 text-[13px] text-muted">{current.description}</p>
            </div>
            {section === "general" && <General />}
            {section === "engine" && <Engine />}
            {section === "model" && (
              <div className="space-y-4">
                <EngineEnvCard />
                <ModelCard />
              </div>
            )}
            {section === "dictionary" && <Dictionary />}
            {section === "api" && <ApiSection />}
            {section === "data" && <Data />}
            {section === "about" && <About />}
          </div>
        </div>
      </div>
    </div>
  );
}

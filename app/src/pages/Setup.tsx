// First run: system check, the engine environment (uv + Python + VoxCPM) and the VoxCPM2 model weights.
import { open } from "@tauri-apps/plugin-dialog";
import { AlertTriangle, Boxes, CheckCircle2, Cpu, Download, FolderOpen, HardDrive, Pause, RefreshCw } from "lucide-react";
import { api, errorMessage } from "../api";
import { useStore } from "../store";
import { formatBytes, formatDuration } from "../languages";
import { Button, Card, ProgressBar } from "../components/ui";
import { Logo } from "../components/Logo";

const ENV_STEPS = ["Python 3.12", "PyTorch ve bağımlılıklar", "VoxCPM", "Sınama"];

export function EngineEnvCard() {
  const { status, engineEnv, notify, set } = useStore();
  if (!status) return null;
  const installing = engineEnv?.stage === "installing";
  const engine = status.engine;
  const ready = engine.env_ready && !installing;
  const install = () => {
    set({ engineEnv: { stage: "installing", current: 0, total: 4, speed_mb_s: 0, message: "başlıyor" } });
    api.installEngineEnv().catch((e) => notify(errorMessage(e), "error"));
  };
  return (
    <Card className="p-5">
      <div className="mb-3 flex items-center gap-3">
        <Boxes size={18} className="text-muted" />
        <div className="flex-1">
          <div className="font-medium">Ses motoru</div>
          <div className="text-[13px] text-muted">
            Resmi VoxCPM (OpenBMB) · PyTorch, Apple GPU (MPS) · ~{formatBytes(engine.env_size)} · uygulamaya özel Python, sisteme dokunmaz
          </div>
        </div>
        {ready && <CheckCircle2 size={20} className="text-green" />}
      </div>
      {installing ? (
        <>
          <ProgressBar value={engineEnv!.total ? (engineEnv!.current + 0.5) / engineEnv!.total : 0} />
          <div className="mt-2 flex items-center justify-between gap-3 text-[12px] text-muted">
            <span className="min-w-0 truncate">
              {ENV_STEPS[engineEnv!.current] ?? "Kuruluyor"} ({engineEnv!.current + 1}/{engineEnv!.total}) · {engineEnv!.message}
            </span>
            <Button size="sm" variant="ghost" icon={<Pause size={13} />} onClick={() => api.cancelEngineEnvInstall()}>
              Durdur
            </Button>
          </div>
        </>
      ) : ready ? (
        <div className="flex items-center justify-between text-[13px] text-muted">
          <span>{engine.loaded ? "Hazır · model bellekte" : "Hazır · ilk işte belleğe yüklenir (~20 sn)"}</span>
          <Button size="sm" variant="ghost" icon={<RefreshCw size={13} />} onClick={install}>
            Onar
          </Button>
        </div>
      ) : (
        <>
          {engine.env === "outdated" && (
            <div className="mb-3 rounded-lg bg-amber-soft px-3 py-2 text-[13px] text-amber">
              Uygulama güncellendi; ses motorunun paketleri yeni sürüme göre yeniden kurulmalı.
            </div>
          )}
          {!engine.uv && (
            <div className="mb-3 rounded-lg bg-red-soft px-3 py-2 text-[13px] text-red">Kurulum aracı (uv) uygulama paketinde yok. Uygulamayı yeniden kurun.</div>
          )}
          <Button variant="primary" icon={<Download size={14} />} disabled={!engine.uv} onClick={install}>
            {engine.env === "outdated" ? "Ses motorunu güncelle" : "Ses motorunu kur"}
          </Button>
        </>
      )}
      {engineEnv?.stage === "error" && <pre className="selectable mt-2 max-h-40 overflow-auto text-[12px] whitespace-pre-wrap text-red">{engineEnv.message}</pre>}
    </Card>
  );
}

export function ModelCard() {
  const { status, model, notify, set } = useStore();
  if (!status) return null;
  const busy = model && (model.stage === "downloading" || model.stage === "verifying");
  const progress = model && model.total ? model.current / model.total : status.model.partial / status.model.expected;
  const remaining = model?.stage === "downloading" && model.speed_mb_s > 0 ? (model.total - model.current) / 1e6 / model.speed_mb_s : null;
  return (
    <Card className="p-5">
      <div className="mb-3 flex items-center gap-3">
        <HardDrive size={18} className="text-muted" />
        <div className="flex-1">
          <div className="font-medium">VoxCPM2 modeli</div>
          <div className="text-[13px] text-muted">
            {status.model.repo} · {formatBytes(status.model.expected)} · sabit sürüm ({status.model.commit.slice(0, 7)}), dosya başına sha256
          </div>
        </div>
        {status.model.ready && !busy && <CheckCircle2 size={20} className="text-green" />}
      </div>
      {busy ? (
        <>
          <ProgressBar value={progress} />
          <div className="mt-2 flex items-center justify-between text-[12px] text-muted">
            <span>
              {model!.stage === "verifying" ? "Doğrulanıyor" : "İndiriliyor"} · {formatBytes(model!.current)} / {formatBytes(model!.total)}
              {model!.stage === "downloading" && model!.speed_mb_s ? ` · ${model!.speed_mb_s} MB/sn` : ""}
              {remaining ? ` · ~${formatDuration(Math.round(remaining))} kaldı` : ""}
            </span>
            <Button size="sm" variant="ghost" icon={<Pause size={13} />} onClick={() => api.cancelModelDownload()}>
              Duraklat
            </Button>
          </div>
        </>
      ) : status.model.ready ? (
        <div className="text-[13px] break-all text-muted">{status.model.path}</div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            icon={<Download size={14} />}
            onClick={() => {
              set({ model: { stage: "downloading", current: status.model.partial, total: status.model.expected, speed_mb_s: 0, message: null } });
              api.downloadModel().catch((e) => notify(errorMessage(e), "error"));
            }}
          >
            {status.model.partial > 0 ? `İndirmeyi sürdür (%${Math.round(progress * 100)})` : "Modeli indir"}
          </Button>
          <Button
            icon={<FolderOpen size={14} />}
            onClick={async () => {
              const path = await open({ directory: true, title: "VoxCPM2 model klasörünü seçin (model.safetensors içeren)" });
              if (typeof path === "string") {
                set({ model: { stage: "verifying", current: 0, total: status.model.expected, speed_mb_s: 0, message: null } });
                api.useLocalModel(path).catch((e) => notify(errorMessage(e), "error"));
              }
            }}
          >
            Elimdeki klasörü kullan…
          </Button>
        </div>
      )}
      {model?.stage === "error" && <div className="mt-2 text-[13px] text-red">{model.message}</div>}
    </Card>
  );
}

export function Setup() {
  const { status } = useStore();
  if (!status) return null;
  const sys = status.system;
  return (
    <div className="drag-region flex h-full items-center justify-center overflow-y-auto bg-bg p-10">
      <div className="w-full max-w-xl space-y-5 [-webkit-app-region:no-drag]">
        <div className="mb-8 flex flex-col items-center text-center">
          <Logo size={72} />
          <h1 className="mt-5 text-[22px] leading-tight font-semibold">Tını'yı kurun</h1>
          <p className="mt-2 max-w-md text-[14px] leading-relaxed text-text-2">
            Tını sesleri bu bilgisayarda tasarlar, klonlar ve okur. Başlamadan önce ses motorunu ve VoxCPM2 modelini kurun; ikisi de bir kez indirilir.
          </p>
        </div>
        <Card className="p-5">
          <div className="mb-3 flex items-center gap-3">
            <Cpu size={18} className="text-muted" />
            <div className="font-medium">Bu bilgisayar</div>
          </div>
          <div className="grid grid-cols-2 gap-y-1.5 text-[13px]">
            <span className="text-muted">İşlemci / GPU</span>
            <span>{sys.gpu ?? sys.arch}</span>
            <span className="text-muted">Bellek</span>
            <span>{sys.ram_gb ? `${sys.ram_gb} GB` : "?"}</span>
            <span className="text-muted">Motor</span>
            <span>VoxCPM · PyTorch MPS</span>
          </div>
          {sys.warnings.map((w) => (
            <div key={w} className="mt-3 flex gap-2 rounded-lg bg-amber-soft px-3 py-2 text-[13px] text-amber">
              <AlertTriangle size={14} className="mt-0.5 shrink-0" /> {w}
            </div>
          ))}
        </Card>
        <EngineEnvCard />
        <ModelCard />
        <div className="px-1 text-[12px] leading-relaxed text-muted">
                    <div>
            Tını ile üretilen sesler yapaydır. Klonlamayı yalnız kendi sesinizle ya da sahibinin açık izniyle kullanın; yayınladığınız sesi yapay zekâ
            ürünü olarak etiketleyin. İzinsiz taklit, dolandırıcılık ve yanıltma yasaktır.
          </div>
        </div>
      </div>
    </div>
  );
}

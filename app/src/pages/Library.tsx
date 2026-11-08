// Voice library: built-in, designed and cloned voices; favorites, aliases, trash, details.
import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { ArchiveRestore, FolderInput, FolderOpen, Heart, History, Library as LibraryIcon, Mic, Mic2, MoreHorizontal, Pencil, PencilRuler, Plus, Search, Share, Trash2, Undo2, X } from "lucide-react";
import { api, errorMessage, type TrashEntry, type VoiceCard } from "../api";
import { useStore } from "../store";
import { LANGUAGES, formatDate, formatDuration, languageName, voiceName } from "../languages";
import { AudioPlayer } from "../components/AudioPlayer";
import { Select } from "../components/Select";
import {
  Badge,
  Button,
  Card,
  EmptyState,
  FilterTabs,
  Flag,
  IconButton,
  Input,
  Label,
  MenuDivider,
  MenuItem,
  PageHeader,
  Popover,
  PromptDialog,
  cx,
} from "../components/ui";
import { GENDER_OPTIONS } from "./Design";

type Filter = "all" | "favorite" | "builtin" | "designed" | "clone";

function VoiceMenu({ voice, onRename }: { voice: VoiceCard; onRename: () => void }) {
  const { refreshVoices, notify, set, navigate } = useStore();
  const manage = async (action: "rename" | "trash", name?: string) => {
    try {
      await api.manageVoice(voice.id, action, name);
      await refreshVoices();
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };
  return (
    <Popover
      align="right"
      width={230}
      trigger={(toggle) => (
        <IconButton
          label="Diğer"
          onClick={(e) => {
            e.stopPropagation();
            toggle();
          }}
        >
          <MoreHorizontal size={15} />
        </IconButton>
      )}
    >
      {(close) => (
        <div className="py-1" onClick={(e) => e.stopPropagation()}>
          <MenuItem
            icon={<Mic2 size={14} />}
            onClick={() => {
              close();
              set({ studioVoice: voice.id, studioResult: null });
              navigate("studio");
            }}
          >
            Bu sesle oku
          </MenuItem>
          <MenuItem
            icon={<Pencil size={14} />}
            onClick={() => {
              close();
              onRename();
            }}
          >
            Yeniden adlandır…
          </MenuItem>
          {voice.alias && (
            <MenuItem
              icon={<Undo2 size={14} />}
              onClick={() => {
                close();
                manage("rename", "");
              }}
            >
              Asıl ada dön
            </MenuItem>
          )}
          {voice.job_id && (
            <MenuItem
              icon={<History size={14} />}
              onClick={() => {
                close();
                if (voice.source === "designed") {
                  set({ designResult: voice.job_id });
                  navigate("design");
                } else {
                  set({ selectedJob: voice.job_id });
                  navigate("jobs");
                }
              }}
            >
              {voice.source === "designed" ? "Tasarımına git" : "Üretimine git"}
            </MenuItem>
          )}
          <MenuItem
            icon={<FolderOpen size={14} />}
            onClick={() => {
              close();
              revealItemInDir(voice.reference);
            }}
          >
            Klasörde göster
          </MenuItem>
          <MenuItem
            icon={<Share size={14} />}
            onClick={async () => {
              close();
              const target = await open({ directory: true, title: "Sesin kopyalanacağı klasör" });
              if (typeof target === "string") {
                try {
                  const path = await api.exportVoice(voice.id, target);
                  notify("Ses klasörü dışa aktarıldı", "success", { label: "Göster", run: () => revealItemInDir(path) });
                } catch (e) {
                  notify(errorMessage(e), "error");
                }
              }
            }}
          >
            Dışa aktar (klasör)…
          </MenuItem>
          {voice.source !== "builtin" && (
            <>
              <MenuDivider />
              <MenuItem
                danger
                icon={<Trash2 size={14} />}
                onClick={() => {
                  close();
                  manage("trash").then(() => notify("Ses çöpe taşındı; kitaplığın altından geri getirebilirsiniz"));
                }}
              >
                Çöpe taşı
              </MenuItem>
            </>
          )}
        </div>
      )}
    </Popover>
  );
}

function VoiceTile({ voice, selected, onSelect, onRename }: { voice: VoiceCard; selected: boolean; onSelect: () => void; onRename: () => void }) {
  const { refreshVoices, set, navigate } = useStore();
  const tags =
    voice.source === "builtin" ? ["hazır ses"] : voice.source === "clone" ? [] : [voice.variant ? `çeşit ${voice.variant}` : "", formatDate(voice.created_at)];
  return (
    <Card
      onClick={onSelect}
      className={cx("group flex flex-col transition", selected ? "border-accent/60 ring-3 ring-accent/15" : "hover:border-border-strong")}
    >
      <div className="flex items-start gap-3 px-4 pt-4">
        <Flag language={voice.language} size={15} className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-medium">{voiceName(voice)}</div>
          <div className="truncate text-[12px] text-muted">{voice.alias ? voice.display_name : languageName(voice.language)}</div>
        </div>
        <button
          onClick={async (e) => {
            e.stopPropagation();
            await api.manageVoice(voice.id, voice.favorite ? "unfavorite" : "favorite");
            refreshVoices();
          }}
          className={cx("flex size-7 items-center justify-center rounded-full hover:bg-surface-2", voice.favorite ? "text-accent" : "text-muted")}
          title={voice.favorite ? "Favorilerden çıkar" : "Favorilere ekle"}
          aria-label={voice.favorite ? "Favorilerden çıkar" : "Favorilere ekle"}
          aria-pressed={voice.favorite}
        >
          <Heart size={15} fill={voice.favorite ? "currentColor" : "none"} />
        </button>
      </div>
      <div className="flex flex-wrap gap-1 px-4 pt-2">
        {tags.filter(Boolean).map((t) => (
          <Badge key={t}>{t}</Badge>
        ))}
        {voice.source === "clone" && <Badge>izinli klon</Badge>}
        {voice.noisy && <Badge color="amber">gürültülü</Badge>}
      </div>
      <div className="px-4 py-3" onClick={(e) => e.stopPropagation()}>
        <AudioPlayer path={voice.preview ?? voice.reference} compact label={voiceName(voice)} />
      </div>
      <div className="mt-auto flex items-center gap-1 border-t border-border px-3 py-2">
        <Button
          size="sm"
          variant="ghost"
          icon={<Mic2 size={13} />}
          onClick={(e) => {
            e.stopPropagation();
            set({ studioVoice: voice.id, studioResult: null });
            navigate("studio");
          }}
        >
          Oku
        </Button>
        <div className="flex-1" />
        <VoiceMenu voice={voice} onRename={onRename} />
      </div>
    </Card>
  );
}

function VoiceDetails({ voice, onClose }: { voice: VoiceCard; onClose: () => void }) {
  const rows: [string, React.ReactNode][] = [
    ["Kimlik", <code className="font-mono text-[12px]">{voice.id}</code>],
    ["Kaynak", voice.source === "builtin" ? "Uygulamayla gelen hazır ses" : voice.source === "clone" ? "İzinli klon" : "Tasarlandı"],
    ["Dil", languageName(voice.language)],
    ["Okuma tohumu", <code className="font-mono">{voice.read_seed}</code>],
    ["Tasarım tohumu", voice.design_seed ? <code className="font-mono">{voice.design_seed}</code> : "–"],
    ["Referans", formatDuration(voice.reference_s)],
    [
      "Gürültü payı",
      voice.noise_margin_db == null ? (
        "–"
      ) : (
        <span className={voice.noisy ? "text-amber" : undefined}>
          {Math.round(voice.noise_margin_db)} dB · {voice.noisy ? "arka plan gürültüsü var, okumalara geçer" : "temiz"}
        </span>
      ),
    ],
    ["Tarih", voice.created_at ? formatDate(voice.created_at) : "–"],
  ];
  return (
    <aside className="fade-in flex w-[380px] shrink-0 flex-col border-l border-border bg-surface">
      <div className="flex items-start gap-3 border-b border-border px-5 py-4">
        <Flag language={voice.language} size={16} className="mt-1" />
        <div className="min-w-0 flex-1">
          <div className="text-[16px] font-semibold">{voiceName(voice)}</div>
          {voice.alias && <div className="text-[12px] text-muted">{voice.display_name}</div>}
        </div>
        <IconButton label="Kapat" onClick={onClose}>
          <X size={16} />
        </IconButton>
      </div>
      <div className="selectable min-h-0 flex-1 space-y-5 overflow-y-auto px-5 py-4">
        <div>
          <Label>Referans kaydı</Label>
          <AudioPlayer path={voice.reference} compact />
        </div>
        {voice.preview && (
          <div>
            <Label>Örnek okuma</Label>
            <AudioPlayer path={voice.preview} compact />
            {voice.preview_text && <p className="mt-1.5 text-[12px] text-muted">“{voice.preview_text}”</p>}
          </div>
        )}
        <dl className="grid grid-cols-[120px_1fr] gap-y-2 text-[13px]">
          {rows.map(([k, v]) => (
            <div key={k} className="contents">
              <dt className="text-muted">{k}</dt>
              <dd>{v}</dd>
            </div>
          ))}
        </dl>
        {voice.description && (
          <div>
            <Label>Ses tarifi</Label>
            <p className="rounded-lg bg-surface-2 p-3 text-[13px] leading-relaxed">{voice.description}</p>
          </div>
        )}
        <div>
          <Label>Referans metni</Label>
          <p className="rounded-lg bg-surface-2 p-3 text-[13px] leading-relaxed">{voice.design_text}</p>
        </div>
        {voice.warning && <p className="text-[12px] text-amber">{voice.warning}</p>}
        <p className="text-[12px] text-muted">
          Bu ses yapaydır. Yayınlarken yapay zekâ ile üretildiğini belirtin. Ses klasörü ({voice.folder.split(/[\\/]/).slice(-2).join("/")}) referans
          projeyle uyumludur.
        </p>
      </div>
    </aside>
  );
}

export function Library() {
  const { voices, selectedVoice, set, refreshVoices, notify, revision, navigate } = useStore();
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [language, setLanguage] = useState("all");
  const [gender, setGender] = useState("all");
  const [renaming, setRenaming] = useState<VoiceCard | null>(null);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trash, setTrash] = useState<TrashEntry[]>([]);

  useEffect(() => {
    api
      .listTrash()
      .then(setTrash)
      .catch(() => {});
  }, [voices, revision]);

  const list = useMemo(() => {
    const q = search.trim().toLocaleLowerCase("tr");
    return voices
      .filter(
        (v) =>
          (filter === "all" || (filter === "favorite" ? v.favorite : v.source === filter)) &&
          (language === "all" || v.language === language) &&
          (gender === "all" || v.gender === gender) &&
          (!q || [voiceName(v), v.display_name, v.description].join(" ").toLocaleLowerCase("tr").includes(q)),
      )
      .sort(
        (a, b) =>
          Number(b.favorite) - Number(a.favorite) ||
          Number(b.source === "builtin") - Number(a.source === "builtin") ||
          Number(b.language === "tr") - Number(a.language === "tr") ||
          voiceName(a).localeCompare(voiceName(b), "tr"),
      );
  }, [voices, filter, language, gender, search]);
  const selected = voices.find((v) => v.id === selectedVoice) ?? null;
  const counts: Record<Filter, number> = {
    all: voices.length,
    favorite: voices.filter((v) => v.favorite).length,
    builtin: voices.filter((v) => v.source === "builtin").length,
    designed: voices.filter((v) => v.source === "designed").length,
    clone: voices.filter((v) => v.source === "clone").length,
  };
  const languages = [...new Set(voices.map((v) => v.language))];
  const filtered = filter !== "all" || language !== "all" || gender !== "all" || search.trim() !== "";
  const resetFilters = () => {
    setFilter("all");
    setLanguage("all");
    setGender("all");
    setSearch("");
  };

  const importVoice = async () => {
    const dir = await open({ directory: true, title: "voice.json ve referans kaydı olan klasör" });
    if (typeof dir !== "string") return;
    try {
      const id = await api.importVoice(dir);
      await refreshVoices();
      set({ selectedVoice: id });
      notify("Ses içe aktarıldı", "success");
    } catch (e) {
      notify(errorMessage(e), "error");
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Kitaplık"
        description={`${voices.length} ses · okumalarda kullanabileceğiniz tasarlanmış, klonlanmış ve hazır sesler`}
        actions={
          <>
            <div className="relative">
              <Search size={14} className="pointer-events-none absolute top-2.5 left-3 text-muted" />
              <Input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Ses ara…" className="w-52 pl-8" aria-label="Ses ara" />
            </div>
            <Button icon={<FolderInput size={15} />} onClick={importVoice}>
              İçe aktar
            </Button>
            <Popover
              align="right"
              width={250}
              trigger={(toggle) => (
                <Button variant="primary" icon={<Plus size={15} />} onClick={toggle}>
                  Yeni ses
                </Button>
              )}
            >
              {(close) => (
                <div className="py-1">
                  <MenuItem
                    icon={<PencilRuler size={14} />}
                    description="İngilizce tarif edin, çeşitlerden seçin"
                    onClick={() => {
                      close();
                      navigate("design");
                    }}
                  >
                    Ses tasarla
                  </MenuItem>
                  <MenuItem
                    icon={<Mic size={14} />}
                    description="Kendi sesiniz ya da izinli bir kayıt"
                    onClick={() => {
                      close();
                      navigate("clone");
                    }}
                  >
                    Ses klonla
                  </MenuItem>
                </div>
              )}
            </Popover>
          </>
        }
      >
        <div className="flex flex-wrap items-center gap-2">
          <FilterTabs
            value={filter}
            onChange={setFilter}
            options={[
              { value: "all", label: "Tümü", count: counts.all },
              { value: "favorite", label: "Favoriler", count: counts.favorite },
              { value: "designed", label: "Tasarladıklarım", count: counts.designed },
              { value: "clone", label: "Klonlar", count: counts.clone },
              ...(counts.builtin ? [{ value: "builtin" as Filter, label: "Hazır", count: counts.builtin }] : []),
            ]}
          />
          <div className="flex-1" />
          <Select
            value={language}
            onChange={setLanguage}
            size="sm"
            aria-label="Dil"
            options={[
              { value: "all", label: "Bütün diller" },
              ...LANGUAGES.filter((l) => languages.includes(l.code)).map((l) => ({ value: l.code, label: l.name, icon: <Flag language={l.code} size={10} /> })),
            ]}
            className="w-40"
          />
          <Select value={gender} onChange={setGender} options={GENDER_OPTIONS} size="sm" className="w-40" aria-label="Cinsiyet" />
        </div>
      </PageHeader>
      <div className="flex min-h-0 flex-1">
        <div className="min-h-0 min-w-0 flex-1 overflow-y-auto px-7 py-5">
          {list.length ? (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(250px,1fr))] gap-4">
              {list.map((v) => (
                <VoiceTile
                  key={v.id}
                  voice={v}
                  selected={v.id === selectedVoice}
                  onSelect={() => set({ selectedVoice: v.id === selectedVoice ? null : v.id })}
                  onRename={() => setRenaming(v)}
                />
              ))}
            </div>
          ) : voices.length && filtered ? (
            <EmptyState
              icon={<Search size={22} />}
              title="Eşleşen ses yok"
              description="Arama ya da süzgeçlerle eşleşen bir ses bulunamadı"
              action={<Button onClick={resetFilters}>Süzgeçleri temizle</Button>}
            />
          ) : (
            <EmptyState
              icon={<LibraryIcon size={22} />}
              title="Kitaplık boş"
              description="Okumalarda kullanacağınız sesler burada durur. Bir ses tasarlayıp beğendiğiniz çeşidi ekleyin ya da kendi sesinizi klonlayın."
              action={
                <>
                  <Button variant="primary" icon={<PencilRuler size={15} />} onClick={() => navigate("design")}>
                    Ses tasarla
                  </Button>
                  <Button icon={<Mic size={15} />} onClick={() => navigate("clone")}>
                    Ses klonla
                  </Button>
                </>
              }
            />
          )}
          {trash.length > 0 && (
            <div className="mt-8">
              <button
                type="button"
                aria-expanded={trashOpen}
                onClick={() => setTrashOpen(!trashOpen)}
                className="flex items-center gap-1.5 text-[13px] font-medium text-muted hover:text-text"
              >
                <Trash2 size={13} /> Silinen sesler ({trash.length}) <span className="text-[11px]">{trashOpen ? "▲" : "▼"}</span>
              </button>
              {trashOpen && (
                <div className="mt-3 divide-y divide-border rounded-xl border border-border bg-surface">
                  {trash.map((t) => (
                    <div key={t.trash_id} className="flex items-center gap-3 px-4 py-2.5">
                      <span className="min-w-0 flex-1 truncate">
                        {t.display_name} <span className="text-muted">({t.voice_id})</span>
                      </span>
                      <span className="text-[12px] text-muted">{formatDate(t.deleted_at)}</span>
                      <Button
                        size="sm"
                        icon={<ArchiveRestore size={13} />}
                        onClick={async () => {
                          try {
                            await api.manageVoice(t.trash_id, "restore");
                            refreshVoices();
                          } catch (e) {
                            notify(errorMessage(e), "error");
                          }
                        }}
                      >
                        Geri getir
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>
        {selected && <VoiceDetails voice={selected} onClose={() => set({ selectedVoice: null })} />}
      </div>
      <PromptDialog
        open={!!renaming}
        onClose={() => setRenaming(null)}
        title="Yeniden adlandır"
        label="Sesin adı"
        allowEmpty
        initialValue={renaming ? voiceName(renaming) : ""}
        hint={`Boş bırakırsanız asıl adı (${renaming?.display_name ?? ""}) kullanılır. Dosyalara dokunulmaz.`}
        onSubmit={async (name) => {
          await api.manageVoice(renaming!.id, "rename", name);
          await refreshVoices();
        }}
      />
    </div>
  );
}

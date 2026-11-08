// Voice picker: search, favorites on top; each row has a preview and a favorite toggle.
import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronsUpDown, Heart, Pause, Play, Search } from "lucide-react";
import { api, fileUrl, type VoiceCard } from "../api";
import { useStore } from "../store";
import { languageName, voiceName } from "../languages";
import { Flag, Popover, cx } from "./ui";

let previewAudio: HTMLAudioElement | null = null;

function playPreview(path: string, onEnd?: () => void) {
  previewAudio?.pause();
  previewAudio = new Audio(fileUrl(path));
  previewAudio.onended = () => onEnd?.();
  previewAudio.play().catch(() => onEnd?.());
}

function stopPreview() {
  previewAudio?.pause();
  previewAudio = null;
}

function VoiceRow({
  v,
  selected,
  highlighted,
  onSelect,
  playing,
  onPlay,
}: {
  v: VoiceCard;
  selected: boolean;
  highlighted: boolean;
  onSelect: () => void;
  playing: boolean;
  onPlay: () => void;
}) {
  const refreshVoices = useStore((s) => s.refreshVoices);
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (highlighted) row.current?.scrollIntoView({ block: "nearest" });
  }, [highlighted]);
  const subtitle = [v.source === "builtin" ? "Hazır ses" : v.source === "clone" ? "İzinli klon" : `${v.name} · çeşit ${v.variant}`, v.alias ? v.display_name : null].filter(Boolean).join(" · ");
  return (
    <div
      ref={row}
      onClick={onSelect}
      className={cx("group flex items-center gap-2.5 px-3 py-2", highlighted ? "bg-surface-2" : "hover:bg-surface-2")}
    >
      <Flag language={v.language} size={13} />
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium">{voiceName(v)}</div>
        {subtitle && <div className="truncate text-[12px] text-muted">{subtitle}</div>}
      </div>
      {(v.preview || v.reference) && (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onPlay();
          }}
          className="flex size-6 items-center justify-center rounded-full text-text-2 opacity-70 hover:bg-surface-3 hover:opacity-100"
          title="Dinle"
          aria-label={`${voiceName(v)} dinle`}
        >
          {playing ? <Pause size={12} fill="currentColor" /> : <Play size={12} fill="currentColor" />}
        </button>
      )}
      <button
        onClick={async (e) => {
          e.stopPropagation();
          await api.manageVoice(v.id, v.favorite ? "unfavorite" : "favorite");
          refreshVoices();
        }}
        className={cx("flex size-6 items-center justify-center rounded-full hover:bg-surface-3", v.favorite ? "text-accent" : "text-muted opacity-0 group-hover:opacity-100")}
        title={v.favorite ? "Favorilerden çıkar" : "Favorilere ekle"}
      >
        <Heart size={13} fill={v.favorite ? "currentColor" : "none"} />
      </button>
      <Check size={14} aria-hidden className={cx("shrink-0 text-text", selected ? "opacity-100" : "opacity-0")} />
    </div>
  );
}

export function VoicePicker({ value, onChange, wide }: { value: string | null; onChange: (id: string) => void; wide?: boolean }) {
  const voices = useStore((s) => s.voices);
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const [playing, setPlaying] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const selected = voices.find((v) => v.id === value);

  const groups = useMemo(() => {
    const q = query.trim().toLocaleLowerCase("tr");
    const matches = voices.filter(
      (v) => !q || [voiceName(v), v.display_name, v.id, languageName(v.language)].join(" ").toLocaleLowerCase("tr").includes(q),
    );
    return [
      { title: "Favoriler", voices: matches.filter((v) => v.favorite) },
      { title: "Hazır sesler", voices: matches.filter((v) => !v.favorite && v.source === "builtin") },
      { title: "Tasarladıklarım", voices: matches.filter((v) => !v.favorite && v.source === "designed") },
      { title: "Klonlar", voices: matches.filter((v) => !v.favorite && v.source === "clone") },
    ].filter((g) => g.voices.length);
  }, [voices, query]);
  const flat = groups.flatMap((g) => g.voices);

  const select = (id: string, close: () => void) => {
    onChange(id);
    stopPreview();
    setPlaying(null);
    close();
  };

  return (
    <Popover
      width={360}
      className={wide ? "w-full" : undefined}
      open={open}
      onOpenChange={(v) => {
        setOpen(v);
        if (!v) {
          stopPreview();
          setPlaying(null);
        } else {
          setQuery("");
          setHighlight(Math.max(0, flat.findIndex((x) => x.id === value)));
        }
      }}
      trigger={(toggle, isOpen) => (
        <button
          type="button"
          onClick={toggle}
          aria-haspopup="dialog"
          aria-expanded={isOpen}
          aria-label={selected ? `Ses: ${voiceName(selected)}` : "Ses seç"}
          className={cx(
            "flex h-9 items-center gap-2.5 rounded-lg border bg-surface px-3 text-left shadow-xs transition hover:border-border-strong",
            isOpen ? "border-accent/60 ring-3 ring-accent/15" : "border-border",
            wide ? "w-full" : "min-w-[240px] max-w-[340px]",
          )}
        >
          {selected ? (
            <>
              <Flag language={selected.language} size={13} />
              <span className="min-w-0 flex-1 truncate font-medium">{voiceName(selected)}</span>
            </>
          ) : (
            <span className="flex-1 text-muted">Ses seç…</span>
          )}
          <ChevronsUpDown size={14} className="shrink-0 text-muted" />
        </button>
      )}
    >
      {(close) => (
        <div
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setHighlight((i) => Math.min(flat.length - 1, i + 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setHighlight((i) => Math.max(0, i - 1));
            } else if (e.key === "Enter" && flat[highlight]) {
              e.preventDefault();
              select(flat[highlight].id, close);
            }
          }}
        >
          <div className="flex items-center gap-2 border-b border-border px-3">
            <Search size={14} className="text-muted" />
            <input
              autoFocus
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setHighlight(0);
              }}
              placeholder="Ses ara (ad, dil, özellik)…"
              className="h-10 flex-1 bg-transparent text-[13px] outline-none placeholder:text-muted"
            />
          </div>
          <div className="max-h-[380px] overflow-y-auto py-1">
            {groups.map((g) => (
              <div key={g.title}>
                <div className="px-3 pt-2.5 pb-1 text-[12px] font-medium text-muted">{g.title}</div>
                {g.voices.map((v) => (
                  <VoiceRow
                    key={v.id}
                    v={v}
                    selected={v.id === value}
                    highlighted={flat[highlight]?.id === v.id}
                    playing={playing === v.id}
                    onSelect={() => select(v.id, close)}
                    onPlay={() => {
                      if (playing === v.id) {
                        stopPreview();
                        setPlaying(null);
                      } else {
                        playPreview(v.preview ?? v.reference, () => setPlaying(null));
                        setPlaying(v.id);
                      }
                    }}
                  />
                ))}
              </div>
            ))}
            {!flat.length && <div className="px-4 py-6 text-center text-muted">Eşleşen ses yok</div>}
          </div>
        </div>
      )}
    </Popover>
  );
}

// Waveform audio player. Only one player plays at a time.
import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import { Pause, Play, RotateCcw } from "lucide-react";
import { fileUrl } from "../api";
import { formatClock } from "../languages";
import { cx } from "./ui";

let current: WaveSurfer | null = null;

function cssColor(name: string, fallback: string) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}

export interface AudioPlayerHandle {
  play: (t?: number) => void;
}

interface Props {
  path: string | null | undefined;
  version?: string | number;
  compact?: boolean;
  onTime?: (t: number) => void;
  className?: string;
  label?: string;
}

export const AudioPlayer = forwardRef<AudioPlayerHandle, Props>(function AudioPlayer({ path, version, compact, onTime, className, label }, ref) {
  const container = useRef<HTMLDivElement>(null);
  const ws = useRef<WaveSurfer | null>(null);
  const [ready, setReady] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [rate, setRate] = useState(1);
  const [error, setError] = useState<string | null>(null);
  const onTimeRef = useRef(onTime);
  onTimeRef.current = onTime;

  useEffect(() => {
    if (!container.current || !path) return;
    setReady(false);
    setError(null);
    const w = WaveSurfer.create({
      container: container.current,
      height: compact ? 34 : 72,
      waveColor: cssColor("--border-strong", "#ccc"),
      progressColor: cssColor("--accent", "#e2582f"),
      cursorColor: cssColor("--accent", "#e2582f"),
      cursorWidth: compact ? 0 : 2,
      barWidth: 2,
      barGap: compact ? 2 : 1.5,
      barRadius: 2,
      normalize: true,
      url: fileUrl(path, version),
      dragToSeek: true,
    });
    ws.current = w;
    w.on("ready", (d) => {
      setReady(true);
      setDuration(d);
    });
    w.on("error", (e) => setError(String(e)));
    w.on("timeupdate", (t) => {
      setTime(t);
      onTimeRef.current?.(t);
    });
    w.on("play", () => {
      if (current && current !== w) current.pause();
      current = w;
      setPlaying(true);
    });
    w.on("pause", () => setPlaying(false));
    w.on("finish", () => setPlaying(false));
    return () => {
      if (current === w) current = null;
      w.destroy();
      ws.current = null;
    };
  }, [path, version, compact]);

  useImperativeHandle(ref, () => ({
    play: (t?: number) => {
      const w = ws.current;
      if (!w) return;
      if (t !== undefined) w.setTime(t);
      w.play();
    },
  }));

  const cycleRate = () => {
    const rates = [1, 1.25, 1.5, 0.75];
    const next = rates[(rates.indexOf(rate) + 1) % rates.length];
    setRate(next);
    ws.current?.setPlaybackRate(next, true);
  };

  if (!path) return null;

  return (
    <div className={cx("flex items-center gap-3", className)}>
      <button
        onClick={() => ws.current?.playPause()}
        disabled={!ready}
        aria-label={playing ? "Duraklat" : label ? `${label} çal` : "Çal"}
        title={playing ? "Duraklat" : "Çal"}
        className={cx(
          "flex shrink-0 items-center justify-center rounded-full bg-accent-fill text-on-accent transition-[filter,transform] hover:brightness-105 active:scale-[0.97] disabled:opacity-40",
          compact ? "size-8" : "size-10",
        )}
      >
        {playing ? <Pause size={compact ? 14 : 17} fill="currentColor" /> : <Play size={compact ? 14 : 17} fill="currentColor" className="ml-0.5" />}
      </button>
      <div className="min-w-0 flex-1">
        {error ? <div className="text-[12px] text-red">Ses açılamadı</div> : <div ref={container} className={cx(!ready && "opacity-60")} />}
      </div>
      {!compact && (
        <div className="flex shrink-0 items-center gap-1.5">
          <span className="w-[86px] text-right font-mono text-[12px] text-muted tabular-nums">
            {formatClock(time)} / {formatClock(duration)}
          </span>
          <button onClick={cycleRate} className="h-6 rounded-md px-1.5 font-mono text-[11px] text-text-2 hover:bg-surface-2" title="Çalma hızı (önizleme)">
            {rate}×
          </button>
          <button onClick={() => ws.current?.setTime(0)} className="flex size-6 items-center justify-center rounded-md text-muted hover:bg-surface-2" title="Başa sar">
            <RotateCcw size={13} />
          </button>
        </div>
      )}
      {compact && <span className="w-9 shrink-0 text-right font-mono text-[11px] text-muted tabular-nums">{formatClock(playing ? time : duration)}</span>}
    </div>
  );
});

// Compact list of earlier jobs (readings, clones, transcripts…) used in result panels.
import type { ReactNode } from "react";
import type { Job } from "../api";
import { formatDate, formatDuration } from "../languages";
import { JOB_STATUS, jobStatusLabel } from "./JobProgress";
import { Badge, cx } from "./ui";

export function HistoryList({
  jobs,
  selected,
  onSelect,
  meta,
  empty,
}: {
  jobs: Job[];
  selected?: string | null;
  onSelect: (id: string) => void;
  /** Second line; defaults to date · audio duration. */
  meta?: (job: Job) => ReactNode;
  empty?: ReactNode;
}) {
  if (!jobs.length) return <>{empty ?? null}</>;
  return (
    <div className="space-y-0.5">
      {jobs.map((j) => (
        <button
          key={j.id}
          type="button"
          onClick={() => onSelect(j.id)}
          aria-current={j.id === selected ? "true" : undefined}
          className={cx("flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition", j.id === selected ? "bg-surface-2 shadow-[inset_0_0_0_1px_var(--border)]" : "hover:bg-surface-2/60")}
        >
          <div className="min-w-0 flex-1">
            <div className="truncate font-medium">{j.title || "Adsız"}</div>
            <div className="truncate text-[12px] text-muted">
              {meta ? meta(j) : [formatDate(j.finished_at ?? j.created_at), j.audio_s ? formatDuration(j.audio_s) : null].filter(Boolean).join(" · ")}
            </div>
          </div>
          {j.status !== "done" && <Badge color={JOB_STATUS[j.status].color}>{jobStatusLabel(j)}</Badge>}
        </button>
      ))}
    </div>
  );
}

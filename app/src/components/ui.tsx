// Shared UI primitives: buttons, fields, badges, cards, page frame, dialogs, menus.
// The custom listbox lives in Select.tsx.
import { useEffect, useId, useRef, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { AlertCircle, Check, ChevronRight, Copy, Loader2, Minus, Plus, X } from "lucide-react";
import { flagUrl, languageName } from "../languages";

export function cx(...parts: (string | false | null | undefined)[]) {
  return parts.filter(Boolean).join(" ");
}

export const modKey = "⌘";

// ---------------------------------------------------------------------------------------------------------------
// Buttons

type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "destructive" | "soft";

const BUTTON_VARIANTS: Record<ButtonVariant, string> = {
  // Disabled primaries turn neutral instead of a pale accent block (most primaries stay enabled and explain on click).
  // Ink on light, cream on dark (the brand's letter colors). Coral is kept for sound: playback, the spoken word, progress.
  primary:
    "bg-primary text-on-primary shadow-[0_1px_2px_rgb(0_0_0/0.12),inset_0_1px_0_rgb(255_255_255/0.08)] hover:bg-primary-hover active:scale-[0.98] disabled:bg-surface-3 disabled:text-muted disabled:shadow-none",
  secondary: "bg-surface border border-border text-text shadow-xs hover:bg-surface-2 hover:border-border-strong active:bg-surface-3 disabled:text-muted",
  ghost: "text-text-2 hover:bg-surface-2 hover:text-text active:bg-surface-3 disabled:text-muted/70",
  danger: "bg-red-soft text-red hover:brightness-95 active:scale-[0.98] disabled:opacity-50",
  destructive: "bg-red text-white shadow-sm hover:brightness-105 active:brightness-95 disabled:opacity-50",
  soft: "bg-surface-2 text-text hover:bg-surface-3 active:scale-[0.98] disabled:opacity-50",
};

const BUTTON_SIZES = {
  sm: "h-7 px-2.5 text-[13px] gap-1.5 rounded-md",
  md: "h-9 px-3.5 gap-2 rounded-lg",
  lg: "h-10 px-4 text-[14px] gap-2 rounded-lg",
};

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  loading,
  className,
  children,
  ...p
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant; size?: keyof typeof BUTTON_SIZES; icon?: ReactNode; loading?: boolean }) {
  return (
    <button
      type="button"
      {...p}
      aria-busy={loading || undefined}
      className={cx(
        "inline-flex shrink-0 items-center justify-center font-medium whitespace-nowrap transition-[background-color,border-color,color,opacity,transform,filter] duration-150 ease-[var(--ease)] select-none disabled:pointer-events-none",
        BUTTON_VARIANTS[variant],
        BUTTON_SIZES[size],
        loading && "pointer-events-none",
        className,
      )}
    >
      {loading ? <Loader2 size={size === "lg" ? 16 : 14} className="animate-spin" /> : icon}
      {children}
    </button>
  );
}

export function IconButton({ label, className, children, ...p }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      {...p}
      title={p.title ?? label}
      aria-label={label}
      className={cx(
        "inline-flex size-8 shrink-0 items-center justify-center rounded-md text-text-2 transition-colors hover:bg-surface-2 hover:text-text active:bg-surface-3 disabled:pointer-events-none disabled:opacity-35",
        className,
      )}
    >
      {children}
    </button>
  );
}

/** Copies `text` to the clipboard; the icon turns into a check for a moment. */
export function CopyButton({ text, label = "Kopyala", className }: { text: string; label?: string; className?: string }) {
  const [done, setDone] = useState(false);
  return (
    <IconButton
      label={done ? "Kopyalandı" : label}
      className={className}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(text);
          setDone(true);
          setTimeout(() => setDone(false), 1400);
        } catch {
          /* clipboard unavailable */
        }
      }}
    >
      {done ? <Check size={14} className="text-green" /> : <Copy size={14} />}
    </IconButton>
  );
}

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return <kbd className={cx("rounded border border-current/20 px-1 py-px font-mono text-[11px]", className)}>{children}</kbd>;
}

// ---------------------------------------------------------------------------------------------------------------
// Fields

export function Label({ children, extra, className, htmlFor }: { children: ReactNode; extra?: ReactNode; className?: string; htmlFor?: string }) {
  return (
    <div className={cx("mb-1.5 flex items-baseline justify-between gap-3 text-[13px] font-medium text-text-2", className)}>
      <label htmlFor={htmlFor}>{children}</label>
      {extra && <span className="text-[12px] font-normal text-muted">{extra}</span>}
    </div>
  );
}

/** Label + control + hint or error message. */
export function Field({
  label,
  extra,
  hint,
  error,
  children,
  className,
  htmlFor,
}: {
  label?: ReactNode;
  extra?: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  children: ReactNode;
  className?: string;
  htmlFor?: string;
}) {
  return (
    <div className={className}>
      {label && (
        <Label extra={extra} htmlFor={htmlFor}>
          {label}
        </Label>
      )}
      {children}
      {error ? (
        <div className="mt-1.5 flex items-start gap-1.5 text-[12px] text-red">
          <AlertCircle size={13} className="mt-px shrink-0" />
          <span>{error}</span>
        </div>
      ) : hint ? (
        <div className="mt-1.5 text-[12px] leading-snug text-muted">{hint}</div>
      ) : null}
    </div>
  );
}

export const inputBase =
  "rounded-lg border bg-surface text-[13px] text-text shadow-xs outline-none transition placeholder:text-muted/80 focus:border-accent/60 focus:ring-3 focus:ring-accent/15 disabled:opacity-60";
export const inputClass = cx(inputBase, "border-border px-3 py-2");

/** Full width unless the className sets a width. */
export function widthClass(className?: string) {
  return className && /(^|\s)w-/.test(className) ? "" : "w-full";
}

export function Input({ invalid, ...p }: React.InputHTMLAttributes<HTMLInputElement> & { invalid?: boolean }) {
  return <input {...p} aria-invalid={invalid || undefined} className={cx(inputClass, "h-9", invalid && "border-red/60", widthClass(p.className), p.className)} />;
}

export function TextArea({ invalid, ...p }: React.TextareaHTMLAttributes<HTMLTextAreaElement> & { invalid?: boolean }) {
  return (
    <textarea
      {...p}
      aria-invalid={invalid || undefined}
      className={cx(inputClass, "resize-none leading-relaxed", invalid && "border-red/60", widthClass(p.className), p.className)}
    />
  );
}

export function Switch({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
}) {
  return (
    <label className={cx("flex cursor-default items-start gap-3 py-1", disabled && "opacity-50")}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cx("relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors", checked ? "bg-primary" : "bg-border-strong")}
      >
        <span className={cx("absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition-transform duration-150 ease-[var(--ease)]", checked && "translate-x-4")} />
      </button>
      <span>
        <span className="block font-medium">{label}</span>
        {description && <span className="mt-0.5 block text-[13px] leading-snug text-muted">{description}</span>}
      </span>
    </label>
  );
}

export function Checkbox({ checked, onChange, children, invalid }: { checked: boolean; onChange: (v: boolean) => void; children: ReactNode; invalid?: boolean }) {
  return (
    <label className="flex cursor-default items-start gap-2.5">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        aria-invalid={invalid || undefined}
        className={cx("mt-0.5 size-4 shrink-0 accent-[var(--primary)]", invalid && "outline-2 outline-offset-1 outline-red/60")}
      />
      <span className="leading-snug">{children}</span>
    </label>
  );
}

/** Segmented radio group for 2-5 short options. */
export function Segmented<T extends string>({
  value,
  onChange,
  options,
  size = "md",
  className,
  "aria-label": ariaLabel,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode; icon?: ReactNode; title?: string }[];
  size?: "sm" | "md";
  className?: string;
  "aria-label"?: string;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const index = options.findIndex((o) => o.value === value);
  return (
    <div role="radiogroup" aria-label={ariaLabel} className={cx("inline-flex items-center gap-0.5 rounded-lg bg-surface-2 p-0.5", className)}>
      {options.map((o, i) => (
        <button
          key={o.value}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={i === (index < 0 ? 0 : index) ? 0 : -1}
          title={o.title}
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => {
            const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
            if (!d) return;
            e.preventDefault();
            const next = (i + d + options.length) % options.length;
            onChange(options[next].value);
            refs.current[next]?.focus();
          }}
          className={cx(
            "inline-flex flex-1 items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap transition-colors",
            size === "sm" ? "h-7 px-2.5 text-[12px]" : "h-8 px-3 text-[13px]",
            o.value === value ? "bg-surface text-text shadow-[0_1px_2px_rgb(0_0_0/0.1)]" : "text-text-2 hover:text-text",
          )}
        >
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Number field with − / + buttons. */
export function Stepper({
  value,
  onChange,
  min,
  max,
  step = 1,
  format,
  className,
  "aria-label": ariaLabel,
}: {
  value: number;
  onChange: (v: number) => void;
  min: number;
  max: number;
  step?: number;
  format?: (v: number) => ReactNode;
  className?: string;
  "aria-label"?: string;
}) {
  const clamp = (v: number) => Math.min(max, Math.max(min, Math.round(v / step) * step));
  return (
    <div
      role="spinbutton"
      tabIndex={0}
      aria-label={ariaLabel}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      onKeyDown={(e) => {
        if (e.key === "ArrowUp" || e.key === "ArrowRight") {
          e.preventDefault();
          onChange(clamp(value + step));
        } else if (e.key === "ArrowDown" || e.key === "ArrowLeft") {
          e.preventDefault();
          onChange(clamp(value - step));
        }
      }}
      className={cx(inputBase, "flex h-9 items-center border-border p-0", widthClass(className), className)}
    >
      <button
        type="button"
        tabIndex={-1}
        aria-label="Azalt"
        disabled={value <= min}
        onClick={() => onChange(clamp(value - step))}
        className="flex h-full w-9 items-center justify-center rounded-l-lg text-text-2 hover:bg-surface-2 disabled:text-muted/50"
      >
        <Minus size={14} />
      </button>
      <div className="flex-1 text-center font-medium tabular-nums">{format ? format(value) : value}</div>
      <button
        type="button"
        tabIndex={-1}
        aria-label="Artır"
        disabled={value >= max}
        onClick={() => onChange(clamp(value + step))}
        className="flex h-full w-9 items-center justify-center rounded-r-lg text-text-2 hover:bg-surface-2 disabled:text-muted/50"
      >
        <Plus size={14} />
      </button>
    </div>
  );
}

/** "Gelişmiş" style disclosure. */
export function Disclosure({ title, children, defaultOpen = false, className }: { title: ReactNode; children: ReactNode; defaultOpen?: boolean; className?: string }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className={className}>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="flex items-center gap-1.5 rounded-md py-1 text-[13px] font-medium text-text-2 hover:text-text"
      >
        <ChevronRight size={14} className={cx("text-muted transition-transform", open && "rotate-90")} />
        {title}
      </button>
      {open && <div className="fade-in mt-3">{children}</div>}
    </div>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Display

export type BadgeColor = "gray" | "accent" | "green" | "red" | "amber";

const BADGE_DOTS: Record<BadgeColor, string | null> = { gray: null, accent: "bg-accent", green: "bg-green", red: "bg-red", amber: "bg-amber" };

/** A quiet neutral tag. Status colors show only as a small dot, so a row of tags never turns into a rainbow. */
export function Badge({ children, color = "gray", className, title }: { children: ReactNode; color?: BadgeColor; className?: string; title?: string }) {
  const dot = BADGE_DOTS[color];
  return (
    <span
      title={title}
      className={cx("inline-flex h-5 items-center gap-1.5 rounded-[5px] bg-surface-2 px-1.5 text-[12px] whitespace-nowrap text-text-2 tabular-nums", className)}
    >
      {dot && <span className={cx("size-1.5 shrink-0 rounded-full", dot)} />}
      {children}
    </span>
  );
}

export function StatusDot({ color }: { color: "green" | "amber" | "red" | "gray" | "accent" }) {
  const c = { green: "bg-green", amber: "bg-amber", red: "bg-red", gray: "bg-muted/60", accent: "bg-accent" }[color];
  return <span className={cx("inline-block size-2 shrink-0 rounded-full", c)} />;
}

export function Flag({ language, size = 16, className }: { language: string | null | undefined; size?: number; className?: string }) {
  const url = flagUrl(language);
  if (!url) return <span className={cx("inline-block rounded-sm bg-surface-3", className)} style={{ width: size * 1.33, height: size }} />;
  return (
    <img
      src={url}
      alt={languageName(language)}
      title={languageName(language)}
      draggable={false}
      className={cx("inline-block shrink-0 rounded-[3px] object-cover shadow-[0_0_0_1px_rgb(0_0_0/0.08)]", className)}
      style={{ width: size * 1.33, height: size }}
    />
  );
}

export function Card({ children, className, onClick }: { children: ReactNode; className?: string; onClick?: () => void }) {
  return (
    <div onClick={onClick} className={cx("rounded-xl border border-border bg-surface shadow-card", className)}>
      {children}
    </div>
  );
}

export function ProgressBar({ value, className }: { value: number; className?: string }) {
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value * 100)}
      className={cx("h-1.5 overflow-hidden rounded-full bg-surface-3", className)}
    >
      <div
        className="h-full rounded-full bg-accent transition-[width] duration-500 ease-[var(--ease)]"
        style={{ width: `${Math.max(2, Math.min(100, value * 100))}%` }}
      />
    </div>
  );
}

/** Sentence-case heading for list sections; `extra` is quiet meta on the right (a count, a state). */
export function SectionLabel({ children, extra, className }: { children: ReactNode; extra?: ReactNode; className?: string }) {
  return (
    <div className={cx("flex items-center justify-between gap-2 px-1 pb-2 text-[12px] font-medium text-muted", className)}>
      <span>{children}</span>
      {extra && <span className="font-normal tabular-nums">{extra}</span>}
    </div>
  );
}

/** Compact empty state: what this place is for, plus one action. Parents decide where it sits. */
export function EmptyState({
  icon,
  title,
  description,
  action,
  className,
}: {
  icon: ReactNode;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cx("mx-auto flex max-w-sm flex-col items-center px-6 py-10 text-center", className)}>
      <div className="mb-3 flex size-11 items-center justify-center rounded-xl bg-surface-2 text-muted">{icon}</div>
      <div className="text-[14px] font-medium">{title}</div>
      {description && <div className="mt-1 text-[13px] leading-relaxed text-muted">{description}</div>}
      {action && <div className="mt-4 flex flex-wrap justify-center gap-2">{action}</div>}
    </div>
  );
}

export function IssueList({ issues, className }: { issues: { message: string }[]; className?: string }) {
  if (!issues.length) return null;
  return (
    <div role="alert" className={cx("flex gap-2.5 rounded-lg border border-red/25 bg-red-soft px-3.5 py-2.5 text-[13px] text-red", className)}>
      <AlertCircle size={15} className="mt-px shrink-0" />
      {issues.length === 1 ? (
        <div>{issues[0].message}</div>
      ) : (
        <ul className="list-inside list-disc space-y-0.5">
          {issues.map((s, i) => (
            <li key={i}>{s.message}</li>
          ))}
        </ul>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Page frame

/** Shared page header: title, one-line description, actions on the right; optional row below (filters, tabs). */
export function PageHeader({
  title,
  description,
  actions,
  children,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cx("drag-region shrink-0 border-b border-border bg-surface px-7 pt-5 pb-4", className)}>
      <div className="flex min-h-9 items-center gap-4">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[20px] leading-tight font-semibold">{title}</h1>
          {description && <p className="mt-0.5 truncate text-[13px] text-muted">{description}</p>}
        </div>
        {actions && <div className="no-drag flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
      {children && <div className="no-drag mt-4">{children}</div>}
    </header>
  );
}

/** Title row of a side panel (results, details). */
export function PanelHeader({ title, subtitle, actions, className }: { title: ReactNode; subtitle?: ReactNode; actions?: ReactNode; className?: string }) {
  return (
    <div className={cx("flex min-h-12 shrink-0 items-center gap-3 border-b border-border px-5 py-2.5", className)}>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">{title}</div>
        {subtitle && <div className="truncate text-[12px] text-muted">{subtitle}</div>}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-1">{actions}</div>}
    </div>
  );
}

/** Tabs for filters (Tümü, Favoriler…). */
export function FilterTabs<T extends string>({
  value,
  onChange,
  options,
}: {
  value: T;
  onChange: (v: T) => void;
  options: { value: T; label: ReactNode; count?: number }[];
}) {
  return (
    <div role="tablist" className="flex flex-wrap items-center gap-1">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="tab"
          aria-selected={value === o.value}
          onClick={() => onChange(o.value)}
          className={cx(
            "inline-flex h-8 items-center gap-1.5 rounded-md px-3 text-[13px] font-medium transition-colors",
            value === o.value ? "bg-surface-2 text-text shadow-[inset_0_0_0_1px_var(--border)]" : "text-text-2 hover:bg-surface-2/60 hover:text-text",
          )}
        >
          {o.label}
          {o.count !== undefined && <span className="tabular-nums opacity-60">{o.count}</span>}
        </button>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Dialogs

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  width = "max-w-lg",
  footer,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  width?: string;
  footer?: ReactNode;
}) {
  const titleId = useId();
  const panel = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) onCloseRef.current();
    };
    window.addEventListener("keydown", onKey);
    // Focus the first field (or the panel) unless something inside already has focus via autoFocus.
    requestAnimationFrame(() => {
      if (!panel.current || panel.current.contains(document.activeElement)) return;
      const first = panel.current.querySelector<HTMLElement>("input, textarea, [data-autofocus]");
      (first ?? panel.current).focus();
    });
    return () => {
      window.removeEventListener("keydown", onKey);
      previous?.focus?.();
    };
  }, [open]);
  if (!open) return null;
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/35 p-6" onMouseDown={onClose}>
      <div
        ref={panel}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx("fade-in flex max-h-[88vh] w-full flex-col overflow-hidden rounded-2xl border border-border bg-surface shadow-overlay outline-none", width)}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-border px-5 py-3.5">
          <div className="min-w-0">
            <div id={titleId} className="text-[15px] font-semibold">
              {title}
            </div>
            {description && <div className="mt-0.5 text-[13px] text-muted">{description}</div>}
          </div>
          <IconButton label="Kapat" onClick={onClose} className="-mr-1.5">
            <X size={16} />
          </IconButton>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        {footer && <div className="flex items-center justify-end gap-2 border-t border-border bg-surface-2/40 px-5 py-3">{footer}</div>}
      </div>
    </div>,
    document.body,
  );
}

export function ConfirmDialog({
  open,
  onClose,
  onConfirm,
  title,
  description,
  confirmLabel = "Onayla",
  destructive,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void | Promise<void>;
  title: ReactNode;
  description?: ReactNode;
  confirmLabel?: string;
  destructive?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      width="max-w-md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Vazgeç
          </Button>
          <Button
            data-autofocus
            variant={destructive ? "destructive" : "primary"}
            loading={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await onConfirm();
                onClose();
              } finally {
                setBusy(false);
              }
            }}
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {description && <div className="px-5 py-4 text-[13px] leading-relaxed text-text-2">{description}</div>}
    </Modal>
  );
}

/** Asks for one line of text (a name). `onSubmit` may throw to keep the dialog open. */
export function PromptDialog({
  open,
  onClose,
  onSubmit,
  title,
  label,
  hint,
  initialValue = "",
  placeholder,
  confirmLabel = "Kaydet",
  allowEmpty,
}: {
  open: boolean;
  onClose: () => void;
  onSubmit: (value: string) => void | Promise<void>;
  title: ReactNode;
  label?: ReactNode;
  hint?: ReactNode;
  initialValue?: string;
  placeholder?: string;
  confirmLabel?: string;
  allowEmpty?: boolean;
}) {
  const [value, setValue] = useState(initialValue);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (open) {
      setValue(initialValue);
      setError(null);
    }
  }, [open, initialValue]);
  const submit = async () => {
    if (!allowEmpty && !value.trim()) return setError("Bir ad yazın");
    setBusy(true);
    try {
      await onSubmit(value.trim());
      onClose();
    } catch (e) {
      setError(e && typeof e === "object" && "message" in e ? String((e as { message: unknown }).message) : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal
      open={open}
      onClose={onClose}
      title={title}
      width="max-w-md"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Vazgeç
          </Button>
          <Button variant="primary" loading={busy} onClick={submit}>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="px-5 py-4">
        <Field label={label} hint={hint} error={error}>
          <Input
            autoFocus
            value={value}
            placeholder={placeholder}
            invalid={!!error}
            onChange={(e) => {
              setValue(e.target.value);
              setError(null);
            }}
            onKeyDown={(e) => e.key === "Enter" && submit()}
          />
        </Field>
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------------------------------------------
// Menus

/** Dropdown menu/panel anchored to its trigger. */
export function Popover({
  trigger,
  children,
  align = "left",
  width = 240,
  open: controlledOpen,
  onOpenChange,
  className,
}: {
  trigger: (toggle: () => void, open: boolean) => ReactNode;
  children: (close: () => void) => ReactNode;
  align?: "left" | "right";
  width?: number;
  open?: boolean;
  onOpenChange?: (v: boolean) => void;
  /** Classes for the anchor wrapper (e.g. w-full for a full-width trigger). */
  className?: string;
}) {
  const [innerOpen, setInnerOpen] = useState(false);
  const open = controlledOpen ?? innerOpen;
  const setOpenRef = useRef<(v: boolean) => void>(() => {});
  setOpenRef.current = (v: boolean) => (onOpenChange ? onOpenChange(v) : setInnerOpen(v));
  const anchor = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number; above: boolean }>({ x: 0, y: 0, above: false });
  useEffect(() => {
    if (!open || !anchor.current) return;
    const r = anchor.current.getBoundingClientRect();
    const above = r.bottom + 360 > window.innerHeight && r.top > 360;
    const x = align === "left" ? Math.min(r.left, window.innerWidth - width - 8) : Math.max(8, r.right - width);
    setPos({ x, y: above ? window.innerHeight - r.top + 6 : r.bottom + 6, above });
    const onOutside = (e: MouseEvent) => {
      const target = e.target as Node;
      if (anchor.current?.contains(target) || panel.current?.contains(target)) return;
      setOpenRef.current(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) {
        e.preventDefault();
        setOpenRef.current(false);
      }
    };
    window.addEventListener("mousedown", onOutside);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("mousedown", onOutside);
      window.removeEventListener("keydown", onKey, true);
    };
  }, [open, align, width]);
  return (
    <div ref={anchor} className={cx("relative inline-flex", className)}>
      {trigger(() => setOpenRef.current(!open), open)}
      {open &&
        createPortal(
          <div
            ref={panel}
            className="fade-in fixed z-50 overflow-hidden rounded-xl border border-border bg-surface shadow-overlay"
            style={{ left: pos.x, width, ...(pos.above ? { bottom: pos.y } : { top: pos.y }) }}
          >
            {children(() => setOpenRef.current(false))}
          </div>,
          document.body,
        )}
    </div>
  );
}

export function MenuItem({
  icon,
  children,
  description,
  hint,
  onClick,
  danger,
  disabled,
}: {
  icon?: ReactNode;
  children: ReactNode;
  description?: ReactNode;
  hint?: ReactNode;
  onClick: () => void;
  danger?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={cx(
        "flex w-full items-start gap-2.5 px-3 py-2 text-left text-[13px] transition-colors hover:bg-surface-2 focus-visible:bg-surface-2 focus-visible:outline-none disabled:pointer-events-none disabled:opacity-45",
        danger ? "text-red" : "text-text",
      )}
    >
      {icon !== undefined && <span className={cx("mt-0.5 flex w-4 shrink-0 justify-center", danger ? "text-red" : "text-muted")}>{icon}</span>}
      <span className="min-w-0 flex-1">
        <span className="block">{children}</span>
        {description && <span className="mt-0.5 block text-[12px] leading-snug text-muted">{description}</span>}
      </span>
      {hint && <span className="mt-0.5 shrink-0 font-mono text-[11px] text-muted">{hint}</span>}
    </button>
  );
}

export function MenuLabel({ children }: { children: ReactNode }) {
  return <div className="px-3 pt-2 pb-1 text-[12px] font-medium text-muted">{children}</div>;
}

export function MenuDivider() {
  return <div className="my-1 h-px bg-border" />;
}

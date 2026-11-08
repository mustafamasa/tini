// Custom select: a button that opens a listbox popover. Keyboard: arrows, Home/End, Enter/Space, Escape, type-ahead.
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronsUpDown } from "lucide-react";
import { cx, inputBase, widthClass } from "./ui";

export interface SelectOption<T extends string = string> {
  value: T;
  label: string;
  /** Second line in the list (not shown on the button). */
  description?: string;
  icon?: ReactNode;
  disabled?: boolean;
}

interface Position {
  left: number;
  top?: number;
  bottom?: number;
  width: number;
  maxHeight: number;
}

export function Select<T extends string>({
  value,
  onChange,
  options,
  className,
  size = "md",
  placeholder = "Seçin…",
  disabled,
  menuWidth,
  align = "left",
  id,
  "aria-label": ariaLabel,
}: {
  value: T;
  onChange: (v: T) => void;
  options: SelectOption<T>[];
  className?: string;
  size?: "sm" | "md";
  placeholder?: string;
  disabled?: boolean;
  /** Minimum width of the list; defaults to the button width. */
  menuWidth?: number;
  align?: "left" | "right";
  id?: string;
  "aria-label"?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  const [pos, setPos] = useState<Position | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ text: "", time: 0 });
  const listId = useId();
  const selectedIndex = options.findIndex((o) => o.value === value);
  const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  const firstEnabled = () => options.findIndex((o) => !o.disabled);

  const place = () => {
    const el = trigger.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const width = Math.max(r.width, menuWidth ?? 0);
    const wanted = Math.min(320, options.reduce((h, o) => h + (o.description ? 46 : 32), 10));
    const below = window.innerHeight - r.bottom - 10;
    const above = r.top - 10;
    const flip = below < wanted && above > below;
    let left = align === "right" ? r.right - width : r.left;
    left = Math.max(8, Math.min(left, window.innerWidth - width - 8));
    setPos(
      flip
        ? { left, bottom: window.innerHeight - r.top + 4, width, maxHeight: Math.min(320, above) }
        : { left, top: r.bottom + 4, width, maxHeight: Math.min(320, below) },
    );
  };

  const show = (index?: number) => {
    if (disabled || !options.length) return;
    place();
    setActive(index ?? (selectedIndex >= 0 ? selectedIndex : firstEnabled()));
    setOpen(true);
  };

  const hide = (refocus = true) => {
    setOpen(false);
    if (refocus) trigger.current?.focus();
  };

  const choose = (i: number) => {
    const o = options[i];
    if (!o || o.disabled) return;
    if (o.value !== value) onChange(o.value);
    hide();
  };

  const step = (from: number, dir: 1 | -1) => {
    let i = from;
    for (let n = 0; n < options.length; n++) {
      const next = i + dir;
      if (next < 0 || next >= options.length) return i;
      i = next;
      if (!options[i].disabled) return i;
    }
    return from;
  };

  const findByText = (key: string, from: number) => {
    const now = Date.now();
    const t = typeahead.current;
    t.text = now - t.time > 700 ? key : t.text + key;
    t.time = now;
    const q = fold(t.text);
    const start = t.text.length === 1 ? from + 1 : Math.max(0, from);
    for (let n = 0; n < options.length; n++) {
      const i = (start + n) % options.length;
      const o = options[i];
      if (!o.disabled && fold(o.label).startsWith(q)) return i;
    }
    return -1;
  };

  /** Case-folds for matching: Turkish lower-casing, then dotless ı = i so "i" finds "Italiano" and "ı" finds "Işık". */
  const fold = (text: string) => text.toLocaleLowerCase("tr").replace(/ı/g, "i");

  const isPrintable = (e: React.KeyboardEvent) => e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey;

  const onTriggerKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      show();
    } else if (isPrintable(e)) {
      const i = findByText(e.key, selectedIndex);
      if (i >= 0 && options[i].value !== value) onChange(options[i].value);
    }
  };

  const onListKey = (e: React.KeyboardEvent) => {
    const typing = Date.now() - typeahead.current.time < 700;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => (i < 0 ? firstEnabled() : step(i, 1)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => (i < 0 ? firstEnabled() : step(i, -1)));
    } else if (e.key === "Home") {
      e.preventDefault();
      setActive(firstEnabled());
    } else if (e.key === "End") {
      e.preventDefault();
      setActive(options.length - 1 - [...options].reverse().findIndex((o) => !o.disabled));
    } else if (e.key === "Enter" || (e.key === " " && !typing)) {
      e.preventDefault();
      choose(active);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      hide();
    } else if (e.key === "Tab") {
      e.preventDefault();
      hide();
    } else if (isPrintable(e)) {
      e.preventDefault();
      const i = findByText(e.key, active);
      if (i >= 0) setActive(i);
    }
  };

  useEffect(() => {
    if (!open) return;
    list.current?.focus({ preventScroll: true });
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (trigger.current?.contains(t) || list.current?.contains(t)) return;
      setOpen(false);
    };
    const onScroll = (e: Event) => {
      if (list.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    const onResize = () => setOpen(false);
    window.addEventListener("mousedown", onDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("mousedown", onDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onResize);
    };
  }, [open]);

  useLayoutEffect(() => {
    if (open && active >= 0) document.getElementById(`${listId}-${active}`)?.scrollIntoView({ block: "nearest" });
  }, [open, active, listId]);

  return (
    <>
      <button
        ref={trigger}
        id={id}
        type="button"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? listId : undefined}
        aria-label={ariaLabel}
        onClick={() => (open ? hide() : show())}
        onKeyDown={onTriggerKey}
        className={cx(
          inputBase,
          "flex items-center gap-2 text-left disabled:pointer-events-none",
          size === "sm" ? "h-8 pr-2 pl-2.5 text-[13px]" : "h-9 pr-2.5 pl-3",
          open ? "border-accent/60 ring-3 ring-accent/15" : "border-border hover:border-border-strong",
          widthClass(className),
          className,
        )}
      >
        {selected?.icon && <span className="flex shrink-0 text-muted">{selected.icon}</span>}
        <span className={cx("min-w-0 flex-1 truncate", !selected && "text-muted")}>{selected?.label ?? placeholder}</span>
        <ChevronsUpDown size={14} className="shrink-0 text-muted" />
      </button>
      {open &&
        pos &&
        createPortal(
          <div
            ref={list}
            id={listId}
            role="listbox"
            tabIndex={-1}
            aria-label={ariaLabel}
            aria-activedescendant={active >= 0 ? `${listId}-${active}` : undefined}
            onKeyDown={onListKey}
            className="fade-in fixed z-[70] overflow-y-auto rounded-xl border border-border bg-surface p-1 shadow-overlay outline-none"
            style={{ left: pos.left, top: pos.top, bottom: pos.bottom, width: pos.width, maxHeight: pos.maxHeight }}
          >
            {options.map((o, i) => (
              <div
                key={o.value}
                id={`${listId}-${i}`}
                role="option"
                aria-selected={o.value === value}
                aria-disabled={o.disabled || undefined}
                onMouseMove={() => !o.disabled && active !== i && setActive(i)}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => choose(i)}
                className={cx(
                  "flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-[13px]",
                  i === active && !o.disabled && "bg-surface-2",
                  o.disabled && "opacity-45",
                )}
              >
                {o.icon && <span className="flex shrink-0 text-muted">{o.icon}</span>}
                <span className="min-w-0 flex-1">
                  <span className={cx("block truncate", o.value === value && "font-medium")}>{o.label}</span>
                  {o.description && <span className="block truncate text-[12px] text-muted">{o.description}</span>}
                </span>
                <Check size={14} className={cx("shrink-0 text-text", o.value === value ? "opacity-100" : "opacity-0")} />
              </div>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}

// Script editor: one line = one sentence, end-of-line pause markers, inline findings and pronunciation dictionary hints.
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { EditorState, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  keymap,
  lineNumbers,
  placeholder as placeholderExt,
  highlightActiveLine,
  highlightActiveLineGutter,
  drawSelection,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, moveLineDown, moveLineUp, copyLineDown, deleteLine } from "@codemirror/commands";
import { setDiagnostics, type Diagnostic } from "@codemirror/lint";
import type { TextCheck } from "../api";

/** Pause markers (`|| name`) understood by the core; the names are part of the user-facing script syntax. */
export const PAUSES: { name: string; label: string; seconds: number }[] = [
  { name: "bagli", label: "Bağlı", seconds: 0.25 },
  { name: "normal", label: "Normal", seconds: 0.5 },
  { name: "paragraf", label: "Paragraf", seconds: 1 },
  { name: "agir", label: "Ağır", seconds: 1.5 },
];

const PAUSE_RE = /\|\|[^\n]*$/;

const pauseMarkers = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = this.build(view);
    }
    update(u: ViewUpdate) {
      if (u.docChanged || u.viewportChanged) this.decorations = this.build(u.view);
    }
    build(view: EditorView) {
      const b = new RangeSetBuilder<Decoration>();
      for (const { from, to } of view.visibleRanges) {
        let pos = from;
        while (pos <= to) {
          const line = view.state.doc.lineAt(pos);
          const m = PAUSE_RE.exec(line.text);
          if (m) b.add(line.from + m.index, line.to, Decoration.mark({ class: "pause-marker" }));
          pos = line.to + 1;
        }
      }
      return b.finish();
    }
  },
  { decorations: (v) => v.decorations },
);

const setDictionaryMatches = StateEffect.define<TextCheck["dictionary"]>();
const dictionaryField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setDictionaryMatches)) {
        const b = new RangeSetBuilder<Decoration>();
        const doc = tr.state.doc;
        for (const m of e.value) {
          if (m.line > doc.lines) continue;
          const line = doc.line(m.line);
          const start = line.from + charToCode(line.text, m.column);
          const end = line.from + charToCode(line.text, m.column + m.length);
          if (end <= line.to && end > start)
            b.add(start, end, Decoration.mark({ class: "dictionary-match", attributes: { title: `Okunuş: ${m.pronunciation}` } }));
        }
        value = b.finish();
      }
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** The core counts Unicode code points; JS strings are UTF-16. Converts a code point offset to a UTF-16 offset. */
function charToCode(text: string, chars: number): number {
  let i = 0;
  let n = 0;
  for (const ch of text) {
    if (n >= chars) break;
    i += ch.length;
    n++;
  }
  return i;
}

const theme = EditorView.theme({
  "&": { color: "var(--text)" },
  ".cm-placeholder": { color: "var(--muted)" },
});

export interface ScriptEditorHandle {
  setPause: (name: string | null) => void;
  setText: (text: string) => void;
  /** Moves the cursor to a line (1-based) and column (code points) and focuses the editor. */
  focusLine: (line: number, column?: number) => void;
}

interface Props {
  text: string;
  onChange: (text: string) => void;
  check: TextCheck | null;
  onSubmit?: () => void;
  placeholder?: string;
  className?: string;
}

export const ScriptEditor = forwardRef<ScriptEditorHandle, Props>(function ScriptEditor({ text, onChange, check, onSubmit, placeholder, className }, ref) {
  const container = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;

  useEffect(() => {
    if (!container.current) return;
    const v = new EditorView({
      parent: container.current,
      state: EditorState.create({
        doc: text,
        extensions: [
          lineNumbers(),
          history(),
          drawSelection(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          EditorView.lineWrapping,
          placeholderExt(placeholder ?? ""),
          keymap.of([
            { key: "Mod-Enter", run: () => (onSubmitRef.current?.(), true) },
            { key: "Alt-ArrowUp", run: moveLineUp },
            { key: "Alt-ArrowDown", run: moveLineDown },
            { key: "Mod-d", run: copyLineDown, preventDefault: true },
            { key: "Mod-Shift-k", run: deleteLine },
            ...historyKeymap,
            ...defaultKeymap,
          ]),
          pauseMarkers,
          dictionaryField,
          theme,
          EditorView.contentAttributes.of({ spellcheck: "false", autocorrect: "off", "aria-label": "Okunacak metin" }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) onChangeRef.current(u.state.doc.toString());
          }),
        ],
      }),
    });
    view.current = v;
    return () => v.destroy();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Apply text coming from outside (e.g. loading a draft) when it differs.
  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== text) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text } });
    }
  }, [text]);

  useEffect(() => {
    const v = view.current;
    if (!v || !check) return;
    const doc = v.state.doc;
    const diags: Diagnostic[] = [];
    for (const f of check.findings) {
      if (f.line > doc.lines) continue;
      const line = doc.line(f.line);
      const from = line.from + charToCode(line.text, f.column);
      const to = Math.min(line.to, line.from + charToCode(line.text, f.column + Math.max(1, f.length)));
      diags.push({ from, to: Math.max(to, from), severity: f.code === "pause" || f.code === "long_line" ? "warning" : "error", message: f.message });
    }
    v.dispatch(setDiagnostics(v.state, diags));
    v.dispatch({ effects: setDictionaryMatches.of(check.dictionary) });
  }, [check]);

  useImperativeHandle(ref, () => ({
    setPause: (name) => {
      const v = view.current;
      if (!v) return;
      const line = v.state.doc.lineAt(v.state.selection.main.head);
      if (!line.text.trim()) return;
      const m = PAUSE_RE.exec(line.text);
      const body = (m ? line.text.slice(0, m.index) : line.text).replace(/\s+$/, "");
      const next = name ? `${body} || ${name}` : body;
      v.dispatch({ changes: { from: line.from, to: line.to, insert: next }, selection: { anchor: line.from + body.length } });
      v.focus();
    },
    setText: (t) => {
      const v = view.current;
      if (!v) return;
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: t } });
      v.focus();
    },
    focusLine: (lineNo, column = 0) => {
      const v = view.current;
      if (!v || lineNo < 1 || lineNo > v.state.doc.lines) return;
      const line = v.state.doc.line(lineNo);
      v.dispatch({ selection: { anchor: Math.min(line.to, line.from + charToCode(line.text, column)) }, scrollIntoView: true });
      v.focus();
    },
  }));

  return <div ref={container} className={"script-editor " + (className ?? "")} />;
});

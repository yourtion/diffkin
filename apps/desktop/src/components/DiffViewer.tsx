import { useEffect, useRef, useState } from "react";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap, lineNumbers } from "@codemirror/view";
import { languageExtensions } from "../lib/languages";
import { bindMoveConnection } from "../lib/move-connection";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { defaultKeymap } from "@codemirror/commands";
import { openSearchPanel, search, searchKeymap } from "@codemirror/search";
import { MergeView, goToNextChunk, goToPreviousChunk, unifiedMergeView } from "@codemirror/merge";
import type { AnalysisResult, ChangeEvent, SourceRange } from "@diffkin/protocol";
import { isProvenFormat } from "../lib/presentation";
import {
  formatFolding,
  foldFormats as foldFormatEffect,
  unifiedFormatFolding,
  type FormatProof,
} from "../lib/format-folding";

type Props = {
  before: string;
  after: string;
  path: string;
  event: ChangeEvent | null;
  scope: boolean;
  analysis: AnalysisResult | null;
  foldFormats: boolean;
  onFoldChange: (fold: boolean) => void;
  layout: "split" | "unified";
  collapse: boolean;
  onLayoutChange: (layout: "split" | "unified") => void;
  onCollapseChange: (collapse: boolean) => void;
};
export function editorOffset(source: string, offset: number) {
  return source.slice(0, offset).replace(/\r\n?/g, "\n").length;
}

export function DiffViewer({
  before,
  after,
  path,
  event,
  scope,
  analysis,
  foldFormats,
  onFoldChange,
  layout,
  collapse,
  onLayoutChange: setLayout,
  onCollapseChange: setCollapse,
}: Props) {
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const overlay = useRef<SVGSVGElement>(null);
  const views = useRef<{ a?: EditorView; b: EditorView } | null>(null);
  const [copied, setCopied] = useState("");
  const focused = scope && event !== null;
  const move = event?.facets.includes("move") && event.before && event.after ? event : null;
  const old =
    focused && event.before
      ? before.slice(event.before.utf16Start, event.before.utf16End)
      : focused
        ? ""
        : before;
  const next =
    focused && event.after
      ? after.slice(event.after.utf16Start, event.after.utf16End)
      : focused
        ? ""
        : after;
  useEffect(() => {
    if (!host.current) return;
    const extensions = (label: string): Extension[] => [
      lineNumbers(),
      ...languageExtensions(path, focused && path.toLowerCase().endsWith(".vue")),
      syntaxHighlighting(defaultHighlightStyle),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.contentAttributes.of({ "aria-label": label, tabindex: "0" }),
      keymap.of([
        ...searchKeymap,
        ...defaultKeymap,
        { key: "Alt-ArrowDown", run: goToNextChunk },
        { key: "Alt-ArrowUp", run: goToPreviousChunk },
      ]),
      search({ top: true }),
      formatFolding,
      EditorView.theme({
        "&": { fontSize: "12px" },
        ".cm-content": { fontFamily: "'SFMono-Regular', Consolas, monospace" },
        ".cm-scroller": { overflowX: "auto" },
      }),
    ];
    const proven =
      !foldFormats || focused || !analysis
        ? []
        : analysis.events.filter(
            (candidate) => candidate.id !== event?.id && isProvenFormat(candidate, analysis),
          );
    const pairedProofs: FormatProof[] = proven.flatMap((candidate) =>
      candidate.before && candidate.after
        ? [
            {
              before: {
                from: editorOffset(before, candidate.before.utf16Start),
                to: editorOffset(before, candidate.before.utf16End),
              },
              after: {
                from: editorOffset(after, candidate.after.utf16Start),
                to: editorOffset(after, candidate.after.utf16End),
              },
            },
          ]
        : [],
    );
    const fold = (view: EditorView, side: "before" | "after", source: string) => {
      const ranges = proven
        .map((candidate) => candidate[side])
        .filter((span): span is SourceRange => span !== null)
        .map((span) => ({
          from: editorOffset(source, span.utf16Start),
          to: editorOffset(source, span.utf16End),
        }))
        .filter(({ from, to }) => from >= 0 && to > from && to <= view.state.doc.length)
        .sort((left, right) => left.from - right.from);
      // Nested format evidence must not create overlapping editor replacements.
      const separate: Array<{ from: number; to: number }> = [];
      for (const range of ranges) {
        const previous = separate.at(-1);
        if (previous && range.from < previous.to) previous.to = Math.max(previous.to, range.to);
        else separate.push({ ...range });
      }
      if (separate.length) view.dispatch({ effects: foldFormatEffect.of(separate) });
    };
    if (layout === "split") {
      const merge = new MergeView({
        parent: host.current,
        a: { doc: old, extensions: extensions("修改前源码") },
        b: { doc: next, extensions: extensions("修改后源码") },
        collapseUnchanged: collapse ? { margin: 4, minSize: 8 } : undefined,
        highlightChanges: true,
        gutter: true,
        diffConfig: { scanLimit: 2000, timeout: 100 },
      });
      views.current = { a: merge.a, b: merge.b };
      fold(merge.a, "before", before);
      fold(merge.b, "after", after);
      const disconnect =
        move && !focused && stage.current && overlay.current
          ? bindMoveConnection(
              stage.current,
              overlay.current,
              merge.a,
              merge.b,
              move,
              editorOffset(before, move.before!.utf16Start),
              editorOffset(after, move.after!.utf16Start),
            )
          : () => {};
      return () => {
        disconnect();
        views.current = null;
        merge.destroy();
      };
    }
    const unified = new EditorView({
      parent: host.current,
      doc: next,
      extensions: [
        ...extensions("统一差异源码"),
        unifiedMergeView({
          original: old,
          mergeControls: false,
          syntaxHighlightDeletions: false,
          collapseUnchanged: collapse ? { margin: 4, minSize: 8 } : undefined,
          diffConfig: { scanLimit: 2000, timeout: 100 },
        }),
        unifiedFormatFolding(pairedProofs),
      ],
    });
    views.current = { b: unified };
    fold(unified, "after", after);
    return () => {
      views.current = null;
      unified.destroy();
    };
  }, [
    old,
    next,
    layout,
    collapse,
    path,
    focused,
    analysis,
    foldFormats,
    event,
    move,
    before,
    after,
  ]);

  useEffect(() => {
    if (focused || !event || !views.current) return;
    for (const [view, span, source] of [
      [views.current.a, event.before, before],
      [views.current.b, event.after, after],
    ] as const) {
      if (view && span) {
        const anchor = Math.min(editorOffset(source, span.utf16Start), view.state.doc.length);
        const head = Math.min(editorOffset(source, span.utf16End), view.state.doc.length);
        view.dispatch({
          selection: { anchor, head },
          effects: EditorView.scrollIntoView(anchor, { y: "center" }),
        });
      }
    }
  }, [event, focused, before, after, layout, collapse, foldFormats]);

  function locate(side: "before" | "after") {
    const view = side === "before" ? views.current?.a : views.current?.b;
    const range = event?.[side];
    if (view && range && !focused) {
      const position = Math.min(
        editorOffset(side === "before" ? before : after, range.utf16Start),
        view.state.doc.length,
      );
      view.dispatch({
        selection: { anchor: position },
        effects: EditorView.scrollIntoView(position, { y: "center" }),
      });
      view.focus();
    }
  }
  function jump(direction: "next" | "previous") {
    const view = views.current?.b;
    if (view) {
      (direction === "next" ? goToNextChunk : goToPreviousChunk)(view);
      view.focus();
    }
  }
  async function copy(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopied("已复制原始内容");
    } catch {
      setCopied("复制失败，可在源码中选择并复制");
    }
    setTimeout(() => setCopied(""), 2500);
  }
  return (
    <section className="diff-viewer" aria-label="代码差异">
      <div className="diff-toolbar">
        <div className="segmented" aria-label="差异布局">
          <button aria-pressed={layout === "split"} onClick={() => setLayout("split")}>
            双栏
          </button>
          <button aria-pressed={layout === "unified"} onClick={() => setLayout("unified")}>
            统一
          </button>
        </div>
        <button onClick={() => jump("previous")} title="Alt + ↑">
          ↑ 上一处
        </button>
        <button onClick={() => jump("next")} title="Alt + ↓">
          ↓ 下一处
        </button>
        <button aria-pressed={!foldFormats} onClick={() => onFoldChange(!foldFormats)}>
          {foldFormats ? "完整原始视图" : "收起格式"}
        </button>
        <button
          onClick={() => {
            const view = views.current?.b;
            if (view) openSearchPanel(view);
          }}
        >
          搜索 ⌘/Ctrl F
        </button>
        <label className="check">
          <input
            type="checkbox"
            checked={collapse}
            onChange={(e) => setCollapse(e.target.checked)}
          />
          折叠未改行
        </label>
      </div>
      {move && !focused && layout === "split" && (
        <div className="move-navigation">
          <span>↗ 移动连线 · {move.title}</span>
          <button onClick={() => locate("before")}>起点 L{move.before?.startLine}</button>
          <button onClick={() => locate("after")}>终点 L{move.after?.startLine}</button>
          <small>虚线端点在视口外 · 仅突出当前关系</small>
        </div>
      )}
      <div className="source-headings">
        <span>
          修改前{focused && event?.before ? ` · L${event.before.startLine}` : ""}
          <button onClick={() => void copy(old)}>复制</button>
        </span>
        <span>
          修改后{focused && event?.after ? ` · L${event.after.startLine}` : ""}
          <button onClick={() => void copy(next)}>复制</button>
        </span>
      </div>
      <div
        className={`merge-stage ${move && !focused && layout === "split" ? "has-move" : ""}`}
        ref={stage}
      >
        <div className="merge-host" ref={host} />
        {move && !focused && layout === "split" && <svg className="move-overlay" ref={overlay} />}
      </div>
      <div className="source-meta" aria-live="polite">
        {copied ||
          `${focused ? "当前变更局部对照" : foldFormats ? "格式降噪视图 · 原始内容完整保留" : "完整原始内容"} · 只读 · ${before.includes("\r\n") ? "CRLF" : "LF"} → ${after.includes("\r\n") ? "CRLF" : "LF"}${!after.endsWith("\n") && after ? " · 修改后无末尾换行" : ""}`}
      </div>
    </section>
  );
}

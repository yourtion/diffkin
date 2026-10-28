import { StateEffect, StateField, type Extension } from "@codemirror/state";
import { getChunks, getOriginalDoc } from "@codemirror/merge";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

type Range = { from: number; to: number };
export type FormatProof = { before: Range; after: Range };
export const foldFormats = StateEffect.define<Range[]>();
const revealFormat = StateEffect.define<Range>();

class FormatPlaceholder extends WidgetType {
  constructor(readonly range: Range) {
    super();
  }
  eq(other: FormatPlaceholder) {
    return this.range.from === other.range.from && this.range.to === other.range.to;
  }
  toDOM(view: EditorView) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "cm-foldPlaceholder";
    button.textContent = "已证明的格式变化 · 点击展开";
    button.setAttribute("aria-label", "展开已证明的格式变化");
    button.onclick = () => view.dispatch({ effects: revealFormat.of(this.range) });
    return button;
  }
  ignoreEvent() {
    return false;
  }
}

export const formatFolding = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    for (const effect of transaction.effects) {
      if (effect.is(foldFormats))
        value = Decoration.set(
          effect.value.map((range) =>
            Decoration.replace({
              block: true,
              inclusive: true,
              widget: new FormatPlaceholder(range),
            }).range(range.from, range.to),
          ),
          true,
        );
      if (effect.is(revealFormat))
        value = value.update({
          filter: (from, to) => from !== effect.value.from || to !== effect.value.to,
        });
    }
    return value;
  },
  provide: (field) => [
    EditorView.decorations.from(field),
    EditorView.atomicRanges.of((view) => view.state.field(field)),
  ],
});

/**
 * Unified merge deletions are zero-width block widgets before the changed document.
 * A replacement of that document cannot cover a widget at its first boundary.
 * Keep the original document/merge state intact, and only suppress a complete
 * rendered chunk when both sides fit one proven range which is still folded.
 * Public merge positions, rather than widget classes or private decoration fields,
 * determine whether the old text is eligible. The CSS class locates rendered DOM.
 */
export function unifiedFormatFolding(proofs: readonly FormatProof[]): Extension {
  return ViewPlugin.fromClass(
    class {
      private disposed = false;
      private hidden = new Map<
        HTMLElement,
        { hidden: HTMLElement["hidden"]; display: string; ariaHidden: string | null }
      >();
      constructor(readonly view: EditorView) {
        this.schedule();
      }
      update(update: ViewUpdate) {
        if (
          update.viewportChanged ||
          update.docChanged ||
          update.startState.field(formatFolding) !== update.state.field(formatFolding)
        )
          this.schedule();
      }
      docViewUpdate() {
        this.schedule();
      }
      private active(proof: FormatProof) {
        let folded = false;
        this.view.state
          .field(formatFolding)
          .between(proof.after.from, proof.after.to, (from, to) => {
            if (from <= proof.after.from && to >= proof.after.to) folded = true;
          });
        return folded;
      }
      private restore(element: HTMLElement) {
        const original = this.hidden.get(element);
        if (!original) return;
        element.hidden = original.hidden;
        element.style.display = original.display;
        if (original.ariaHidden === null) element.removeAttribute("aria-hidden");
        else element.setAttribute("aria-hidden", original.ariaHidden);
        this.hidden.delete(element);
      }
      private schedule() {
        this.view.requestMeasure({
          key: this,
          read: (view) => {
            if (this.disposed) return [];
            const chunks = getChunks(view.state)?.chunks ?? [];
            const originalLength = getOriginalDoc(view.state).length;
            const folded = proofs.filter((proof) => this.active(proof));
            return Array.from(
              view.contentDOM.querySelectorAll<HTMLElement>(".cm-deletedChunk"),
            ).map((element) => {
              const position = view.posAtDOM(element);
              const chunk = chunks.find((candidate) => candidate.fromB === position);
              const hide =
                chunk !== undefined &&
                folded.some(
                  (proof) =>
                    proof.before.from <= chunk.fromA &&
                    proof.before.to >= Math.min(chunk.toA, originalLength) &&
                    proof.after.from <= chunk.fromB &&
                    proof.after.to >= Math.min(chunk.toB, view.state.doc.length),
                );
              return { element, hide };
            });
          },
          write: (widgets, view) => {
            if (this.disposed) return;
            let changed = false;
            const rendered = new Set(widgets.map(({ element }) => element));
            for (const element of this.hidden.keys()) {
              if (!rendered.has(element)) this.restore(element);
            }
            for (const { element, hide } of widgets) {
              if (hide && !this.hidden.has(element)) {
                this.hidden.set(element, {
                  hidden: element.hidden,
                  display: element.style.display,
                  ariaHidden: element.getAttribute("aria-hidden"),
                });
                element.hidden = true;
                element.style.display = "none";
                element.setAttribute("aria-hidden", "true");
                changed = true;
              } else if (!hide && this.hidden.has(element)) {
                this.restore(element);
                changed = true;
              }
            }
            // The next measure accounts for the changed block height. This does
            // not dispatch a transaction or alter the comparison's source text.
            if (changed) view.requestMeasure();
          },
        });
      }
      destroy() {
        this.disposed = true;
        for (const element of this.hidden.keys()) this.restore(element);
      }
    },
  );
}

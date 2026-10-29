import type { EditorView } from "@codemirror/view";
import {
  getSearchQuery,
  openSearchPanel,
  searchPanelOpen,
  setSearchQuery,
  SearchQuery,
} from "@codemirror/search";

type Side = { anchor: number; head: number; left: number; query: SearchQuery; search: boolean };
type Memory = { top: number; a?: Side; b: Side };
const memories = new Map<string, Memory>();
function side(view: EditorView): Side {
  const { anchor, head } = view.state.selection.main;
  return {
    anchor,
    head,
    left: view.scrollDOM.scrollLeft,
    query: new SearchQuery({
      ...getSearchQuery(view.state),
      search:
        view.dom.querySelector<HTMLInputElement>('.cm-search input[name="search"]')?.value ??
        getSearchQuery(view.state).search,
    }),
    search: searchPanelOpen(view.state),
  };
}
export function saveEditor(
  key: string,
  host: HTMLElement,
  views: { a?: EditorView; b: EditorView },
) {
  const value = { top: host.scrollTop, a: views.a ? side(views.a) : undefined, b: side(views.b) };
  if (value.b.query.search.length > 4096 || (value.a?.query.search.length ?? 0) > 4096) return;
  memories.delete(key);
  memories.set(key, value);
  if (memories.size > 100) memories.delete(memories.keys().next().value!);
}
export function restoreEditor(
  key: string,
  host: HTMLElement,
  views: { a?: EditorView; b: EditorView },
) {
  const value = memories.get(key);
  if (!value) return;
  for (const [view, saved] of [
    [views.a, value.a],
    [views.b, value.b],
  ] as const) {
    if (!view || !saved) continue;
    if (saved.search) openSearchPanel(view);
    view.dispatch({
      selection: {
        anchor: Math.min(saved.anchor, view.state.doc.length),
        head: Math.min(saved.head, view.state.doc.length),
      },
      effects: setSearchQuery.of(saved.query),
    });
    view.requestMeasure({
      read: () => null,
      write: () => {
        if (view.dom.isConnected) {
          host.scrollTop = value.top;
          view.scrollDOM.scrollLeft = saved.left;
        }
      },
    });
  }
}

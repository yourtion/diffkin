import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import type { FileSummary } from "@diffkin/protocol";

const rowHeight = 44;
const statuses: Record<string, string> = {
  added: "A",
  deleted: "D",
  modified: "M",
  renamed: "R",
  conflict: "!",
  "type-changed": "T",
};
type Row =
  | { key: string; depth: number; directory: string }
  | { key: string; depth: number; file: FileSummary };
export function ChangeNavigator({
  files,
  visible,
  snapshotId,
  selected,
  onSelect,
  reviewed,
  query,
}: {
  files: FileSummary[];
  visible: boolean;
  snapshotId: string | undefined;
  selected: string | undefined;
  onSelect: (file: FileSummary) => void;
  reviewed: (file: FileSummary) => boolean;
  query: string;
}) {
  const [tree, setTree] = useState(false);
  const [closed, setClosed] = useState<Set<string>>(new Set());
  const [top, setTop] = useState(0);
  const [height, setHeight] = useState(600);
  const viewport = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => {
    const result: Row[] = [];
    const seen = new Set<string>();
    for (const file of files) {
      const parts = file.path.split("/");
      let hidden = false;
      if (tree)
        for (let depth = 0; depth < parts.length - 1; depth++) {
          const path = parts.slice(0, depth + 1).join("/");
          if (!seen.has(path)) {
            seen.add(path);
            result.push({ key: `dir:${path}`, directory: path, depth });
          }
          if (!query && closed.has(path)) {
            hidden = true;
            break;
          }
        }
      if (!hidden) result.push({ key: file.id, file, depth: tree ? parts.length - 1 : 0 });
    }
    return result;
  }, [files, tree, closed, query]);
  const fileRows = useMemo(() => {
    const indexes = new Map<string, number>();
    rows.forEach((row, index) => {
      if ("file" in row) indexes.set(row.file.id, index);
    });
    return indexes;
  }, [rows]);
  const selectedRow = selected === undefined ? undefined : fileRows.get(selected);
  useEffect(() => {
    const element = viewport.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setHeight(element.clientHeight));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (viewport.current) viewport.current.scrollTop = 0;
  }, [query, tree]);
  // Only navigation context changes reveal the selection; appended pages and
  // review updates must not interrupt manual scrolling.
  useEffect(() => {
    const element = viewport.current;
    if (visible && selectedRow !== undefined && element) {
      const y = selectedRow * rowHeight;
      if (y < element.scrollTop || y + rowHeight > element.scrollTop + element.clientHeight)
        element.scrollTop = y;
    }
  }, [selected, selectedRow, query, tree, visible, snapshotId]);
  function navigate(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next = Math.max(
      0,
      Math.min(
        rows.length - 1,
        event.key === "Home"
          ? 0
          : event.key === "End"
            ? rows.length - 1
            : index + (event.key === "ArrowDown" ? 1 : -1),
      ),
    );
    const element = viewport.current;
    if (!element) return;
    element.scrollTop = next * rowHeight;
    setTop(element.scrollTop);
    requestAnimationFrame(() =>
      element.querySelector<HTMLButtonElement>(`[data-row="${next}"] button`)?.focus(),
    );
  }
  const start = Math.max(0, Math.min(rows.length - 1, Math.floor(top / rowHeight) - 4));
  const end = Math.min(rows.length, start + Math.ceil(height / rowHeight) + 9);
  return (
    <>
      <div className="navigator-layout">
        <button aria-pressed={!tree} onClick={() => setTree(false)}>
          平铺
        </button>
        <button aria-pressed={tree} onClick={() => setTree(true)}>
          目录树
        </button>
        <span>{files.length} 个匹配</span>
      </div>
      <div
        className="file-list virtual-files"
        ref={viewport}
        onScroll={(event) => setTop(event.currentTarget.scrollTop)}
      >
        <div style={{ height: rows.length * rowHeight, position: "relative" }}>
          {rows.slice(start, end).map((row, offset) => (
            <div
              key={row.key}
              data-row={start + offset}
              style={{
                position: "absolute",
                top: (start + offset) * rowHeight,
                height: rowHeight,
                width: "100%",
              }}
            >
              {"directory" in row ? (
                <button
                  onKeyDown={(event) => navigate(event, start + offset)}
                  className="directory-row"
                  style={{ paddingLeft: 10 + Math.min(row.depth, 8) * 12 }}
                  aria-expanded={!!query || !closed.has(row.directory)}
                  onClick={() =>
                    setClosed((current) => {
                      const next = new Set(current);
                      if (next.has(row.directory)) next.delete(row.directory);
                      else next.add(row.directory);
                      return next;
                    })
                  }
                >
                  {!query && closed.has(row.directory) ? "▸" : "▾"}{" "}
                  {row.directory.split("/").at(-1)}
                </button>
              ) : (
                <button
                  onKeyDown={(event) => navigate(event, start + offset)}
                  className={`file-item ${row.file.id === selected ? "selected" : ""}`}
                  style={{ paddingLeft: 10 + Math.min(row.depth, 8) * 12 }}
                  aria-label={row.file.path}
                  aria-current={row.file.id === selected ? "true" : undefined}
                  title={[
                    row.file.oldPath ? `${row.file.oldPath} → ${row.file.path}` : row.file.path,
                    ...row.file.notices,
                  ].join("\n")}
                  onClick={() => onSelect(row.file)}
                >
                  <span className={`file-status ${row.file.status}`}>
                    {statuses[row.file.status] ?? "?"}
                  </span>
                  <span className="file-name">
                    <span className="path-directory">
                      {row.file.path.slice(0, row.file.path.lastIndexOf("/") + 1)}
                    </span>
                    <span className="path-basename">{row.file.path.split("/").at(-1)}</span>
                  </span>
                  {reviewed(row.file) && (
                    <span className="review-dot" aria-label="已审查">
                      ✓
                    </span>
                  )}
                </button>
              )}
            </div>
          ))}
        </div>
      </div>
    </>
  );
}

import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ChangeEvent,
  ComparisonManifest,
  ComparisonSpec,
  FileAnalysis,
  FileSummary,
} from "@diffkin/protocol";
import { client, desktopAvailable, Generation, message, type DiffkinClient } from "./lib/client";
import { loadPreferences, savePreferences } from "./lib/preferences";
import { DiffViewer } from "./components/DiffViewer";
import { ChangeInspector } from "./components/ChangeInspector";

const status: Record<string, string> = {
  added: "A",
  deleted: "D",
  modified: "M",
  renamed: "R",
  "type-changed": "T",
  conflict: "!",
};
const modeNames = {
  worktree: "全部本地改动",
  staged: "已暂存",
  unstaged: "未暂存",
  commits: "两个提交",
};
let nextJob = 0;
function jobId() {
  return `ui-${Date.now()}-${++nextJob}`;
}
function reviewKey(manifest: ComparisonManifest, file: FileSummary, basis: string) {
  return `${manifest.id}:${file.id}:${file.beforeId}:${file.afterId}:${basis}`;
}
function analysisBasis(data: FileAnalysis) {
  return data.analysis
    ? `${data.analysis.schemaVersion}:${data.analysis.ruleVersion}`
    : "metadata-v1";
}
function readReviews(): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem("diffkin.reviews.v1") ?? "[]");
    return Array.isArray(value)
      ? value.filter((item): item is string => typeof item === "string").slice(-4096)
      : [];
  } catch {
    return [];
  }
}

export function App({
  api = client,
  available = desktopAvailable,
}: {
  api?: DiffkinClient;
  available?: boolean;
}) {
  const [initial] = useState(loadPreferences);
  const [spec, setSpec] = useState<ComparisonSpec>(initial.spec);
  const [recent, setRecent] = useState(initial.recent);
  const [manifest, setManifest] = useState<ComparisonManifest | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSummary | null>(null);
  const [data, setData] = useState<FileAnalysis | null>(null);
  const [event, setEvent] = useState<ChangeEvent | null>(null);
  const [scope, setScope] = useState(false);
  const [foldFormats, setFoldFormats] = useState(true);
  const [filter, setFilter] = useState("");
  const [reviews, setReviews] = useState(readReviews);
  const [reviewBases, setReviewBases] = useState<Record<string, string>>({});
  const [pending, setPending] = useState<"capture" | "file" | null>(null);
  const [paging, setPaging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const captureGeneration = useRef(new Generation());
  const fileGeneration = useRef(new Generation());
  const pageGeneration = useRef(new Generation());
  const captureJob = useRef<string | null>(null);
  const fileJob = useRef<string | null>(null);
  const pathInput = useRef<HTMLInputElement>(null);
  const visibleFiles =
    manifest?.files.filter((file) =>
      file.path.toLocaleLowerCase().includes(filter.toLocaleLowerCase()),
    ) ?? [];

  const cancelOne = useCallback(
    (ref: { current: string | null }) => {
      if (ref.current) {
        void api.cancel(ref.current).catch(() => {});
        ref.current = null;
      }
    },
    [api],
  );
  function cancel() {
    captureGeneration.current.next();
    fileGeneration.current.next();
    pageGeneration.current.next();
    cancelOne(captureJob);
    cancelOne(fileJob);
    setPending(null);
    setPaging(false);
    setError("已取消。可以重新选择文件或刷新比较。");
  }
  async function selectFile(file: FileSummary, snapshot = manifest) {
    if (!snapshot) return;
    cancelOne(fileJob);
    const generation = fileGeneration.current.next();
    const job = jobId();
    fileJob.current = job;
    setSelectedFile(file);
    setData(null);
    setEvent(null);
    setScope(false);
    setPending("file");
    setError(null);
    try {
      const result = await api.file(snapshot.id, file.id, job);
      if (!fileGeneration.current.isCurrent(generation)) return;
      if (result.file.summary.id !== file.id) throw new Error("收到不属于当前文件的结果");
      if (result.analysis && result.analysis.schemaVersion !== 1)
        throw new Error("不支持的比较协议版本");
      setData(result);
      setReviewBases((current) => ({ ...current, [file.id]: analysisBasis(result) }));
    } catch (cause) {
      if (fileGeneration.current.isCurrent(generation)) setError(message(cause));
    } finally {
      if (fileGeneration.current.isCurrent(generation)) {
        fileJob.current = null;
        setPending(null);
      }
    }
  }
  async function open() {
    if (!available || !spec.repository.trim()) return;
    cancelOne(captureJob);
    cancelOne(fileJob);
    fileGeneration.current.next();
    pageGeneration.current.next();
    const generation = captureGeneration.current.next();
    const job = jobId();
    captureJob.current = job;
    setManifest(null);
    setReviewBases({});
    setData(null);
    setSelectedFile(null);
    setEvent(null);
    setFilter("");
    setError(null);
    setPending("capture");
    setPaging(false);
    const request = { ...spec, repository: spec.repository.trim() };
    try {
      const result = await api.open(request, job);
      if (!captureGeneration.current.isCurrent(generation)) return;
      setManifest(result);
      setRecent(savePreferences(request, recent));
      if (result.files[0]) void selectFile(result.files[0], result);
      else setPending(null);
    } catch (cause) {
      if (captureGeneration.current.isCurrent(generation)) {
        setError(message(cause));
        setPending(null);
      }
    } finally {
      if (captureGeneration.current.isCurrent(generation)) captureJob.current = null;
    }
  }
  async function more() {
    if (!manifest || paging) return;
    const id = manifest.id;
    const generation = pageGeneration.current.next();
    setPaging(true);
    try {
      const page = await api.page(id, manifest.files.length);
      if (!pageGeneration.current.isCurrent(generation)) return;
      setManifest((current) =>
        current?.id === id
          ? {
              ...current,
              files: [
                ...current.files,
                ...page.files.filter(
                  (file) => !current.files.some((previous) => previous.id === file.id),
                ),
              ],
            }
          : current,
      );
    } catch (cause) {
      if (pageGeneration.current.isCurrent(generation)) setError(message(cause));
    } finally {
      if (pageGeneration.current.isCurrent(generation)) setPaging(false);
    }
  }
  function markReviewed() {
    if (!manifest || !selectedFile || !data) return;
    const key = reviewKey(manifest, selectedFile, analysisBasis(data));
    const next = reviews.includes(key)
      ? reviews.filter((item) => item !== key)
      : [...reviews, key].slice(-4096);
    setReviews(next);
    try {
      localStorage.setItem("diffkin.reviews.v1", JSON.stringify(next));
    } catch {
      /* In-memory review remains usable. */
    }
  }
  useEffect(() => {
    function keyboard(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === "o") {
        e.preventDefault();
        pathInput.current?.focus();
        return;
      }
      if (
        e.target instanceof HTMLElement &&
        (e.target.closest("input, textarea, select, .cm-editor") || e.target.isContentEditable)
      )
        return;
      if ((e.key === "j" || e.key === "k") && visibleFiles.length) {
        e.preventDefault();
        const index = visibleFiles.findIndex((file) => file.id === selectedFile?.id);
        const next =
          visibleFiles[
            Math.max(0, Math.min(visibleFiles.length - 1, index + (e.key === "j" ? 1 : -1)))
          ];
        if (next) void selectFile(next);
      }
    }
    window.addEventListener("keydown", keyboard);
    return () => window.removeEventListener("keydown", keyboard);
  });
  useEffect(
    () => () => {
      cancelOne(captureJob);
      cancelOne(fileJob);
    },
    [cancelOne],
  );

  const isReviewed = (file: FileSummary) =>
    !!manifest &&
    !!reviewBases[file.id] &&
    reviews.includes(reviewKey(manifest, file, reviewBases[file.id]));
  const reviewed = selectedFile ? isReviewed(selectedFile) : false;
  return (
    <main className="app-shell">
      <header className="app-header">
        <div className="brand">
          <span className="mark">↔</span>
          <h1>
            Diffkin <span>观变</span>
          </h1>
          <span className="alpha">ALPHA</span>
        </div>
        <span className="local-badge">
          <i /> 本地 · 只读审查
        </span>
      </header>
      <form
        className="repository-bar"
        onSubmit={(e) => {
          e.preventDefault();
          void open();
        }}
      >
        <label className="repo-path">
          <span>仓库</span>
          <input
            ref={pathInput}
            aria-label="Git 仓库路径"
            placeholder="输入本地 Git 仓库路径…"
            value={spec.repository}
            onChange={(e) => setSpec({ ...spec, repository: e.target.value })}
            list="recent-repositories"
          />
        </label>
        <datalist id="recent-repositories">
          {recent.map((path) => (
            <option key={path} value={path}>
              {path}
            </option>
          ))}
        </datalist>
        <select
          aria-label="比较模式"
          value={spec.mode}
          onChange={(e) => setSpec({ ...spec, mode: e.target.value as ComparisonSpec["mode"] })}
        >
          {Object.entries(modeNames).map(([mode, label]) => (
            <option key={mode} value={mode}>
              {label}
            </option>
          ))}
        </select>
        {spec.mode === "commits" ? (
          <div className="revision-inputs">
            <input
              aria-label="基线提交"
              value={spec.base ?? ""}
              onChange={(e) => setSpec({ ...spec, base: e.target.value })}
              placeholder="HEAD~1"
            />
            <span>→</span>
            <input
              aria-label="目标提交"
              value={spec.head ?? ""}
              onChange={(e) => setSpec({ ...spec, head: e.target.value })}
              placeholder="HEAD"
            />
          </div>
        ) : (
          spec.mode !== "staged" && (
            <label className="check">
              <input
                type="checkbox"
                checked={spec.includeUntracked}
                onChange={(e) => setSpec({ ...spec, includeUntracked: e.target.checked })}
              />
              未跟踪文件
            </label>
          )
        )}
        <button
          className="primary"
          type="submit"
          disabled={!available || !spec.repository.trim() || pending === "capture"}
        >
          {pending === "capture" ? "捕获中…" : manifest ? "刷新比较" : "打开比较"}
        </button>
        {pending && (
          <button className="cancel" type="button" onClick={cancel}>
            取消
          </button>
        )}
      </form>
      {!available && (
        <p className="notice">浏览器仅预览界面。请运行桌面应用以读取本地 Git 仓库。</p>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {manifest && (
        <div className="snapshot-bar">
          <span>{modeNames[manifest.mode]}</span>
          <code title={manifest.baseLabel}>{manifest.baseLabel}</code>
          <span>→</span>
          <code title={manifest.headLabel}>{manifest.headLabel}</code>
          <span className="snapshot-tag" title={manifest.id}>
            已冻结快照 · 刷新后读取新改动
          </span>
        </div>
      )}
      {manifest?.warnings.map((warning, index) => (
        <p className="notice compact" key={index}>
          {warning}
        </p>
      ))}
      <div className={`workspace ${!manifest ? "unopened" : ""}`} aria-busy={pending !== null}>
        <nav className="files-panel" aria-label="变更文件">
          <div className="panel-heading">
            <h2>变更文件</h2>
            <span>{manifest?.totalFiles ?? 0}</span>
          </div>
          <input
            className="file-search"
            aria-label="筛选文件"
            placeholder="筛选路径…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <div className="file-list">
            {visibleFiles.map((file) => (
              <button
                key={file.id}
                className={`file-item ${file.id === selectedFile?.id ? "selected" : ""}`}
                aria-current={file.id === selectedFile?.id ? "true" : undefined}
                onClick={() => void selectFile(file)}
                title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}
              >
                <span className={`file-status ${file.status}`}>{status[file.status] ?? "?"}</span>
                <span className="file-name">
                  {file.path}
                  <small>{file.oldPath ? `← ${file.oldPath}` : (file.notices[0] ?? "")}</small>
                </span>
                {isReviewed(file) && (
                  <span className="review-dot" aria-label="已审查">
                    ✓
                  </span>
                )}
              </button>
            ))}
            {manifest && visibleFiles.length === 0 && (
              <p className="muted">{manifest.totalFiles ? "没有匹配的路径" : "没有变化"}</p>
            )}
            {manifest && manifest.files.length < manifest.totalFiles && (
              <button className="load-more" disabled={paging} onClick={() => void more()}>
                {paging ? "加载中…" : `加载更多（${manifest.files.length}/${manifest.totalFiles}）`}
              </button>
            )}
          </div>
          <div className="file-help">J / K 切换文件 · ⌘/Ctrl O 打开</div>
        </nav>
        <section className="comparison-panel">
          {selectedFile ? (
            <>
              <div className="file-heading">
                <div>
                  <h2>{selectedFile.path}</h2>
                  <p>
                    {selectedFile.oldMode ?? "∅"} → {selectedFile.newMode ?? "∅"}
                    {data?.analysis
                      ? ` · +${data.analysis.raw.added} −${data.analysis.raw.removed}`
                      : ""}
                  </p>
                </div>
                <button
                  disabled={!data}
                  className={reviewed ? "reviewed" : ""}
                  aria-pressed={reviewed}
                  onClick={markReviewed}
                >
                  {reviewed ? "✓ 已审查此版本" : "标记已审查"}
                </button>
              </div>
              {selectedFile.notices.map((notice, index) => (
                <p className="notice compact" key={index}>
                  {notice}
                </p>
              ))}
              <details
                className="source-identities"
                open={data?.file.before === null && data?.file.after === null}
              >
                <summary>来源身份与文件元数据</summary>
                <dl>
                  <dt>变化类型</dt>
                  <dd>{selectedFile.status}</dd>
                  <dt>修改前路径</dt>
                  <dd>{selectedFile.oldPath ?? selectedFile.path}</dd>
                  <dt>修改后路径</dt>
                  <dd>{selectedFile.path}</dd>
                  <dt>修改前对象</dt>
                  <dd>
                    <code>{selectedFile.beforeId ?? "不存在"}</code>
                  </dd>
                  <dt>修改后对象</dt>
                  <dd>
                    <code>{selectedFile.afterId ?? "不存在"}</code>
                  </dd>
                  <dt>文件模式</dt>
                  <dd>
                    {selectedFile.oldMode ?? "∅"} → {selectedFile.newMode ?? "∅"}
                  </dd>
                </dl>
              </details>
              {data?.notice && <p className="notice compact">{data.notice}</p>}
              {data?.file.beforeTruncated || data?.file.afterTruncated ? (
                <p className="notice compact">
                  当前为有限预览，尚未完成全文件细化。不能据此确认全部内容。
                </p>
              ) : null}
              {event && (
                <div className="relation-bar">
                  <strong>{event.title}</strong>
                  <label className="check">
                    <input
                      type="checkbox"
                      checked={scope}
                      onChange={(e) => setScope(e.target.checked)}
                    />
                    仅对照这组变化
                  </label>
                  <button
                    onClick={() => {
                      setEvent(null);
                      setScope(false);
                    }}
                  >
                    返回完整文件
                  </button>
                </div>
              )}
              {data?.file.before === null && data.file.after === null ? (
                <div className="empty-state metadata-state">
                  <h2>内容未载入，请核对对象身份与元数据</h2>
                  <p>二进制、子模块或不可读取的内容不会被视为没有变化。</p>
                </div>
              ) : data ? (
                <DiffViewer
                  before={data.file.before ?? ""}
                  after={data.file.after ?? ""}
                  path={selectedFile.path}
                  event={event}
                  scope={scope}
                  analysis={data.analysis}
                  foldFormats={foldFormats}
                  onFoldChange={setFoldFormats}
                />
              ) : (
                <div className="empty-state">
                  <span className="spinner" />
                  <h2>{pending === "file" ? "正在比较冻结内容" : "选择文件以重新比较"}</h2>
                </div>
              )}
            </>
          ) : (
            <div className="empty-state">
              <span className="empty-mark">↔</span>
              <h2>
                {pending === "capture"
                  ? "正在捕获仓库快照"
                  : manifest
                    ? "当前比较没有文件变化"
                    : "看清变化，从一次比较开始"}
              </h2>
              <p>
                {manifest
                  ? "可切换比较模式，或刷新读取新的改动。"
                  : "打开本地仓库，审查暂存区、工作区或两个提交之间的变化。"}
              </p>
              <div className="empty-features">
                <span>原始内容完整保留</span>
                <span>移动与内部修改对照</span>
                <span>JS / TS 结构分析</span>
              </div>
            </div>
          )}
        </section>
        {data?.analysis && (
          <ChangeInspector
            key={`${manifest?.id}:${selectedFile?.id}`}
            result={data.analysis}
            selected={event}
            fold={foldFormats}
            onFoldChange={setFoldFormats}
            onSelect={(selected) => {
              setEvent(selected);
              setScope(selected?.facets.includes("move") ?? false);
            }}
          />
        )}
      </div>
      <footer>
        <span>
          {pending === "capture"
            ? "冻结文件与比较基线…"
            : pending === "file"
              ? "计算文本与结构变化…"
              : "就绪"}
        </span>
        <span>
          {manifest
            ? `${manifest.files.filter(isReviewed).length} / ${manifest.totalFiles} 已审查 · `
            : ""}
          源码留在本机 · 不执行仓库脚本
        </span>
      </footer>
    </main>
  );
}

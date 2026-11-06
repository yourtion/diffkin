/* oxlint-disable jsx-a11y/prefer-tag-over-role -- A focusable window splitter uses the ARIA separator widget, not a thematic hr. */
import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type {
  ChangeEvent,
  ComparisonManifest,
  ComparisonSpec,
  FileAnalysis,
  FileSummary,
  WorkspaceSession,
  EvidenceBasis,
} from "@diffkin/protocol";
import { client, desktopAvailable, Generation, message, type DiffkinClient } from "./lib/client";
import { FileAnalysisCache } from "./lib/file-analysis-cache";
import { usePreferences } from "./lib/use-preferences";
import { PreferencesPanel } from "./components/PreferencesPanel";
import { ChangeNavigator } from "./components/ChangeNavigator";
import { LargeFileViewer, needsSegments } from "./components/LargeFileViewer";
import { DiffViewer } from "./components/DiffViewer";
import { ChangeInspector } from "./components/ChangeInspector";
import { ExplanationPanel } from "./components/ExplanationPanel";
import { WorkspaceHome } from "./components/WorkspaceHome";
import { WorkspacePanel } from "./components/WorkspacePanel";
import { ValidationPanel } from "./components/ValidationPanel";
import { ComparisonSetup } from "./components/ComparisonSetup";
import { comparisonSources, diffSources, modeNames } from "./lib/comparison-direction";
import { CaptureNotices } from "./components/CaptureNotices";
import {
  workspaceClient,
  sameBasis,
  lastViewedWorkspace,
  rememberWorkspace,
} from "./lib/workspace-client";

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
  const settings = usePreferences(available);
  const { recent, view } = settings.preferences;
  const [draft, setDraft] = useState<ComparisonSpec | null>(null);
  const spec = draft ?? settings.preferences.spec;
  const setSpec = (next: ComparisonSpec) => {
    if (draft) setDraft(next);
    else settings.update((current) => ({ ...current, spec: next }));
  };
  const foldFormats = view.foldFormats;
  const setFoldFormats = (foldFormats: boolean) =>
    settings.update((current) => ({ ...current, view: { ...current.view, foldFormats } }));
  const [showFiles, setShowFiles] = useState(
    () => window.matchMedia("(min-width: 1001px)").matches,
  );
  useEffect(() => {
    const wide = window.matchMedia("(min-width: 1001px)");
    const resize = () => setShowFiles(wide.matches);
    wide.addEventListener("change", resize);
    return () => wide.removeEventListener("change", resize);
  }, []);
  const [showDetails, setShowDetails] = useState(false);
  const [aiPanel, setAiPanel] = useState<"explain" | "quiz" | null>(null);
  const [showCompareSettings, setShowCompareSettings] = useState(false);
  const [navigationWidth, setNavigationWidth] = useState(260);
  const resizeNavigation = (value: number) =>
    setNavigationWidth(Math.max(200, Math.min(420, value)));
  const [showPreferences, setShowPreferences] = useState(false);
  const [workspaceSession, setWorkspaceSession] = useState<WorkspaceSession | null>(null);
  const [repositoryId, setRepositoryId] = useState<string | null>(null);
  const [workspacePage, setWorkspacePage] = useState<"home" | "create" | "settings" | "review">(
    "home",
  );
  const showWorkspace = workspacePage === "create" || workspacePage === "settings";
  const [workspaceBusy, setWorkspaceBusy] = useState(false);
  const [validationOpen, setValidationOpen] = useState(false);
  const [validationMounted, setValidationMounted] = useState(false);
  const [savingReview, setSavingReview] = useState(false);
  const [manifest, setManifest] = useState<ComparisonManifest | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileSummary | null>(null);
  const repositoryViews = useRef(
    new Map<string, { manifest: ComparisonManifest; file: FileSummary }>(),
  );
  const [analysisReady, setAnalysisReady] = useState(false);
  const [data, setData] = useState<FileAnalysis | null>(null);
  const [fileCache] = useState(() => new FileAnalysisCache());
  const [event, setEvent] = useState<ChangeEvent | null>(null);
  const [scope, setScope] = useState(false);
  const searchInput = useRef<HTMLInputElement>(null);
  const [catalogBusy, setCatalogBusy] = useState(false);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [history, setHistory] = useState<{ snapshot: string; files: FileSummary[]; index: number }>(
    {
      snapshot: "",
      files: [],
      index: -1,
    },
  );
  const [filter, setFilter] = useState("");
  const [reviews, setReviews] = useState(readReviews);
  const [reviewBases, setReviewBases] = useState<Record<string, string>>({});
  const [capturing, setCapturing] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const pending = capturing ? "capture" : loadingFile ? "file" : null;
  const [paging, setPaging] = useState(false);
  const [cancelled, setCancelled] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const captureGeneration = useRef(new Generation());
  const fileGeneration = useRef(new Generation());
  const pageGeneration = useRef(new Generation());
  const captureJob = useRef<string | null>(null);
  const fileJob = useRef<string | null>(null);
  const [appliedSpec, setAppliedSpec] = useState<ComparisonSpec | null>(null);
  const [recoverScope, setRecoverScope] = useState(false);
  const editButton = useRef<HTMLButtonElement>(null);
  const filesButton = useRef<HTMLButtonElement>(null);
  const workspaceTabs = useRef<HTMLElement>(null);
  const pathInput = useRef<HTMLInputElement>(null);
  const pickerButton = useRef<HTMLButtonElement>(null);
  const files = manifest?.files;
  const visibleFiles = useMemo(() => {
    if (!filter) return files ?? [];
    const query = filter.toLocaleLowerCase();
    return files?.filter((file) => file.path.toLocaleLowerCase().includes(query)) ?? [];
  }, [files, filter]);

  const cancelOne = useCallback(
    (ref: { current: string | null }) => {
      if (ref.current) {
        void api.cancel(ref.current).catch(() => {});
        ref.current = null;
      }
    },
    [api],
  );
  function beginComparisonOperation() {
    fileGeneration.current.next();
    pageGeneration.current.next();
    cancelOne(captureJob);
    cancelOne(fileJob);
    setCapturing(false);
    setLoadingFile(false);
    setPaging(false);
    setCancelled(false);
    setError(null);
    setRecoverScope(false);
    return captureGeneration.current.next();
  }
  function cancel() {
    beginComparisonOperation();
    setCancelled(true);
  }
  function changeRepository(repository: string) {
    setSpec({ ...spec, repository });
    if (repository.trim() === spec.repository.trim()) return;
    setError(null);
    setRecoverScope(false);
    setCancelled(false);
  }
  function editComparison(pickDirectory = false) {
    if (workspaceBusy || capturing) return;
    if (appliedSpec && !showCompareSettings) setDraft(appliedSpec);
    setShowCompareSettings(true);
    setWorkspacePage("review");
    setShowPreferences(false);
    setValidationOpen(false);
    setError(null);
    setRecoverScope(false);
    requestAnimationFrame(() => {
      if (pickDirectory && available) pickerButton.current?.click();
      else pathInput.current?.focus();
    });
  }
  function backToReview() {
    setDraft(null);
    setShowCompareSettings(false);
    setError(null);
    setRecoverScope(false);
    requestAnimationFrame(() => editButton.current?.focus());
  }
  function isCurrentRepository(path: string) {
    return manifest !== null && (path === appliedSpec?.repository || path === manifest.repository);
  }
  function leaveComparison() {
    repositoryViews.current.clear();
    setAppliedSpec(null);
    setWorkspaceSession(null);
    setRepositoryId(null);
    setManifest(null);
    resetComparisonView();
  }
  function resetComparisonView() {
    setData(null);
    setAnalysisReady(false);
    setSelectedFile(null);
    setHistory({ snapshot: "", files: [], index: -1 });
    setReviewBases({});
    setEvent(null);
    setScope(false);
    setAiPanel(null);
    setFilter("");
    setCatalogBusy(false);
    setCatalogError(null);
  }
  async function selectFile(file: FileSummary, snapshot = manifest, fromHistory = false) {
    if (!snapshot) return;
    if (!fromHistory)
      setHistory((current) => {
        if (current.snapshot === snapshot.id && current.files[current.index]?.id === file.id)
          return current;
        const preceding =
          current.snapshot === snapshot.id ? current.files.slice(0, current.index + 1) : [];
        const files = [...preceding, file].slice(-100);
        return { snapshot: snapshot.id, files, index: files.length - 1 };
      });
    cancelOne(fileJob);
    const generation = fileGeneration.current.next();
    setSelectedFile(file);
    setAiPanel(null);
    setScope(false);
    setCancelled(false);
    setError(null);
    const show = (result: FileAnalysis) => {
      setData(result);
      setAnalysisReady(true);
      setEvent(result.analysis?.events.find((item) => item.facets.includes("move")) ?? null);
      setReviewBases((current) => ({ ...current, [file.id]: analysisBasis(result) }));
    };
    const cached = fileCache.get(snapshot.id, file.id);
    if (cached) {
      show(cached);
      setLoadingFile(false);
      return;
    }
    const job = jobId();
    fileJob.current = job;
    setData(null);
    setAnalysisReady(false);
    setEvent(null);
    setLoadingFile(true);
    let settled = false;
    void api
      .preview(snapshot.id, file.id)
      .then((preview) => {
        if (
          !settled &&
          fileGeneration.current.isCurrent(generation) &&
          preview.summary.id === file.id
        )
          setData({ file: preview, analysis: null, notice: "冻结文本已就绪，正在分析结构…" });
      })
      .catch(() => {
        /* The authoritative analysis request reports any failure. */
      });
    try {
      const result = await api.file(snapshot.id, file.id, job);
      settled = true;
      if (!fileGeneration.current.isCurrent(generation)) return;
      if (result.file.summary.id !== file.id) throw new Error("收到不属于当前文件的结果");
      if (result.analysis && result.analysis.schemaVersion !== 1)
        throw new Error("不支持的比较协议版本");
      fileCache.remember(snapshot.id, result);
      show(result);
    } catch (cause) {
      if (fileGeneration.current.isCurrent(generation)) setError(message(cause));
    } finally {
      settled = true;
      if (fileGeneration.current.isCurrent(generation)) {
        fileJob.current = null;
        setLoadingFile(false);
      }
    }
  }
  async function open(input: ComparisonSpec, refresh = false) {
    if (!available || capturing || workspaceBusy || !input.repository.trim()) return;
    const generation = beginComparisonOperation();
    fileCache.clear();
    const job = jobId();
    captureJob.current = job;
    const request = { ...input, repository: input.repository.trim() };
    if (request.mode === "commits") {
      request.base = request.base || null;
      request.head = request.head || null;
    }
    const sameRepository = isCurrentRepository(request.repository);
    const previousPath = sameRepository ? selectedFile?.path : undefined;
    if ((manifest || workspaceSession) && !sameRepository) leaveComparison();
    setCapturing(true);
    try {
      let result = await api.open(request, job);
      if (!captureGeneration.current.isCurrent(generation)) return;
      if (
        previousPath &&
        result.repository === manifest?.repository &&
        result.files.length < result.totalFiles &&
        !result.files.some((file) => file.path === previousPath)
      )
        result = await api.catalog(result.id);
      if (!captureGeneration.current.isCurrent(generation)) return;
      setWorkspaceSession(null);
      repositoryViews.current.clear();
      // The previous snapshot can still be browsed while capture is in flight.
      fileCache.clear();
      setRepositoryId(null);
      setReviewBases({});
      setEvent(null);
      setAppliedSpec(request);
      setDraft(null);
      if (manifest && manifest.repository !== result.repository) resetComparisonView();
      setManifest(result);
      settings.update((current) => ({
        ...current,
        spec: request,
        recent: [...new Set([request.repository, ...current.recent])].slice(0, 8),
      }));
      setShowCompareSettings(false);
      if (!refresh) requestAnimationFrame(() => filesButton.current?.focus());
      const next =
        (result.repository === manifest?.repository
          ? result.files.find((file) => file.path === previousPath)
          : undefined) ?? result.files[0];
      if (next) void selectFile(next, result);
      else {
        cancelOne(fileJob);
        fileGeneration.current.next();
        setLoadingFile(false);
        resetComparisonView();
      }
    } catch (cause) {
      if (captureGeneration.current.isCurrent(generation)) {
        const overBudget =
          cause && typeof cause === "object" && "code" in cause && cause.code === "budget-exceeded";
        if (!refresh || overBudget) {
          setShowCompareSettings(true);
          setDraft(request);
        }
        setRecoverScope(Boolean(overBudget));
        setError(
          message(cause) + (overBudget ? "。可在「路径范围」填写仓库内的文件或目录后重试。" : ""),
        );
      }
    } finally {
      if (captureGeneration.current.isCurrent(generation)) {
        captureJob.current = null;
        setCapturing(false);
      }
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
  const needsCatalog = !!filter && !!manifest && manifest.files.length < manifest.totalFiles;
  const catalogId = needsCatalog ? manifest?.id : null;
  useEffect(() => {
    if (!catalogId) return;
    let active = true;
    const id = catalogId;
    queueMicrotask(() => {
      if (active) {
        setCatalogBusy(true);
        setCatalogError(null);
      }
    });
    void api
      .catalog(id)
      .then((result) => {
        if (!active) return;
        if (result.id !== id || result.files.length !== result.totalFiles)
          throw new Error("完整变更清单尚未就绪");
        setManifest((current) => (current?.id === id ? result : current));
      })
      .catch((cause) => {
        if (active) setCatalogError(message(cause));
      })
      .finally(() => {
        if (active) setCatalogBusy(false);
      });
    return () => {
      active = false;
    };
  }, [catalogId, api]);
  async function nextUnreviewed() {
    if (!manifest) return;
    const id = manifest.id;
    const ticket = fileGeneration.current.ticket();
    try {
      const complete =
        manifest.files.length < manifest.totalFiles ? await api.catalog(id) : manifest;
      if (!fileGeneration.current.isCurrent(ticket)) return;
      if (complete.id !== id || complete.files.length !== complete.totalFiles)
        throw new Error("完整变更清单尚未就绪");
      setManifest(complete);
      const files = complete.files.filter((file) =>
        file.path.toLocaleLowerCase().includes(filter.toLocaleLowerCase()),
      );
      const index = files.findIndex((file) => file.id === selectedFile?.id);
      const next = [...files.slice(index + 1), ...files.slice(0, index)].find(
        (file) => !isReviewed(file),
      );
      if (next) void selectFile(next, complete);
    } catch (cause) {
      if (fileGeneration.current.isCurrent(ticket)) setError(message(cause));
    }
  }
  function browseHistory(direction: number) {
    const next = history.index + direction;
    const file = history.files[next];
    if (file) {
      setHistory({ ...history, index: next });
      void selectFile(file, manifest, true);
    }
  }
  function activateWorkspace(
    session: WorkspaceSession,
    preferredRepository?: string,
    ticket?: number,
  ) {
    if (ticket !== undefined && !captureGeneration.current.isCurrent(ticket)) return;
    // A ticket denotes a restore or capture. Switching repository tabs stays within the snapshot.
    if (ticket !== undefined || session.activeSnapshotId !== workspaceSession?.activeSnapshotId) {
      fileCache.clear();
      repositoryViews.current.clear();
    } else if (repositoryId && manifest && selectedFile) {
      repositoryViews.current.set(repositoryId, { manifest, file: selectedFile });
    }
    beginComparisonOperation();
    const snapshot = session.snapshots.find((item) => item.id === session.activeSnapshotId);
    const selected =
      snapshot?.repositories.find((repo) => repo.repositoryId === preferredRepository) ??
      snapshot?.repositories.find((repo) => repo.manifest?.totalFiles) ??
      snapshot?.repositories.find((repo) => repo.manifest) ??
      snapshot?.repositories[0];
    setWorkspaceSession(session);
    setRepositoryId(selected?.repositoryId ?? null);
    rememberWorkspace(session.config.id, selected?.repositoryId ?? null);
    const previous = selected && repositoryViews.current.get(selected.repositoryId);
    const visibleManifest = previous?.manifest ?? selected?.manifest ?? null;
    const nextFile = previous?.file ?? visibleManifest?.files[0];
    setManifest(visibleManifest);
    resetComparisonView();
    setError(selected?.error ? message(selected.error) : null);
    const config = session.config.repositories.find((repo) => repo.id === selected?.repositoryId);
    setAppliedSpec(config?.spec ?? null);
    setDraft(null);
    setShowCompareSettings(false);
    if (config) settings.update((current) => ({ ...current, spec: config.spec }));
    if (nextFile && visibleManifest) void selectFile(nextFile, visibleManifest);
  }
  function enterWorkspace(session: WorkspaceSession, ticket?: number) {
    if (ticket !== undefined && !captureGeneration.current.isCurrent(ticket)) return;
    const last = lastViewedWorkspace();
    const preferred = session.config.id === last?.id ? last.repositoryId : null;
    activateWorkspace(session, preferred ?? undefined, ticket);
    setWorkspacePage(session.activeSnapshotId ? "review" : "settings");
    setShowPreferences(false);
    if (session.activeSnapshotId)
      requestAnimationFrame(() =>
        workspaceTabs.current
          ?.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')
          ?.focus(),
      );
  }
  function updateWorkspace(session: WorkspaceSession) {
    setWorkspaceSession((current) =>
      current?.config.id === session.config.id &&
      current.activeSnapshotId === session.activeSnapshotId
        ? session
        : current,
    );
  }
  async function refreshComparison() {
    if (!workspaceSession) {
      if (appliedSpec) await open(appliedSpec, true);
      return;
    }
    if (capturing || workspaceBusy) return;
    const ticket = beginComparisonOperation();
    const job = jobId();
    captureJob.current = job;
    setCapturing(true);
    try {
      const session = await workspaceClient.capture(
        workspaceSession.config,
        job,
        workspaceSession.activeSnapshotId,
      );
      if (!captureGeneration.current.isCurrent(ticket)) return;
      captureJob.current = null;
      activateWorkspace(session, repositoryId ?? undefined, ticket);
    } catch (cause) {
      if (captureGeneration.current.isCurrent(ticket)) {
        captureJob.current = null;
        setCapturing(false);
        setError(message(cause));
      }
    }
  }
  function currentBasis(file: FileSummary, basis: string): EvidenceBasis | null {
    if (!workspaceSession?.activeSnapshotId || !repositoryId || !manifest) return null;
    const separator = basis.indexOf(":");
    return {
      workspaceSnapshotId: workspaceSession.activeSnapshotId,
      repositoryId,
      repositorySnapshotId: manifest.id,
      fileId: file.id,
      beforeId: file.beforeId,
      afterId: file.afterId,
      schemaVersion: separator < 0 ? 1 : Number(basis.slice(0, separator)),
      ruleVersion: separator < 0 ? basis : basis.slice(separator + 1),
    };
  }
  async function markReviewed() {
    if (!manifest || !selectedFile || !data) return;
    if (workspaceSession && repositoryId) {
      const expected = currentBasis(selectedFile, analysisBasis(data));
      if (!expected) return;
      const wasReviewed = workspaceSession.reviews.some(
        (record) => record.state === "reviewed" && sameBasis(record.basis, expected),
      );
      setSavingReview(true);
      const generation = captureGeneration.current.ticket();
      try {
        updateWorkspace(
          await workspaceClient.review(
            workspaceSession.config.id,
            expected,
            wasReviewed ? "pending" : "reviewed",
          ),
        );
      } catch (cause) {
        if (captureGeneration.current.isCurrent(generation)) setError(message(cause));
      } finally {
        setSavingReview(false);
      }
      return;
    }
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
        editComparison(true);
        return;
      }
      if (validationOpen || workspacePage !== "review" || showCompareSettings || !manifest) {
        if (
          e.key === "Escape" &&
          showCompareSettings &&
          manifest &&
          !capturing &&
          !e.defaultPrevented &&
          !document.querySelector(".revision-menu")
        )
          backToReview();
        return;
      }
      if ((e.metaKey || e.ctrlKey) && e.key === "p") {
        e.preventDefault();
        setShowFiles(true);
        requestAnimationFrame(() => searchInput.current?.focus());
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

  const isReviewed = (file: FileSummary) => {
    if (!manifest || !reviewBases[file.id]) return false;
    if (workspaceSession) {
      const basis = currentBasis(file, reviewBases[file.id]);
      return (
        !!basis &&
        workspaceSession.reviews.some(
          (record) => record.state === "reviewed" && sameBasis(record.basis, basis),
        )
      );
    }
    return reviews.includes(reviewKey(manifest, file, reviewBases[file.id]));
  };
  const activeWorkspace = workspaceSession?.snapshots.find(
    (item) => item.id === workspaceSession.activeSnapshotId,
  );
  const activeRepository = activeWorkspace?.repositories.find(
    (repo) => repo.repositoryId === repositoryId,
  );
  const setupVisible =
    !validationOpen &&
    workspacePage === "review" &&
    (showCompareSettings || (!manifest && !workspaceSession));
  const reviewing =
    !!manifest &&
    !validationOpen &&
    !setupVisible &&
    (workspacePage === "review" || workspacePage === "settings");
  const sources = manifest
    ? diffSources(
        manifest,
        workspaceSession
          ? activeRepository?.revisions
          : appliedSpec
            ? comparisonSources(appliedSpec)
            : undefined,
      )
    : null;
  const reviewed = selectedFile ? isReviewed(selectedFile) : false;
  return (
    <main
      className="app-shell review-shell"
      style={{ "--navigation-width": `${navigationWidth}px` } as CSSProperties}
    >
      <header className="app-header">
        <div className="brand">
          <span className="mark">↔</span>
          <h1>
            Diffkin <span>观变</span>
          </h1>
          <span className="alpha">ALPHA</span>
        </div>
        <div className="header-actions">
          <button
            aria-pressed={
              !validationOpen &&
              (workspacePage !== "review" || (!!workspaceSession && !setupVisible))
            }
            disabled={workspaceBusy || capturing}
            onClick={() => {
              setValidationOpen(false);
              setShowPreferences(false);
              setWorkspacePage("home");
            }}
          >
            工作区
          </button>
          <button
            aria-pressed={
              !validationOpen && workspacePage === "review" && (!workspaceSession || setupVisible)
            }
            disabled={workspaceBusy || capturing}
            onClick={() => {
              if (!workspaceSession && manifest && !showCompareSettings) {
                setWorkspacePage("review");
                setValidationOpen(false);
              } else editComparison();
            }}
          >
            单仓库比较
          </button>
          <button
            aria-pressed={validationOpen}
            disabled={!available || workspaceBusy || capturing}
            onClick={() => {
              setValidationMounted(true);
              setValidationOpen(true);
            }}
          >
            验证
          </button>
          <button
            aria-pressed={showPreferences}
            onClick={() => setShowPreferences(!showPreferences)}
          >
            {settings.failure ? "设置 · 未保存" : "设置"}
          </button>
          <span className="local-badge">
            <i /> 本地 · 只读审查
          </span>
        </div>
      </header>
      {showPreferences && (
        <PreferencesPanel
          preferences={settings.preferences}
          onChange={settings.update}
          status={settings.status}
          failure={settings.failure}
          onSave={settings.saveExplicitly}
          onClose={() => setShowPreferences(false)}
          available={available}
        />
      )}
      {workspacePage === "home" && !validationOpen && (
        <WorkspaceHome
          available={available}
          onCreate={() => setWorkspacePage("create")}
          onBegin={beginComparisonOperation}
          onOpen={enterWorkspace}
        />
      )}
      {activeWorkspace &&
        !validationOpen &&
        !setupVisible &&
        (workspacePage === "review" || workspacePage === "settings") && (
          <nav ref={workspaceTabs} className="workspace-tabs" aria-label="工作区仓库">
            <strong>{workspaceSession?.config.name}</strong>
            {activeWorkspace.repositories.map((repo) => (
              <button
                key={repo.repositoryId}
                className={repo.error ? "repository-error" : ""}
                aria-pressed={repositoryId === repo.repositoryId}
                onClick={() => activateWorkspace(workspaceSession!, repo.repositoryId)}
              >
                {repo.label}
                <small>
                  {repo.error
                    ? "不可用"
                    : repo.manifest?.totalFiles
                      ? `${repo.manifest.totalFiles} 个变化`
                      : "未修改 · 上下文"}
                </small>
              </button>
            ))}
            <button
              className="workspace-settings"
              disabled={workspaceBusy || capturing}
              aria-expanded={showWorkspace}
              onClick={() => setWorkspacePage(showWorkspace ? "review" : "settings")}
            >
              工作区设置
            </button>
          </nav>
        )}
      {reviewing && (
        <div className="review-context" aria-label="当前比较">
          <button
            ref={filesButton}
            aria-expanded={showFiles}
            aria-controls="review-files"
            onClick={() => setShowFiles(!showFiles)}
          >
            变更文件 · {manifest.totalFiles}
          </button>
          <strong title={manifest.repository}>
            {manifest.repository.split(/[\\/]/).filter(Boolean).at(-1)}
          </strong>
          <span>{modeNames[manifest.mode]}</span>
          <code title={`${manifest.baseLabel} → ${manifest.headLabel}`}>
            {sources?.map((side) => side.name).join(" → ")}
            {!workspaceSession && appliedSpec?.pathScope && ` · ${appliedSpec.pathScope}`}
          </code>
          {!workspaceSession && (
            <button
              ref={editButton}
              disabled={capturing || workspaceBusy}
              aria-expanded={showCompareSettings}
              aria-controls="comparison-settings"
              onClick={() => editComparison()}
            >
              更改比较
            </button>
          )}
          <button
            disabled={pending === "capture" || workspaceBusy}
            onClick={() => void refreshComparison()}
          >
            {workspaceSession ? "刷新工作区" : "刷新比较"}
          </button>
          {pending && <button onClick={cancel}>取消</button>}
          <button
            disabled={history.index <= 0}
            onClick={() => browseHistory(-1)}
            aria-label="浏览历史后退"
          >
            ←
          </button>
          <button
            disabled={history.index >= history.files.length - 1}
            onClick={() => browseHistory(1)}
            aria-label="浏览历史前进"
          >
            →
          </button>
          <button onClick={() => void nextUnreviewed()}>下个未审查</button>
          <button
            aria-expanded={showDetails}
            aria-controls="review-details"
            onClick={() => {
              setAiPanel(null);
              setShowDetails(!showDetails);
            }}
          >
            变更详情
          </button>
        </div>
      )}
      {setupVisible && (
        <ComparisonSetup
          spec={spec}
          recent={recent}
          available={available}
          busy={capturing}
          editing={!!manifest}
          error={error}
          recoverScope={recoverScope}
          pathInput={pathInput}
          pickerButton={pickerButton}
          onChange={setSpec}
          onRepository={changeRepository}
          onSubmit={() => void open(spec)}
          onCancel={cancel}
          onBack={backToReview}
        />
      )}
      {showWorkspace && !validationOpen && (
        <WorkspacePanel
          available={available}
          currentSpec={spec}
          session={workspacePage === "create" ? null : workspaceSession}
          repositoryId={repositoryId}
          selectedFile={selectedFile}
          onActivate={enterWorkspace}
          onBegin={beginComparisonOperation}
          onUpdate={updateWorkspace}
          onClose={() =>
            setWorkspacePage(workspacePage === "create" || !activeWorkspace ? "home" : "review")
          }
          onBusyChange={setWorkspaceBusy}
        />
      )}
      {!available && (
        <p className="notice">浏览器仅预览界面。请运行桌面应用以读取本地 Git 仓库。</p>
      )}
      {error &&
        !setupVisible &&
        !validationOpen &&
        workspacePage !== "home" &&
        workspacePage !== "create" && (
          <p className="error" role="alert">
            {error}
            {activeRepository?.error?.code === "budget-exceeded" && workspacePage === "review" && (
              <button
                onClick={() => {
                  setWorkspacePage("settings");
                  requestAnimationFrame(() =>
                    document.getElementById(`scope-${repositoryId}`)?.focus(),
                  );
                }}
              >
                调整此仓库的路径范围
              </button>
            )}
          </p>
        )}
      {reviewing && (
        <CaptureNotices
          key={`${manifest.repository}:${manifest.id}`}
          warnings={manifest.warnings}
        />
      )}
      <div
        className={`workspace ${!manifest ? "unopened" : ""}`}
        aria-busy={pending !== null}
        hidden={!reviewing}
      >
        <nav id="review-files" hidden={!showFiles} className="files-panel" aria-label="变更文件">
          <div className="panel-heading">
            <h2>变更文件</h2>
            <span>{manifest?.totalFiles ?? 0}</span>
          </div>
          <input
            ref={searchInput}
            className="file-search"
            aria-label="筛选文件"
            placeholder="搜索全部变更路径… ⌘/Ctrl P"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          {catalogBusy && needsCatalog && (
            <p className="navigation-scope" role="status">
              正在搜索完整变更清单…
            </p>
          )}
          {catalogError && needsCatalog && (
            <p className="navigation-scope" role="alert">
              完整搜索失败：{catalogError}。清空搜索后可继续分页浏览。
            </p>
          )}
          <ChangeNavigator
            files={visibleFiles}
            visible={showFiles && reviewing}
            snapshotId={manifest?.id}
            selected={selectedFile?.id}
            query={filter}
            onSelect={(file) => {
              void selectFile(file);
              if (window.matchMedia("(max-width: 1000px)").matches) {
                setShowFiles(false);
                filesButton.current?.focus();
              }
            }}
            reviewed={isReviewed}
          />
          {manifest && visibleFiles.length === 0 && !needsCatalog && (
            <p className="muted">{manifest.totalFiles ? "没有匹配的路径" : "没有变化"}</p>
          )}
          {manifest && manifest.files.length < manifest.totalFiles && (
            <button className="load-more" disabled={paging} onClick={() => void more()}>
              {paging ? "加载中…" : `加载更多（${manifest.files.length}/${manifest.totalFiles}）`}
            </button>
          )}
          <div className="file-help">J / K 切换文件 · ⌘/Ctrl O 打开</div>
        </nav>
        {showFiles && (
          <div
            role="separator"
            className="navigation-resizer"
            aria-label="文件导航宽度"
            aria-orientation="vertical"
            aria-valuemin={200}
            aria-valuemax={420}
            aria-valuenow={navigationWidth}
            tabIndex={0}
            onKeyDown={(event) => {
              if (["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                event.preventDefault();
                resizeNavigation(
                  event.key === "Home"
                    ? 200
                    : event.key === "End"
                      ? 420
                      : navigationWidth + (event.key === "ArrowRight" ? 20 : -20),
                );
              }
            }}
            onPointerDown={(event) => {
              event.currentTarget.setPointerCapture(event.pointerId);
            }}
            onPointerMove={(event) => {
              if (event.currentTarget.hasPointerCapture(event.pointerId))
                resizeNavigation(event.clientX);
            }}
            onPointerUp={(event) => {
              if (event.currentTarget.hasPointerCapture(event.pointerId))
                event.currentTarget.releasePointerCapture(event.pointerId);
            }}
          />
        )}
        <section className="comparison-panel">
          {selectedFile ? (
            <>
              <div className="file-heading">
                <div>
                  <h2 title={selectedFile.path}>
                    <span className="heading-parent">
                      {selectedFile.path.slice(0, selectedFile.path.lastIndexOf("/") + 1)}
                    </span>
                    <span className="heading-leaf">
                      {selectedFile.path.slice(selectedFile.path.lastIndexOf("/") + 1)}
                    </span>
                  </h2>
                  <p>
                    {selectedFile.oldMode ?? "∅"} → {selectedFile.newMode ?? "∅"}
                    {data?.analysis
                      ? ` · +${data.analysis.raw.added} −${data.analysis.raw.removed}`
                      : ""}
                  </p>
                </div>
                <div className="file-actions">
                  <button
                    disabled={!analysisReady || !data?.analysis}
                    aria-expanded={aiPanel === "explain"}
                    onClick={() => {
                      setShowDetails(false);
                      setAiPanel(aiPanel === "explain" ? null : "explain");
                    }}
                  >
                    AI 解释
                  </button>
                  <button
                    disabled={
                      !analysisReady ||
                      !data?.analysis ||
                      !(data.analysis.raw.added + data.analysis.raw.removed)
                    }
                    aria-expanded={aiPanel === "quiz"}
                    onClick={() => {
                      setShowDetails(false);
                      setAiPanel(aiPanel === "quiz" ? null : "quiz");
                    }}
                  >
                    变更测验
                  </button>
                  <button
                    disabled={!data || !analysisReady || savingReview}
                    className={reviewed ? "reviewed" : ""}
                    aria-pressed={reviewed}
                    onClick={() => void markReviewed()}
                  >
                    {reviewed ? "✓ 已审查此版本" : "标记已审查"}
                  </button>
                </div>
              </div>
              {selectedFile.notices.map((notice, index) => (
                <p className="notice compact" key={index}>
                  {notice}
                </p>
              ))}
              <details
                className="source-identities"
                hidden={!showDetails && !(data?.file.before === null && data?.file.after === null)}
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
              ) : data && sources && needsSegments(data.file) ? (
                <LargeFileViewer
                  key={`${manifest?.id}:${selectedFile.id}`}
                  file={data.file}
                  sources={sources}
                />
              ) : data && sources ? (
                <DiffViewer
                  sources={sources}
                  viewKey={`${manifest?.repository}:${selectedFile.path}`}
                  before={data.file.before ?? ""}
                  after={data.file.after ?? ""}
                  path={selectedFile.path}
                  event={event}
                  scope={scope}
                  analysis={data.analysis}
                  foldFormats={foldFormats}
                  onFoldChange={setFoldFormats}
                  layout={view.layout}
                  collapse={view.collapseUnchanged}
                  onLayoutChange={(layout) =>
                    settings.update((current) => ({
                      ...current,
                      view: { ...current.view, layout },
                    }))
                  }
                  onCollapseChange={(collapseUnchanged) =>
                    settings.update((current) => ({
                      ...current,
                      view: { ...current.view, collapseUnchanged },
                    }))
                  }
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
              <h2>当前比较没有文件变化</h2>
              <p>可更改比较内容，或刷新读取新的改动。</p>
            </div>
          )}
        </section>
        {aiPanel && !validationOpen && data?.analysis && manifest && selectedFile && (
          <ExplanationPanel
            key={`${aiPanel}:${manifest.id}:${selectedFile.id}:${aiPanel === "explain" ? (event?.id ?? "") : ""}`}
            mode={aiPanel}
            comparisonId={manifest.id}
            fileId={selectedFile.id}
            analysis={data.analysis}
            event={event}
            onClose={() => setAiPanel(null)}
          />
        )}
        {data?.analysis && (
          <div id="review-details" className="review-details" hidden={!showDetails}>
            <button className="close-details" onClick={() => setShowDetails(false)}>
              收起详情
            </button>
            <ChangeInspector
              key={`${manifest?.id}:${selectedFile?.id}`}
              result={data.analysis}
              selected={event}
              fold={foldFormats}
              onFoldChange={setFoldFormats}
              onSelect={(selected) => {
                setEvent(selected);
                setScope(
                  !!selected &&
                    !!data.analysis?.events.some((parent) => parent.children.includes(selected.id)),
                );
              }}
            />
          </div>
        )}
      </div>
      {validationMounted && (
        <div className="validation-container" hidden={!validationOpen}>
          <ValidationPanel
            repository={manifest?.repository ?? spec.repository}
            manifest={manifest}
            onRepository={changeRepository}
          />
        </div>
      )}
      <footer>
        <span role="status">
          {cancelled
            ? "已取消"
            : pending === "capture"
              ? manifest
                ? "正在刷新，当前显示上一次比较"
                : "正在捕获仓库快照…"
              : pending === "file"
                ? "计算文本与结构变化…"
                : "就绪"}
        </span>
        <span>
          {manifest
            ? `${manifest.files.filter(isReviewed).length} / ${manifest.totalFiles} 已审查 · `
            : ""}
          源码留在本机 · 比较不执行脚本
        </span>
      </footer>
    </main>
  );
}

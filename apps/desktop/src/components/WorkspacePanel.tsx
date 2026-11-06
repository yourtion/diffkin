import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ComparisonSpec,
  FileSummary,
  WorkspaceConfig,
  WorkspaceRepository,
  WorkspaceSession,
} from "@diffkin/protocol";
import { client, message } from "../lib/client";
import { workspaceClient } from "../lib/workspace-client";
import "./workspace.css";
import { RepositoryPicker } from "./RepositoryPicker";
import { RevisionPair } from "./RevisionPair";
import { modeNames } from "../lib/comparison-direction";

const identifier = () => crypto.randomUUID();
function repository(spec: ComparisonSpec): WorkspaceRepository {
  return {
    id: identifier(),
    label: spec.repository.split(/[\\/]/).filter(Boolean).at(-1) || "仓库",
    spec: { ...spec },
  };
}
export function WorkspacePanel({
  available,
  currentSpec,
  session,
  repositoryId,
  selectedFile,
  onActivate,
  onBegin,
  onUpdate,
  onClose,
  onBusyChange,
}: {
  available: boolean;
  currentSpec: ComparisonSpec;
  session: WorkspaceSession | null;
  repositoryId: string | null;
  selectedFile: FileSummary | null;
  onActivate: (session: WorkspaceSession, ticket: number) => void;
  onBegin: () => number;
  onUpdate: (session: WorkspaceSession) => void;
  onClose: () => void;
  onBusyChange: (busy: boolean) => void;
}) {
  const [config, setConfig] = useState<WorkspaceConfig>(
    () =>
      session?.config ?? {
        id: identifier(),
        name: "我的工作区",
        repositories: [repository(currentSpec)],
      },
  );
  const [busy, setBusy] = useState(false);
  const [capturing, setCapturing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState("");
  const [targetRepo, setTargetRepo] = useState("");
  const [targetFile, setTargetFile] = useState("");
  const [targetPages, setTargetPages] = useState<Record<string, FileSummary[]>>({});
  const [note, setNote] = useState("");
  const job = useRef<string | null>(null);
  const generation = useRef(0);
  const operation = useRef(0);
  useEffect(() => {
    onBusyChange(busy);
    return () => onBusyChange(false);
  }, [busy, onBusyChange]);
  const snapshot = session?.snapshots.find((item) => item.id === session.activeSnapshotId);
  const destination = snapshot?.repositories.find((item) => item.repositoryId === targetRepo);
  const targetFiles = destination?.manifest
    ? (targetPages[destination.manifest.id] ?? destination.manifest.files)
    : [];
  const stopCapture = useCallback(() => {
    generation.current++;
    operation.current++;
    if (job.current) void client.cancel(job.current).catch(() => {});
    job.current = null;
  }, []);
  useEffect(() => stopCapture, [stopCapture]);
  async function action(task: () => Promise<void>) {
    const current = ++operation.current;
    setBusy(true);
    setError(null);
    setNotice("");
    try {
      await task();
    } catch (cause) {
      if (operation.current === current) setError(message(cause));
    } finally {
      if (operation.current === current) setBusy(false);
    }
  }
  function updateRepo(id: string, update: Partial<WorkspaceRepository>) {
    setConfig((current) => ({
      ...current,
      repositories: current.repositories.map((item) =>
        item.id === id ? { ...item, ...update } : item,
      ),
    }));
  }
  function changePath(repo: WorkspaceRepository, path: string) {
    const name = (value: string) => value.split(/[\\/]/).filter(Boolean).at(-1) || "仓库";
    updateRepo(repo.id, {
      label: repo.label === name(repo.spec.repository) ? name(path) : repo.label,
      spec: { ...repo.spec, repository: path },
    });
  }
  async function capture() {
    const ticket = onBegin();
    const current = ++generation.current;
    const token = `workspace-${identifier()}`;
    job.current = token;
    setCapturing(true);
    await action(async () => {
      const result = await workspaceClient.capture(config, token);
      if (generation.current !== current) return;
      onActivate(result, ticket);
    });
    if (job.current === token) {
      job.current = null;
      setCapturing(false);
    }
  }
  async function restore(id: string, snapshotId: string | null = null) {
    const ticket = onBegin();
    await action(async () => {
      const result = await workspaceClient.restore(id, snapshotId);
      onActivate(result, ticket);
    });
  }
  async function exportManifest() {
    if (!session) return;
    await action(async () => {
      const result = await workspaceClient.export(session.config.id, session.activeSnapshotId);
      if (result.snapshotId !== session.activeSnapshotId)
        throw new Error("工作区快照已切换，请重新打开工作区并确认要导出的版本。");
      const url = URL.createObjectURL(
        new Blob([JSON.stringify(result, null, 2)], { type: "application/json" }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = `diffkin-workspace-${session.config.id}.json`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
      setNotice("已导出版本身份与审查记录，未包含源码、绝对路径、备注或凭据。");
    });
  }
  async function moreTargets() {
    const manifest = destination?.manifest;
    if (!manifest) return;
    await action(async () => {
      const page = await client.page(manifest.id, targetFiles.length);
      setTargetPages((current) => {
        const previous = current[manifest.id] ?? manifest.files;
        return {
          ...current,
          [manifest.id]: [
            ...previous,
            ...page.files.filter((file) => !previous.some((item) => item.id === file.id)),
          ],
        };
      });
    });
  }
  async function relate() {
    const snapshotId = session?.activeSnapshotId;
    if (!session || !snapshotId || !repositoryId || !selectedFile || !targetRepo || !targetFile)
      return;
    await action(async () => {
      const result = await workspaceClient.linkFiles(session.config.id, {
        id: identifier(),
        workspaceSnapshotId: snapshotId,
        from: { repositoryId, fileId: selectedFile.id },
        to: { repositoryId: targetRepo, fileId: targetFile },
        note,
      });
      onUpdate(result);
      setNote("");
      setNotice("手工关联已保存，并绑定两侧的快照与分析依据。");
    });
  }
  const describe = (repoId: string, fileId: string, snapshotId: string) => {
    const repo = session?.snapshots
      .find((item) => item.id === snapshotId)
      ?.repositories.find((item) => item.repositoryId === repoId);
    return `${repo?.label ?? repoId} / ${repo?.manifest?.files.find((item) => item.id === fileId)?.path ?? fileId.slice(0, 12)}`;
  };
  return (
    <section
      className={
        session?.activeSnapshotId ? "workspace-manager" : "workspace-manager workspace-editor"
      }
      aria-label="多仓库工作区"
    >
      <div className="workspace-manager-heading">
        <h2>
          {session?.activeSnapshotId ? "工作区设置" : session ? session.config.name : "新建工作区"}
        </h2>
        <button onClick={onClose} disabled={busy}>
          {session?.activeSnapshotId ? "收起设置" : "返回工作区"}
        </button>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {notice && <output className="notice">{notice}</output>}
      <div className="workspace-actions">
        <label>
          工作区名称{" "}
          <input
            aria-label="工作区名称"
            value={config.name}
            onChange={(e) => setConfig({ ...config, name: e.target.value })}
            disabled={busy}
          />
        </label>
      </div>
      <div className="repository-configs">
        {config.repositories.map((repo, index) => (
          <fieldset className="repository-config" key={repo.id} disabled={busy}>
            <legend>仓库 {index + 1}</legend>
            <label>
              显示名称
              <input
                aria-label={`仓库 ${index + 1} 名称`}
                value={repo.label}
                onChange={(e) => updateRepo(repo.id, { label: e.target.value })}
              />
            </label>
            <label className="workspace-path">
              本地仓库
              <input
                className="workspace-path"
                aria-label={`仓库 ${index + 1} 路径`}
                placeholder="本地 Git 仓库路径"
                value={repo.spec.repository}
                onChange={(e) => changePath(repo, e.target.value)}
              />
            </label>
            <RepositoryPicker
              key={repo.spec.repository}
              value={repo.spec.repository}
              label={`选择仓库 ${index + 1} 目录`}
              disabled={!available || busy}
              onChoose={(path) => changePath(repo, path)}
            />
            <label>
              比较内容
              <select
                aria-label={`仓库 ${index + 1} 模式`}
                value={repo.spec.mode}
                onChange={(e) =>
                  updateRepo(repo.id, {
                    spec: { ...repo.spec, mode: e.target.value as ComparisonSpec["mode"] },
                  })
                }
              >
                {Object.entries(modeNames).map(([value, label]) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
            {repo.spec.mode === "commits" && (
              <RevisionPair
                spec={repo.spec}
                repositoryLabel={`仓库 ${index + 1}`}
                disabled={!available || busy}
                onChange={(spec) => updateRepo(repo.id, { spec })}
              />
            )}
            {(repo.spec.mode === "unstaged" || repo.spec.mode === "worktree") && (
              <label className="check">
                <input
                  type="checkbox"
                  checked={repo.spec.includeUntracked}
                  onChange={(e) =>
                    updateRepo(repo.id, {
                      spec: { ...repo.spec, includeUntracked: e.target.checked },
                    })
                  }
                />
                纳入未跟踪
              </label>
            )}
            {(repo.spec.mode === "worktree" || repo.spec.mode === "unstaged") && (
              <label className="check">
                <input
                  type="checkbox"
                  checked={repo.spec.verifyAll === true}
                  onChange={(event) =>
                    updateRepo(repo.id, { spec: { ...repo.spec, verifyAll: event.target.checked } })
                  }
                />
                完整原文核验
              </label>
            )}
            <label className="workspace-scope">
              路径范围
              <input
                id={`scope-${repo.id}`}
                aria-label={`仓库 ${index + 1} 路径范围`}
                placeholder="整个仓库"
                value={repo.spec.pathScope ?? ""}
                onChange={(e) =>
                  updateRepo(repo.id, {
                    spec: { ...repo.spec, pathScope: e.target.value || undefined },
                  })
                }
              />
            </label>
            <button
              disabled={config.repositories.length <= 1}
              onClick={() =>
                setConfig({
                  ...config,
                  repositories: config.repositories.filter((item) => item.id !== repo.id),
                })
              }
            >
              移除仓库 {index + 1}
            </button>
          </fieldset>
        ))}
      </div>
      <div className="workspace-actions">
        <button
          disabled={busy || config.repositories.length >= 8}
          onClick={() =>
            setConfig({
              ...config,
              repositories: [
                ...config.repositories,
                repository({ ...currentSpec, repository: "" }),
              ],
            })
          }
        >
          关联另一个仓库
        </button>
        <button
          disabled={
            busy ||
            !available ||
            !config.name.trim() ||
            config.repositories.some((repo) => !repo.spec.repository.trim())
          }
          onClick={() =>
            void action(async () => {
              const result = await workspaceClient.save(config);
              onUpdate(result);
              if (!session) onClose();
              setNotice("配置已保存；捕获工作区后读取这些仓库的改动。");
            })
          }
        >
          保存配置
        </button>
        <button
          className="primary"
          disabled={
            busy ||
            !available ||
            !config.name.trim() ||
            config.repositories.some((repo) => !repo.spec.repository.trim())
          }
          onClick={() => void capture()}
        >
          {busy ? "处理中…" : "比较工作区"}
        </button>
        {busy && capturing && (
          <button
            onClick={() => {
              stopCapture();
              setCapturing(false);
              setBusy(false);
              setNotice("已请求取消工作区捕获。");
            }}
          >
            取消工作区捕获
          </button>
        )}
        <button disabled={busy || !session} onClick={() => void exportManifest()}>
          导出版本清单
        </button>
        {session && (
          <select
            aria-label="工作区历史快照"
            value={session.activeSnapshotId ?? ""}
            disabled={busy}
            onChange={(e) => void restore(session.config.id, e.target.value)}
          >
            <option value="">尚未捕获</option>
            {session.snapshots.map((item) => (
              <option key={item.id} value={item.id}>
                {new Date(Number(item.capturedAtMs)).toLocaleString()} · {item.id.slice(0, 8)}
              </option>
            ))}
          </select>
        )}
      </div>
      {session && (
        <details className="manual-relations">
          <summary>手工关联 · {session.relations.length} 条</summary>
          <p>
            当前来源：
            {snapshot?.repositories.find((repo) => repo.repositoryId === repositoryId)?.label ??
              "未选择"}{" "}
            / {selectedFile?.path ?? "请先选择文件"}
          </p>
          <div className="workspace-actions">
            <select
              aria-label="关联目标仓库"
              value={targetRepo}
              onChange={(e) => {
                setTargetRepo(e.target.value);
                setTargetFile("");
              }}
            >
              <option value="">选择目标仓库</option>
              {snapshot?.repositories
                .filter((repo) => repo.manifest)
                .map((repo) => (
                  <option key={repo.repositoryId} value={repo.repositoryId}>
                    {repo.label}
                  </option>
                ))}
            </select>
            <select
              aria-label="关联目标文件"
              value={targetFile}
              onChange={(e) => setTargetFile(e.target.value)}
            >
              <option value="">选择目标文件</option>
              {targetFiles.map((file) => (
                <option key={file.id} value={file.id}>
                  {file.path}
                </option>
              ))}
            </select>
            {destination?.manifest && targetFiles.length < destination.manifest.totalFiles && (
              <button disabled={busy} onClick={() => void moreTargets()}>
                加载更多目标文件（{targetFiles.length}/{destination.manifest.totalFiles}）
              </button>
            )}
            <input
              aria-label="关联备注"
              placeholder="关联原因（仅保存在本机）"
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
            <button disabled={busy || !selectedFile || !targetFile} onClick={() => void relate()}>
              保存手工关联
            </button>
          </div>
          {session.relations.map((relation) => (
            <p key={relation.id}>
              {describe(
                relation.from.repositoryId,
                relation.from.fileId,
                relation.from.workspaceSnapshotId,
              )}{" "}
              →{" "}
              {describe(
                relation.to.repositoryId,
                relation.to.fileId,
                relation.to.workspaceSnapshotId,
              )}{" "}
              ·{" "}
              {relation.from.workspaceSnapshotId === session.activeSnapshotId &&
              relation.to.workspaceSnapshotId === session.activeSnapshotId
                ? "当前快照关系"
                : "历史关系，待重新确认"}
              {relation.note ? ` · ${relation.note}` : ""}
            </p>
          ))}
        </details>
      )}
    </section>
  );
}

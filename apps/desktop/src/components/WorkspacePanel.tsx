import { useCallback, useEffect, useRef, useState } from "react";
import type {
  ComparisonSpec,
  FileSummary,
  WorkspaceConfig,
  WorkspaceRepository,
  WorkspaceSession,
  WorkspaceSummary,
} from "@diffkin/protocol";
import { client, message } from "../lib/client";
import { workspaceClient } from "../lib/workspace-client";
import "./workspace.css";
import { RepositoryPicker } from "./RepositoryPicker";
import { RevisionPicker } from "./RevisionPicker";

const modes = {
  worktree: "全部本地改动",
  staged: "已暂存",
  unstaged: "未暂存",
  commits: "分支 / 提交",
};
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
  onActivate: (session: WorkspaceSession, repoId?: string, ticket?: number) => void;
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
  const [saved, setSaved] = useState<WorkspaceSummary[]>([]);
  const [savedId, setSavedId] = useState("");
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
  useEffect(() => {
    let active = true;
    if (available)
      void workspaceClient
        .list()
        .then((items) => {
          if (active) setSaved(items);
        })
        .catch((cause) => {
          if (active) setError(message(cause));
        });
    return () => {
      active = false;
      stopCapture();
    };
  }, [available, stopCapture]);
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
  async function capture() {
    const ticket = onBegin();
    const current = ++generation.current;
    const token = `workspace-${identifier()}`;
    job.current = token;
    setCapturing(true);
    await action(async () => {
      const result = await workspaceClient.capture(config, token);
      if (generation.current !== current) return;
      onActivate(result, undefined, ticket);
      setTargetRepo("");
      setTargetFile("");
      setTargetPages({});
      setSaved(await workspaceClient.list());
      setNotice("工作区快照已固定；各仓库独立绑定基线与候选。未修改仓库保留为上下文。");
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
      setConfig(result.config);
      onActivate(result, undefined, ticket);
      setTargetRepo("");
      setTargetFile("");
      setTargetPages({});
      setNotice("已恢复本机保存的冻结证据，没有重新读取活动源码。");
    });
  }
  async function exportManifest() {
    if (!session) return;
    await action(async () => {
      const result = await workspaceClient.export(session.config.id);
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
    if (!session || !repositoryId || !selectedFile || !targetRepo || !targetFile) return;
    await action(async () => {
      const [from, to] = await Promise.all([
        workspaceClient.basis(session.config.id, repositoryId, selectedFile.id),
        workspaceClient.basis(session.config.id, targetRepo, targetFile),
      ]);
      const result = await workspaceClient.relation(session.config.id, {
        id: identifier(),
        from,
        to,
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
    <section className="workspace-manager" aria-label="多仓库工作区">
      <div className="workspace-manager-heading">
        <h2>多仓库工作区</h2>
        <button onClick={onClose} disabled={busy}>
          收起设置
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
        <button
          disabled={busy}
          onClick={() => {
            setConfig({
              id: identifier(),
              name: "新工作区",
              repositories: [repository(currentSpec)],
            });
            setNotice("编辑新工作区；捕获后才替换当前审阅。");
          }}
        >
          新建工作区
        </button>
        <select
          aria-label="已保存工作区"
          value={savedId}
          onChange={(e) => setSavedId(e.target.value)}
        >
          <option value="">选择已保存工作区</option>
          {saved.map((item) => (
            <option value={item.id} key={item.id}>
              {item.name} · {item.repositoryCount} 仓库{item.error ? " · 存档异常" : ""}
            </option>
          ))}
        </select>
        <button disabled={busy || !savedId || !available} onClick={() => void restore(savedId)}>
          恢复工作区
        </button>
      </div>
      <div className="repository-configs">
        {config.repositories.map((repo, index) => (
          <fieldset className="repository-config" key={repo.id} disabled={busy}>
            <legend>仓库 {index + 1}</legend>
            <input
              aria-label={`仓库 ${index + 1} 名称`}
              value={repo.label}
              onChange={(e) => updateRepo(repo.id, { label: e.target.value })}
            />
            <input
              className="workspace-path"
              aria-label={`仓库 ${index + 1} 路径`}
              placeholder="本地 Git 仓库路径"
              value={repo.spec.repository}
              onChange={(e) =>
                updateRepo(repo.id, { spec: { ...repo.spec, repository: e.target.value } })
              }
            />
            <RepositoryPicker
              value={repo.spec.repository}
              label={`选择仓库 ${index + 1} 目录`}
              disabled={!available || busy}
              onChoose={(path) => updateRepo(repo.id, { spec: { ...repo.spec, repository: path } })}
            />
            <select
              aria-label={`仓库 ${index + 1} 模式`}
              value={repo.spec.mode}
              onChange={(e) =>
                updateRepo(repo.id, {
                  spec: { ...repo.spec, mode: e.target.value as ComparisonSpec["mode"] },
                })
              }
            >
              {Object.entries(modes).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
            {repo.spec.mode === "commits" ? (
              <>
                <RevisionPicker
                  repository={repo.spec.repository}
                  label={`仓库 ${index + 1} 基线`}
                  value={repo.spec.base ?? ""}
                  placeholder="HEAD~1"
                  disabled={!available || busy}
                  onChange={(base) => updateRepo(repo.id, { spec: { ...repo.spec, base } })}
                />
                <span>→</span>
                <RevisionPicker
                  repository={repo.spec.repository}
                  label={`仓库 ${index + 1} 候选`}
                  value={repo.spec.head ?? ""}
                  placeholder="HEAD"
                  disabled={!available || busy}
                  onChange={(head) => updateRepo(repo.id, { spec: { ...repo.spec, head } })}
                />
              </>
            ) : (
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
          disabled={busy || !available}
          onClick={() =>
            void action(async () => {
              const result = await workspaceClient.save(config);
              onUpdate(result);
              setSaved(await workspaceClient.list());
              setNotice("配置已保存；捕获工作区后读取这些仓库的改动。");
            })
          }
        >
          保存配置
        </button>
        <button
          className="primary"
          disabled={
            busy || !available || config.repositories.some((repo) => !repo.spec.repository.trim())
          }
          onClick={() => void capture()}
        >
          {busy ? "处理中…" : "捕获工作区"}
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

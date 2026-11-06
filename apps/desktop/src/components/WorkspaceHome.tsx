import { useEffect, useRef, useState } from "react";
import type { WorkspaceSession, WorkspaceSummary } from "@diffkin/protocol";
import { message } from "../lib/client";
import { lastViewedWorkspace, workspaceClient } from "../lib/workspace-client";

export function WorkspaceHome({
  available,
  onCreate,
  onBegin,
  onOpen,
}: {
  available: boolean;
  onCreate: () => void;
  onBegin: () => number;
  onOpen: (session: WorkspaceSession, ticket: number) => void;
}) {
  const [items, setItems] = useState<WorkspaceSummary[] | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [last] = useState(lastViewedWorkspace);
  const mounted = useRef(false);
  useEffect(() => {
    let active = true;
    mounted.current = true;
    if (available)
      void workspaceClient.list().then(
        (saved) => {
          if (active)
            setItems(
              [...saved].sort((a, b) => Number(b.id === last?.id) - Number(a.id === last?.id)),
            );
        },
        (cause) => {
          if (active) setError(message(cause));
        },
      );
    return () => {
      active = false;
      mounted.current = false;
    };
  }, [available, attempt, last]);
  async function open(id: string) {
    if (opening) return;
    const ticket = onBegin();
    setOpening(id);
    setError("");
    try {
      const session = await workspaceClient.restore(id);
      if (mounted.current) onOpen(session, ticket);
    } catch (cause) {
      if (mounted.current) setError(message(cause));
    } finally {
      if (mounted.current) setOpening(null);
    }
  }
  return (
    <section className="setup-screen" aria-labelledby="workspace-home-title">
      <div className="comparison-setup workspace-home">
        <div className="workspace-home-heading">
          <h2 id="workspace-home-title">我的工作区</h2>
          <button
            className={items?.length ? undefined : "primary"}
            disabled={!available || !!opening}
            onClick={onCreate}
          >
            新建工作区
          </button>
        </div>
        {items?.length ? (
          <ul aria-label="已保存工作区">
            {items.map((item) => (
              <li key={item.id}>
                <button disabled={!!opening} onClick={() => void open(item.id)}>
                  <strong>{item.name}</strong>
                  <span>
                    {item.id === last?.id && "上次打开 · "}
                    {item.repositoryCount} 个仓库 ·{" "}
                    {item.snapshotCount ? `${item.snapshotCount} 个快照` : "尚未比较"}
                    {item.error && " · 存档异常"}
                  </span>
                  <b aria-hidden="true">{opening === item.id ? "…" : "→"}</b>
                </button>
              </li>
            ))}
          </ul>
        ) : items ? (
          <p>新建工作区，添加要一起审查的 Git 仓库。</p>
        ) : available && !error ? (
          <output>正在读取已保存的工作区…</output>
        ) : null}
        {error && (
          <p className="error" role="alert">
            {error}{" "}
            {!items && (
              <button
                onClick={() => {
                  setError("");
                  setAttempt(attempt + 1);
                }}
              >
                重试
              </button>
            )}
          </p>
        )}
      </div>
    </section>
  );
}

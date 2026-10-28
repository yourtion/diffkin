import { useCallback, useEffect, useRef, useState } from "react";
import type { RepositoryRefs } from "@diffkin/protocol";
import { client, message } from "../lib/client";
import { navigationClient } from "../lib/navigation-client";
import "./navigation.css";

const kinds = { branch: "本地", remote: "远程", tag: "标签" };
export function RevisionPicker({
  repository,
  value,
  onChange,
  label,
  placeholder,
  disabled,
}: {
  repository: string;
  value: string;
  onChange: (value: string) => void;
  label: string;
  placeholder: string;
  disabled?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [query, setQuery] = useState("");
  const requestKey = `${repository}\0${query}`;
  const [response, setResponse] = useState<{ key: string; page: RepositoryRefs } | null>(null);
  const result = response?.key === requestKey ? response.page : null;
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const host = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const generation = useRef(0);
  const job = useRef<string | null>(null);
  const stop = useCallback(() => {
    generation.current++;
    if (job.current) void client.cancel(job.current).catch(() => {});
    job.current = null;
  }, []);
  const load = useCallback(
    async function load(offset: number, ticket: number) {
      const token = `refs-${crypto.randomUUID()}`;
      job.current = token;
      setLoading(true);
      setError("");
      try {
        const page = await navigationClient.refs(repository.trim(), query, offset, token);
        if (generation.current !== ticket) return;
        setResponse((previous) => ({
          key: requestKey,
          page:
            offset === 0 || previous?.key !== requestKey
              ? page
              : {
                  ...page,
                  refs: [
                    ...previous.page.refs,
                    ...page.refs.filter(
                      (ref) => !previous.page.refs.some((old) => old.name === ref.name),
                    ),
                  ],
                },
        }));
      } catch (cause) {
        if (generation.current === ticket) setError(message(cause));
      } finally {
        if (generation.current === ticket) {
          setLoading(false);
          job.current = null;
        }
      }
    },
    [repository, query, requestKey],
  );
  useEffect(() => {
    stop();
    if (!expanded || disabled || !repository.trim()) return;
    const ticket = generation.current;
    const timer = setTimeout(() => void load(0, ticket), 180);
    return () => {
      clearTimeout(timer);
      stop();
    };
  }, [expanded, repository, disabled, load, stop]);
  useEffect(() => {
    if (!expanded) return;
    search.current?.focus();
    function outside(event: MouseEvent) {
      if (event.target instanceof Node && !host.current?.contains(event.target)) setExpanded(false);
    }
    function keyboard(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        setExpanded(false);
        toggle.current?.focus();
      }
    }
    document.addEventListener("mousedown", outside);
    document.addEventListener("keydown", keyboard);
    return () => {
      document.removeEventListener("mousedown", outside);
      document.removeEventListener("keydown", keyboard);
    };
  }, [expanded]);
  return (
    <div className="revision-picker" ref={host}>
      <input
        aria-label={label}
        value={value}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
      />
      <button
        type="button"
        ref={toggle}
        disabled={disabled || !repository.trim()}
        aria-expanded={expanded}
        aria-label={`选择${label}`}
        onClick={() => setExpanded(!expanded)}
      >
        分支 ▾
      </button>
      {expanded && (
        <dialog open className="revision-menu" aria-label={`选择${label}`}>
          <div className="revision-search">
            <input
              ref={search}
              aria-label={`搜索${label}`}
              placeholder="搜索本地 / 远程分支或标签"
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <button type="button" aria-label="关闭分支选择" onClick={() => setExpanded(false)}>
              ×
            </button>
          </div>
          <p>可选择本地已有引用，或在输入框直接填写 commit ID / ref。不自动拉取远程。</p>
          {error && (
            <p role="alert" className="picker-error">
              {error}
            </p>
          )}
          <div className="revision-options">
            {result?.refs.map((ref) => (
              <button
                type="button"
                key={ref.name}
                onClick={() => {
                  onChange(ref.name);
                  setExpanded(false);
                  toggle.current?.focus();
                }}
              >
                <span>
                  <small>{kinds[ref.kind]}</small> {ref.label}
                  {result.currentBranch === ref.name ? " · 当前" : ""}
                </span>
                <code>{ref.target.slice(0, 10)}</code>
              </button>
            ))}
          </div>
          {loading && <output>读取引用中…</output>}
          {!loading && result?.refs.length === 0 && <p>没有匹配的分支或标签，可直接填写提交。</p>}
          {result?.nextOffset != null && (
            <button
              type="button"
              disabled={loading}
              onClick={() => void load(result.nextOffset!, generation.current)}
            >
              更多引用
            </button>
          )}
          {result?.truncated && <p>引用较多，请缩小搜索范围或直接填写完整引用。</p>}
          {result?.warnings.map((warning) => (
            <p key={warning}>{warning}</p>
          ))}
        </dialog>
      )}
    </div>
  );
}

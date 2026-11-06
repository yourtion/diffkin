import { useEffect, useRef, type RefObject } from "react";
import type { ComparisonSpec } from "@diffkin/protocol";
import { RepositoryPicker } from "./RepositoryPicker";
import { RevisionPair } from "./RevisionPair";
import { modeNames, comparisonSources } from "../lib/comparison-direction";

type Props = {
  spec: ComparisonSpec;
  recent: string[];
  available: boolean;
  busy: boolean;
  editing: boolean;
  error: string | null;
  recoverScope: boolean;
  pathInput: RefObject<HTMLInputElement | null>;
  pickerButton: RefObject<HTMLButtonElement | null>;
  onChange: (spec: ComparisonSpec) => void;
  onRepository: (path: string) => void;
  onSubmit: () => void;
  onCancel: () => void;
  onBack: () => void;
};

export function ComparisonSetup({
  spec,
  recent,
  available,
  busy,
  editing,
  error,
  recoverScope,
  pathInput,
  pickerButton,
  onChange,
  onRepository,
  onSubmit,
  onCancel,
  onBack,
}: Props) {
  const options = useRef<HTMLDetailsElement>(null);
  const scope = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if ((spec.pathScope || spec.verifyAll) && options.current) options.current.open = true;
  }, [spec.pathScope, spec.verifyAll]);
  useEffect(() => {
    if (recoverScope && options.current) {
      options.current.open = true;
      scope.current?.focus();
    }
  }, [recoverScope]);
  const localFiles = spec.mode === "worktree" || spec.mode === "unstaged";
  return (
    <section className="setup-screen" aria-labelledby="setup-title">
      <form
        id="comparison-settings"
        className="comparison-setup"
        onSubmit={(e) => {
          e.preventDefault();
          if (!busy) onSubmit();
        }}
      >
        <h2 id="setup-title">{editing ? "更改比较" : "打开仓库比较"}</h2>
        <fieldset disabled={busy}>
          <label className="setup-label" htmlFor="repository-path">
            本地仓库
          </label>
          <div className="setup-repo">
            <input
              id="repository-path"
              ref={pathInput}
              aria-label="Git 仓库路径"
              placeholder="输入或选择 Git 仓库目录"
              value={spec.repository}
              onChange={(event) => onRepository(event.target.value)}
            />
            {recent.length > 0 && (
              <select
                aria-label="最近使用的仓库"
                value=""
                onChange={(event) => onRepository(event.target.value)}
              >
                <option disabled value="">
                  最近仓库
                </option>
                {recent.map((path) => (
                  <option key={path} value={path}>
                    {path}
                  </option>
                ))}
              </select>
            )}
            <RepositoryPicker
              key={spec.repository}
              buttonRef={pickerButton}
              value={spec.repository}
              onChoose={onRepository}
              disabled={!available || busy}
            />
          </div>
          <label className="setup-label" htmlFor="comparison-mode">
            比较内容
          </label>
          <div className="setup-mode">
            <select
              id="comparison-mode"
              aria-label="比较模式"
              value={spec.mode}
              onChange={(event) =>
                onChange({ ...spec, mode: event.target.value as ComparisonSpec["mode"] })
              }
            >
              {Object.entries(modeNames).map(([value, name]) => (
                <option key={value} value={value}>
                  {name}
                </option>
              ))}
            </select>
            {spec.mode !== "commits" && <span>{comparisonSources(spec).join(" → ")}</span>}
          </div>
          {spec.mode === "commits" && (
            <RevisionPair spec={spec} disabled={!available || busy} onChange={onChange} />
          )}
          <details className="setup-options" ref={options}>
            <summary>范围与选项{spec.pathScope ? ` · ${spec.pathScope}` : ""}</summary>
            <label className="scope-field">
              路径范围
              <input
                ref={scope}
                aria-label="捕获路径范围"
                placeholder="留空比较整个仓库"
                value={spec.pathScope ?? ""}
                onChange={(event) =>
                  onChange({ ...spec, pathScope: event.target.value || undefined })
                }
              />
            </label>
            {localFiles && (
              <div className="setup-checks">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={spec.includeUntracked}
                    onChange={(event) =>
                      onChange({ ...spec, includeUntracked: event.target.checked })
                    }
                  />
                  包含未跟踪文件
                </label>
                <label
                  className="check"
                  title="逐个核对跟踪文件的原始字节，大仓库可能需要更长时间。"
                >
                  <input
                    type="checkbox"
                    checked={spec.verifyAll === true}
                    onChange={(event) => onChange({ ...spec, verifyAll: event.target.checked })}
                  />
                  完整原文核验
                </label>
              </div>
            )}
          </details>
        </fieldset>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        <div className="setup-actions">
          {busy ? (
            <>
              <span>正在读取仓库…</span>
              <button
                type="button"
                onClick={(event) => {
                  event.preventDefault();
                  onCancel();
                }}
              >
                取消
              </button>
            </>
          ) : (
            <>
              {editing && (
                <button type="button" onClick={onBack}>
                  返回当前比较
                </button>
              )}
              <button
                type="submit"
                className="primary"
                disabled={!available || !spec.repository.trim()}
              >
                开始比较
              </button>
            </>
          )}
        </div>
      </form>
    </section>
  );
}

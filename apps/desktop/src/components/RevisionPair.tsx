import { useId } from "react";
import type { ComparisonSpec } from "@diffkin/protocol";
import { comparisonSources, revisionNames } from "../lib/comparison-direction";
import { RevisionPicker } from "./RevisionPicker";

export function RevisionPair({
  spec,
  disabled,
  repositoryLabel = "",
  onChange,
}: {
  spec: ComparisonSpec;
  disabled: boolean;
  repositoryLabel?: string;
  onChange: (next: ComparisonSpec) => void;
}) {
  const id = useId();
  const [base, head] = comparisonSources(spec);
  const [baseName, headName] = revisionNames([base, head]);
  function field(side: "base" | "head", label: string, placeholder: string) {
    return (
      <div className="revision-side">
        <label htmlFor={`${id}-${side}`}>{label}</label>
        <RevisionPicker
          id={`${id}-${side}`}
          repository={spec.repository}
          label={repositoryLabel ? `${repositoryLabel} ${label}` : label}
          value={spec[side] ?? ""}
          placeholder={placeholder}
          disabled={disabled}
          onChange={(value) => onChange({ ...spec, [side]: value || null })}
        />
      </div>
    );
  }
  return (
    <div className="revision-pair">
      {field("base", "基准（修改前）", "HEAD~1")}
      <button
        className="swap-revisions"
        type="button"
        disabled={disabled || base === head}
        aria-label={repositoryLabel ? `${repositoryLabel} 交换比较方向` : "交换比较方向"}
        onClick={() => onChange({ ...spec, base: head, head: base })}
      >
        ⇄ 交换
      </button>
      {field("head", "目标（修改后）", "HEAD")}
      <p className="revision-preview" aria-live="polite">
        {base === head ? (
          <>
            两侧选中了同一版本 <strong>{baseName}</strong>。
          </>
        ) : (
          <>
            查看 <strong title={head}>{headName}</strong> 相对{" "}
            <strong title={base}>{baseName}</strong> 的改动
          </>
        )}
      </p>
    </div>
  );
}

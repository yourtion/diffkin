import type { ComparisonManifest, ComparisonSpec } from "@diffkin/protocol";

export const modeNames = {
  worktree: "全部本地改动",
  staged: "已暂存",
  unstaged: "未暂存",
  commits: "分支 / 提交",
};

const localSources = {
  worktree: ["HEAD", "工作区"],
  staged: ["HEAD", "暂存区"],
  unstaged: ["暂存区", "工作区"],
} as const;

export function comparisonSources(
  spec: Pick<ComparisonSpec, "mode"> & Partial<Pick<ComparisonSpec, "base" | "head">>,
): readonly [string, string] {
  return spec.mode === "commits"
    ? [spec.base || "HEAD~1", spec.head || "HEAD"]
    : localSources[spec.mode];
}

export function revisionNames([base, head]: readonly [string, string]): [string, string] {
  function name(ref: string) {
    if (ref.startsWith("refs/tags/")) return "标签 " + ref.slice("refs/tags/".length);
    if (ref.startsWith("refs/")) return ref.replace(/^refs\/(heads|remotes)\//, "");
    return ref.replace(/^([0-9a-f]{10})(?:[0-9a-f]{30}|[0-9a-f]{54})$/i, "$1");
  }
  const before = name(base);
  const after = name(head);
  return base !== head && before === after ? [base, head] : [before, after];
}

export type DiffSource = { name: string; title: string };
export function diffSources(
  manifest: ComparisonManifest,
  revisions?: readonly [string, string],
): [DiffSource, DiffSource] {
  const frozen = [manifest.baseLabel, manifest.headLabel] as const;
  const refs = manifest.mode === "commits" ? (revisions ?? frozen) : localSources[manifest.mode];
  const names = revisionNames(refs);
  function side(index: 0 | 1): DiffSource {
    const ref = refs[index];
    return {
      name: names[index],
      title: ref === frozen[index] ? ref : `${ref} · ${frozen[index]}`,
    };
  }
  return [side(0), side(1)];
}

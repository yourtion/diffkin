import type { ComparisonSpec } from "@diffkin/protocol";

const key = "diffkin.session.v1";
export type Preferences = { spec: ComparisonSpec; recent: string[] };
const defaults: Preferences = {
  spec: { repository: "", mode: "worktree", base: "HEAD~1", head: "HEAD", includeUntracked: true },
  recent: [],
};
export function loadPreferences(): Preferences {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!saved || typeof saved !== "object" || !("spec" in saved)) return defaults;
    const spec = saved.spec as ComparisonSpec;
    if (
      typeof spec.repository !== "string" ||
      !["commits", "staged", "unstaged", "worktree"].includes(spec.mode)
    )
      return defaults;
    return {
      spec: {
        repository: spec.repository,
        mode: spec.mode,
        base: typeof spec.base === "string" ? spec.base : "HEAD~1",
        head: typeof spec.head === "string" ? spec.head : "HEAD",
        includeUntracked: spec.includeUntracked === true,
      },
      recent:
        "recent" in saved && Array.isArray(saved.recent)
          ? saved.recent.filter((p): p is string => typeof p === "string").slice(0, 8)
          : [],
    };
  } catch {
    return defaults;
  }
}
export function savePreferences(spec: ComparisonSpec, recent: string[]): string[] {
  const next = [...new Set([spec.repository, ...recent])].slice(0, 8);
  try {
    localStorage.setItem(key, JSON.stringify({ spec, recent: next }));
  } catch {
    /* Storage may be unavailable; comparison still works. */
  }
  return next;
}

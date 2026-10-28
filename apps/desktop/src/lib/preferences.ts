import type { AppPreferences, ComparisonSpec } from "@diffkin/protocol";

const key = "diffkin.preferences.v1";
const pendingKey = "diffkin.preferences.pending.v1";
const acknowledgedKey = "diffkin.preferences.ack.v1";
export function defaultPreferences(): AppPreferences {
  return {
    schemaVersion: 1,
    spec: {
      repository: "",
      mode: "worktree",
      base: "HEAD~1",
      head: "HEAD",
      includeUntracked: true,
    },
    recent: [],
    view: { layout: "split", foldFormats: true, collapseUnchanged: true },
  };
}
export function parsePreferences(value: unknown): AppPreferences | null {
  if (!value || typeof value !== "object") return null;
  const saved = value as Partial<AppPreferences>;
  const spec = saved.spec;
  const view = saved.view;
  const text = (value: unknown, limit: number): value is string =>
    typeof value === "string" &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).length <= limit;
  const exactFields = (value: object, fields: string[]) =>
    !Array.isArray(value) &&
    Object.keys(value).length === fields.length &&
    Object.keys(value).every((key) => fields.includes(key));
  if (
    !exactFields(saved, ["schemaVersion", "spec", "recent", "view"]) ||
    saved.schemaVersion !== 1 ||
    !spec ||
    typeof spec !== "object" ||
    !(
      exactFields(spec, ["repository", "mode", "base", "head", "includeUntracked"]) ||
      exactFields(spec, ["repository", "mode", "base", "head", "includeUntracked", "verifyAll"])
    ) ||
    !(spec.verifyAll === undefined || typeof spec.verifyAll === "boolean") ||
    !text(spec.repository, 4096) ||
    !["commits", "staged", "unstaged", "worktree"].includes(spec.mode) ||
    !(spec.base === null || text(spec.base, 1024)) ||
    !(spec.head === null || text(spec.head, 1024)) ||
    typeof spec.includeUntracked !== "boolean" ||
    !Array.isArray(saved.recent) ||
    saved.recent.length > 8 ||
    !saved.recent.every((path) => text(path, 4096)) ||
    !view ||
    typeof view !== "object" ||
    !exactFields(view, ["layout", "foldFormats", "collapseUnchanged"]) ||
    !["split", "unified"].includes(view.layout) ||
    typeof view.foldFormats !== "boolean" ||
    typeof view.collapseUnchanged !== "boolean"
  )
    return null;
  const parsed: AppPreferences = {
    schemaVersion: 1,
    spec: {
      repository: spec.repository,
      mode: spec.mode,
      base: spec.base,
      head: spec.head,
      includeUntracked: spec.includeUntracked,
      ...(spec.verifyAll === true ? { verifyAll: true } : {}),
    },
    recent: [...saved.recent],
    view: {
      layout: view.layout,
      foldFormats: view.foldFormats,
      collapseUnchanged: view.collapseUnchanged,
    },
  };
  return new TextEncoder().encode(JSON.stringify(parsed)).length <= 64 * 1024 ? parsed : null;
}
function storedPreferences(storageKey: string): AppPreferences | null {
  try {
    const raw = localStorage.getItem(storageKey);
    return raw !== null && raw.length <= 64 * 1024 ? parsePreferences(JSON.parse(raw)) : null;
  } catch {
    return null;
  }
}
/** Recover only a valid preference snapshot which has not been acknowledged by the native store. */
export function loadPendingPreferences(): AppPreferences | null {
  const pending = storedPreferences(pendingKey);
  const acknowledged = storedPreferences(acknowledgedKey);
  return pending && JSON.stringify(pending) !== JSON.stringify(acknowledged) ? pending : null;
}
export function markPendingPreferences(preferences: AppPreferences) {
  const valid = parsePreferences(preferences);
  if (!valid) return;
  try {
    localStorage.setItem(pendingKey, JSON.stringify(valid));
  } catch {
    /* Native saving remains available. */
  }
}
export function acknowledgePreferences(preferences: AppPreferences) {
  const valid = parsePreferences(preferences);
  if (!valid) return;
  // Keep the acknowledgement separate: an old window must never delete a newer window's pending value.
  try {
    localStorage.setItem(acknowledgedKey, JSON.stringify(valid));
  } catch {
    /* A retry on next startup is safe. */
  }
}
export function loadPreferences(): AppPreferences {
  const defaults = defaultPreferences();
  const pending = loadPendingPreferences();
  if (pending) return pending;
  try {
    const cached = localStorage.getItem(key);
    if (cached !== null) return parsePreferences(JSON.parse(cached)) ?? defaults;
    const legacy: unknown = JSON.parse(localStorage.getItem("diffkin.session.v1") ?? "null");
    if (
      !legacy ||
      typeof legacy !== "object" ||
      !("spec" in legacy) ||
      !legacy.spec ||
      typeof legacy.spec !== "object"
    )
      return defaults;
    const spec = legacy.spec as ComparisonSpec;
    return (
      parsePreferences({
        ...defaults,
        spec: {
          ...defaults.spec,
          repository: spec.repository,
          mode: spec.mode,
          base: typeof spec.base === "string" ? spec.base : "HEAD~1",
          head: typeof spec.head === "string" ? spec.head : "HEAD",
          includeUntracked: spec.includeUntracked === true,
        },
        recent:
          "recent" in legacy && Array.isArray(legacy.recent)
            ? legacy.recent.filter((path) => typeof path === "string").slice(0, 8)
            : [],
      }) ?? defaults
    );
  } catch {
    return defaults;
  }
}
export function cachePreferences(preferences: AppPreferences) {
  try {
    localStorage.setItem(key, JSON.stringify(preferences));
  } catch {
    /* Native storage can still persist settings. */
  }
}

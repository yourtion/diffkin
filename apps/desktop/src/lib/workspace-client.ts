import { invoke } from "@tauri-apps/api/core";
import type {
  EvidenceBasis,
  WorkspaceRelationDraft,
  ReviewState,
  WorkspaceConfig,
  WorkspaceExport,
  WorkspaceSession,
  WorkspaceSummary,
} from "@diffkin/protocol";

const lastViewedKey = "diffkin.last-workspace.v1";
export function lastViewedWorkspace(): { id: string; repositoryId: string | null } | null {
  try {
    const item = JSON.parse(localStorage.getItem(lastViewedKey) ?? "null");
    return typeof item?.id === "string" &&
      (typeof item.repositoryId === "string" || item.repositoryId === null)
      ? item
      : null;
  } catch {
    return null;
  }
}
export function rememberWorkspace(id: string, repositoryId: string | null) {
  try {
    localStorage.setItem(lastViewedKey, JSON.stringify({ id, repositoryId }));
  } catch {
    // The native workspace archive remains available if WebView storage is full.
  }
}
export const workspaceClient = {
  list: () => invoke<WorkspaceSummary[]>("workspace_list"),
  save: (config: WorkspaceConfig) => invoke<WorkspaceSession>("workspace_save", { config }),
  capture: (config: WorkspaceConfig, jobId: string) =>
    invoke<WorkspaceSession>("workspace_capture", { config, jobId }),
  restore: (id: string, snapshotId: string | null = null) =>
    invoke<WorkspaceSession>("workspace_restore", { id, snapshotId }),
  review: (id: string, basis: EvidenceBasis, state: ReviewState) =>
    invoke<WorkspaceSession>("workspace_review", { id, basis, state }),
  linkFiles: (id: string, draft: WorkspaceRelationDraft) =>
    invoke<WorkspaceSession>("workspace_link_files", { id, draft }),
  export: (id: string, snapshotId: string | null) =>
    invoke<WorkspaceExport>("workspace_export", { id, snapshotId }),
};
export function sameBasis(a: EvidenceBasis, b: EvidenceBasis) {
  return (
    a.workspaceSnapshotId === b.workspaceSnapshotId &&
    a.repositoryId === b.repositoryId &&
    a.repositorySnapshotId === b.repositorySnapshotId &&
    a.fileId === b.fileId &&
    a.beforeId === b.beforeId &&
    a.afterId === b.afterId &&
    a.schemaVersion === b.schemaVersion &&
    a.ruleVersion === b.ruleVersion
  );
}

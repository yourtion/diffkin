import { invoke } from "@tauri-apps/api/core";
import type {
  EvidenceBasis,
  ManualRelation,
  ReviewState,
  WorkspaceConfig,
  WorkspaceExport,
  WorkspaceSession,
  WorkspaceSummary,
} from "@diffkin/protocol";
export const workspaceClient = {
  list: () => invoke<WorkspaceSummary[]>("workspace_list"),
  save: (config: WorkspaceConfig) => invoke<WorkspaceSession>("workspace_save", { config }),
  capture: (config: WorkspaceConfig, jobId: string) =>
    invoke<WorkspaceSession>("workspace_capture", { config, jobId }),
  restore: (id: string, snapshotId: string | null = null) =>
    invoke<WorkspaceSession>("workspace_restore", { id, snapshotId }),
  basis: (id: string, repositoryId: string, fileId: string) =>
    invoke<EvidenceBasis>("workspace_basis", { id, repositoryId, fileId }),
  review: (id: string, basis: EvidenceBasis, state: ReviewState) =>
    invoke<WorkspaceSession>("workspace_review", { id, basis, state }),
  relation: (id: string, relation: ManualRelation) =>
    invoke<WorkspaceSession>("workspace_relation", { id, relation }),
  export: (id: string) => invoke<WorkspaceExport>("workspace_export", { id }),
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

use crate::{CompareMode, ComparisonManifest, ComparisonSpec, FileStatus, RepositoryError};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRepository {
    pub id: String,
    pub label: String,
    pub spec: ComparisonSpec,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceConfig {
    pub id: String,
    pub name: String,
    pub repositories: Vec<WorkspaceRepository>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub repository_id: String,
    pub label: String,
    pub manifest: Option<ComparisonManifest>,
    pub error: Option<RepositoryError>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub workspace_id: String,
    /// Decimal Unix milliseconds. A string avoids integer precision loss in IPC.
    pub captured_at_ms: String,
    pub repositories: Vec<RepositorySnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvidenceBasis {
    pub workspace_snapshot_id: String,
    pub repository_id: String,
    pub repository_snapshot_id: String,
    pub file_id: String,
    pub before_id: Option<String>,
    pub after_id: Option<String>,
    pub schema_version: u32,
    pub rule_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewState {
    Pending,
    Reviewed,
    NeedsAttention,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualRelation {
    pub id: String,
    pub from: EvidenceBasis,
    pub to: EvidenceBasis,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRecord {
    pub basis: EvidenceBasis,
    pub state: ReviewState,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSession {
    pub config: WorkspaceConfig,
    pub snapshots: Vec<WorkspaceSnapshot>,
    pub active_snapshot_id: Option<String>,
    pub relations: Vec<ManualRelation>,
    pub reviews: Vec<ReviewRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    pub id: String,
    pub name: String,
    pub repository_count: u32,
    pub snapshot_count: u32,
    pub error: Option<String>,
}

/// Deliberately excludes config paths, display labels, source, notes and error messages.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExport {
    pub schema_version: u32,
    pub workspace_id: String,
    pub snapshot_id: Option<String>,
    pub repositories: Vec<ExportRepository>,
    pub relations: Vec<ExportRelation>,
    pub reviews: Vec<ReviewRecord>,
}
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportRepository {
    pub repository_id: String,
    pub snapshot_id: Option<String>,
    pub mode: Option<CompareMode>,
    pub base_identity: Option<String>,
    pub head_identity: Option<String>,
    pub available: bool,
    pub files: Vec<ExportFile>,
}
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub file_id: String,
    pub before_id: Option<String>,
    pub after_id: Option<String>,
    pub status: FileStatus,
}
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportRelation {
    pub id: String,
    pub from: EvidenceBasis,
    pub to: EvidenceBasis,
}

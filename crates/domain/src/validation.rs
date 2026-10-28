use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationSpec {
    pub repository: String,
    pub candidate_ref: String,
    pub baseline_ref: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIdentity {
    pub path: String,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationSnapshot {
    pub commit: String,
    pub tree: String,
    pub content_id: String,
    pub root: String,
    pub file_count: u32,
    pub total_bytes: u32,
    pub locks: Vec<ValidationIdentity>,
    pub complete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationPlan {
    pub id: String,
    pub created_at_ms: f64,
    pub repository: String,
    pub program: String,
    pub program_id: String,
    pub args: Vec<String>,
    pub working_directory: String,
    pub timeout_seconds: u32,
    pub command_id: String,
    pub candidate: ValidationSnapshot,
    pub baseline: Option<ValidationSnapshot>,
    pub executable: bool,
    pub blockers: Vec<String>,
    pub dependency_status: String,
    pub limitations: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationTarget {
    Candidate,
    Baseline,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ValidationStatus {
    Passed,
    Failed,
    NotRun,
    Indeterminate,
    Cancelled,
    Interrupted,
    EnvError,
    Running,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationLog {
    pub sequence: u32,
    pub stream: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationLogPage {
    pub entries: Vec<ValidationLog>,
    pub next_sequence: u32,
    pub truncated: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub id: String,
    pub plan_id: String,
    pub target: ValidationTarget,
    pub status: ValidationStatus,
    pub exit_code: Option<i32>,
    pub started_at_ms: f64,
    pub finished_at_ms: f64,
    pub source_id: String,
    pub program_id: String,
    pub command_id: String,
    pub dependency_status: String,
    pub source_changed: bool,
    pub changed_paths: Vec<String>,
    pub log_tail: Vec<ValidationLog>,
    pub log_truncated: bool,
    pub message: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum ValidationError {
    Invalid { message: String },
    Io { message: String },
    Cancelled { message: String },
    Unsupported { message: String },
}

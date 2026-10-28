use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod repository;
pub mod structure;
pub mod validation;
pub mod workspace;
pub use repository::*;
pub use structure::*;
pub use validation::*;
pub use workspace::*;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompareRequest {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// Original line text, including its line ending when present.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub schema_version: u32,
    pub lines: Vec<DiffLine>,
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum CompareError {
    InputTooLarge { message: String },
    Cancelled { message: String },
    Internal { message: String },
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysis {
    pub file: FrozenFile,
    pub analysis: Option<AnalysisResult>,
    pub notice: Option<String>,
}

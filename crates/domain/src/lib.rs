use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompareRequest {
    pub before: String,
    pub after: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Equal,
    Delete,
    Insert,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    /// Original line text, including its line ending when present.
    pub text: String,
}

#[derive(Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub schema_version: u32,
    pub lines: Vec<DiffLine>,
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum CompareError {
    InputTooLarge { message: String },
    Internal { message: String },
}

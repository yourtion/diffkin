use crate::Comparison;
use serde::Serialize;
use ts_rs::TS;

/// All offsets address the original, unnormalized source. Ends are exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceRange {
    pub byte_start: u32,
    pub byte_end: u32,
    pub utf16_start: u32,
    pub utf16_end: u32,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEvent {
    pub id: String,
    pub title: String,
    pub facets: Vec<String>,
    pub before: Option<SourceRange>,
    pub after: Option<SourceRange>,
    pub evidence: Vec<String>,
    /// Indices into AnalysisResult.raw.lines, exclusively non-equal lines.
    pub raw_edit_ids: Vec<u32>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum Presentation {
    Visible,
    FoldedFormat,
    TextFallback,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CoverageEntry {
    pub raw_edit_id: u32,
    pub presentation: Presentation,
    pub event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub schema_version: u32,
    pub language: String,
    pub rule_version: String,
    pub raw: Comparison,
    pub events: Vec<ChangeEvent>,
    pub coverage: Vec<CoverageEntry>,
    pub fallbacks: Vec<String>,
}

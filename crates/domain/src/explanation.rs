use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ExplanationProvider {
    Codex,
    Claude,
    Openai,
    Pi,
    Trae,
    Acp,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationTool {
    pub provider: ExplanationProvider,
    pub executable: Option<String>,
}

#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalApiConnection {
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationRequest {
    pub prepared_id: String,
    pub provider: ExplanationProvider,
    pub model: String,
    #[ts(optional)]
    pub connection: Option<LocalApiConnection>,
    pub question: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationExcerpt {
    pub start_line: u32,
    pub end_line: u32,
    pub focus_start: u32,
    pub focus_end: u32,
    pub text: String,
}

/// Constructed from frozen source in Rust, never accepted back from the WebView.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationContext {
    pub comparison_id: String,
    pub file_id: String,
    pub event_id: Option<String>,
    pub before_id: Option<String>,
    pub after_id: Option<String>,
    pub path: String,
    pub title: String,
    pub before: Option<ExplanationExcerpt>,
    pub after: Option<ExplanationExcerpt>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PreparedExplanation {
    pub id: String,
    pub context: ExplanationContext,
    pub tools: Vec<ExplanationTool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ExplanationSide {
    Before,
    After,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExplanationReference {
    pub side: ExplanationSide,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodeExplanation {
    pub meaning: String,
    pub change: String,
    pub rationale: String,
    pub caveats: Vec<String>,
    pub references: Vec<ExplanationReference>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExplanationResult {
    pub prepared_id: String,
    pub provider: ExplanationProvider,
    pub explanation: CodeExplanation,
}

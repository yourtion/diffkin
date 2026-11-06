use crate::{ExplanationProvider, ExplanationTool};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuizEvidence {
    pub id: u32,
    pub path: String,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PreparedQuiz {
    pub id: String,
    pub evidence: Vec<QuizEvidence>,
    pub added: u32,
    pub removed: u32,
    pub question_count: u8,
    pub tools: Vec<ExplanationTool>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuizRequest {
    pub prepared_id: String,
    pub provider: ExplanationProvider,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuizQuestion {
    pub prompt: String,
    pub choices: [String; 4],
    pub correct_index: u8,
    pub explanation: String,
    pub evidence_ids: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangeQuiz {
    pub questions: Vec<QuizQuestion>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QuizResult {
    pub prepared_id: String,
    pub quiz: ChangeQuiz,
}

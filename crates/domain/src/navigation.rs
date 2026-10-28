use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryRefKind {
    Branch,
    Remote,
    Tag,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRef {
    /// Full refname is passed unchanged to the existing comparison resolver.
    pub name: String,
    pub label: String,
    pub kind: RepositoryRefKind,
    pub target: String,
    pub symbolic_target: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryRefs {
    pub repository: String,
    pub refs: Vec<RepositoryRef>,
    pub current_branch: Option<String>,
    pub offset: u32,
    pub next_offset: Option<u32>,
    /// Additional matching refs exist beyond the bounded 4096-ref scan.
    pub truncated: bool,
    pub warnings: Vec<String>,
}

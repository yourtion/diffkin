use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum CompareMode {
    Commits,
    Staged,
    Unstaged,
    Worktree,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComparisonSpec {
    pub repository: String,
    pub mode: CompareMode,
    pub base: Option<String>,
    pub head: Option<String>,
    #[serde(default)]
    pub include_untracked: bool,
    /// Full literal verification, including files accepted by the Git stat cache.
    /// Omitted false values preserve existing workspace archive serialization.
    #[serde(default, skip_serializing_if = "is_false")]
    pub verify_all: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    TypeChanged,
    Conflict,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    pub id: String,
    /// Display-only path. Identity and filesystem operations use the original bytes.
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    pub before_id: Option<String>,
    pub after_id: Option<String>,
    pub notices: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonManifest {
    pub id: String,
    pub repository: String,
    pub mode: CompareMode,
    pub base_label: String,
    pub head_label: String,
    pub files: Vec<FileSummary>,
    pub total_files: u32,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FrozenFile {
    pub summary: FileSummary,
    /// None means absent or unavailable. Consult status and notices to distinguish them.
    pub before: Option<String>,
    pub after: Option<String>,
    pub before_truncated: bool,
    pub after_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum RepositoryError {
    Cancelled { message: String },
    BudgetExceeded { message: String },
    GitUnavailable { message: String },
    InvalidRepository { message: String },
    InvalidRevision { message: String },
    Stale { message: String },
    IoFailure { message: String },
    Unsupported { message: String },
}

impl std::fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Cancelled { message }
            | Self::BudgetExceeded { message }
            | Self::GitUnavailable { message }
            | Self::InvalidRepository { message }
            | Self::InvalidRevision { message }
            | Self::Stale { message }
            | Self::IoFailure { message }
            | Self::Unsupported { message } => message,
        };
        formatter.write_str(message)
    }
}
impl std::error::Error for RepositoryError {}

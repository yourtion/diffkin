use crate::ComparisonSpec;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum DiffLayout {
    Split,
    Unified,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewPreferences {
    pub layout: DiffLayout,
    pub fold_formats: bool,
    pub collapse_unchanged: bool,
}

/// UI preferences only. Loading this record does not open a repository or authorize a command.
#[derive(Debug, Clone, Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppPreferences {
    pub schema_version: u32,
    pub spec: ComparisonSpec,
    pub recent: Vec<String>,
    pub view: ViewPreferences,
}

/// Recoverable storage errors: the existing file is preserved until an explicit valid save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "code", rename_all = "kebab-case")]
pub enum PreferencesError {
    UnsupportedVersion { message: String },
    Corrupt { message: String },
    Invalid { message: String },
    UnsafePath { message: String },
    Io { message: String },
}

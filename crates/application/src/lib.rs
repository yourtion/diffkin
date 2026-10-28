pub mod repository;
pub use diffkin_analysis::analyze as analyze_text;
pub use diffkin_analysis::compare as compare_text;
pub use diffkin_analysis::MAX_BYTES as MAX_INPUT_BYTES;
pub use repository::{capture, FrozenComparison};

/// Shared by the native bridge and CLI; metadata-only changes never become empty-text claims.
pub fn analyze_file(
    file: diffkin_domain::FrozenFile,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<diffkin_domain::FileAnalysis, diffkin_domain::RepositoryError> {
    use diffkin_domain::{CompareError, FileAnalysis, FileStatus, RepositoryError};
    use std::sync::atomic::Ordering;
    if cancel.load(Ordering::Relaxed) {
        return Err(RepositoryError::Cancelled {
            message: "Comparison cancelled".into(),
        });
    }
    let special_mode = [&file.summary.old_mode, &file.summary.new_mode]
        .into_iter()
        .flatten()
        .any(|mode| mode == "120000" || mode == "160000");
    let unavailable = (file.summary.before_id.is_some() && file.before.is_none())
        || (file.summary.after_id.is_some() && file.after.is_none());
    if special_mode
        || unavailable
        || file.before_truncated
        || file.after_truncated
        || file.summary.status == FileStatus::Conflict
    {
        return Ok(FileAnalysis { file, analysis: None, notice: Some("This change requires the explicit metadata/raw preview. Structural analysis is unavailable for conflicts, symbolic links, submodules, binary/non-UTF-8 or truncated content.".into()) });
    }
    match diffkin_analysis::analyze(
        &file.summary.path,
        file.before.as_deref().unwrap_or(""),
        file.after.as_deref().unwrap_or(""),
        cancel,
    ) {
        Ok(analysis) => Ok(FileAnalysis {
            file,
            analysis: Some(analysis),
            notice: None,
        }),
        Err(CompareError::InputTooLarge { message }) => Ok(FileAnalysis {
            file,
            analysis: None,
            notice: Some(message),
        }),
        Err(CompareError::Cancelled { message }) => Err(RepositoryError::Cancelled { message }),
        Err(CompareError::Internal { message }) => Err(RepositoryError::IoFailure { message }),
    }
}

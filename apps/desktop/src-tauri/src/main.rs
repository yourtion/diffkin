#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod runtime;

use diffkin_domain::{ComparisonManifest, ComparisonSpec, FileAnalysis, RepositoryError};
use runtime::{check_cancel, permit, Runtime};
use std::sync::Arc;
use tauri::State;

type Shared = Arc<Runtime>;
fn internal(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::IoFailure {
        message: error.to_string(),
    }
}

#[tauri::command]
async fn open_comparison(
    spec: ComparisonSpec,
    job_id: String,
    state: State<'_, Shared>,
) -> Result<ComparisonManifest, RepositoryError> {
    let runtime = state.inner().clone();
    let token = runtime.begin(&job_id)?;
    let worker = runtime.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit(&worker.capture_gate, &token)?;
        check_cancel(&token)?;
        let snapshot = Arc::new(diffkin_application::capture(&spec, &token)?);
        check_cancel(&token)?;
        let manifest = snapshot.manifest(0, 200);
        *worker.current.lock().unwrap() = Some(snapshot);
        worker.cache.lock().unwrap().clear();
        Ok(manifest)
    })
    .await
    .map_err(internal);
    runtime.finish(&job_id);
    result?
}

#[tauri::command]
fn list_changes(
    id: String,
    offset: usize,
    limit: usize,
    state: State<'_, Shared>,
) -> Result<ComparisonManifest, RepositoryError> {
    Ok(state.snapshot(&id)?.manifest(offset, limit.min(200)))
}

#[tauri::command]
async fn request_file(
    id: String,
    file_id: String,
    job_id: String,
    state: State<'_, Shared>,
) -> Result<FileAnalysis, RepositoryError> {
    let runtime = state.inner().clone();
    let snapshot = runtime.snapshot(&id)?;
    if let Some(result) = runtime.cached(&id, &file_id) {
        return Ok(result);
    }
    let token = runtime.begin(&job_id)?;
    let worker = runtime.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit(&worker.analysis_gate, &token)?;
        check_cancel(&token)?;
        let file = snapshot.read_file(&file_id)?;
        let result = diffkin_application::analyze_file(file, &token)?;
        check_cancel(&token)?;
        worker.remember(id, file_id, result.clone());
        Ok(result)
    })
    .await
    .map_err(internal);
    runtime.finish(&job_id);
    result?
}

#[tauri::command]
fn cancel_job(job_id: String, state: State<'_, Shared>) {
    state.cancel(&job_id);
}

fn main() {
    tauri::Builder::default()
        .manage(Arc::new(Runtime::default()))
        .invoke_handler(tauri::generate_handler![
            open_comparison,
            list_changes,
            request_file,
            cancel_job
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Diffkin desktop");
}

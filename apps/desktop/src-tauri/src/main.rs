#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod runtime;
mod validation_commands;
mod workspace_commands;

use diffkin_domain::{ComparisonManifest, ComparisonSpec, FileAnalysis, RepositoryError};
use runtime::{check_cancel, permit, Runtime};
use std::sync::Arc;
use tauri::{Manager, State};

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
    let quitting = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let application = tauri::Builder::default()
        .manage(Arc::new(Runtime::default()))
        .manage(Arc::new(workspace_commands::WorkspaceRuntime::default()))
        .manage(Arc::new(validation_commands::ValidationRuntime::default()))
        .setup(|app| {
            validation_commands::recover(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_comparison,
            list_changes,
            request_file,
            cancel_job,
            workspace_commands::workspace_list,
            workspace_commands::workspace_save,
            workspace_commands::workspace_capture,
            workspace_commands::workspace_restore,
            workspace_commands::workspace_basis,
            workspace_commands::workspace_review,
            workspace_commands::workspace_relation,
            workspace_commands::workspace_export,
            validation_commands::validation_prepare,
            validation_commands::validation_run,
            validation_commands::validation_logs,
            validation_commands::validation_history,
            validation_commands::validation_plan,
            validation_commands::validation_result
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Diffkin desktop");
    application.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
            let runtime = app.state::<Shared>().inner().clone();
            runtime.shutdown();
            if runtime.has_active_jobs() {
                api.prevent_exit();
                if !quitting.swap(true, std::sync::atomic::Ordering::AcqRel) {
                    let handle = app.clone();
                    std::thread::spawn(move || {
                        // Wait for child process groups and frozen-capture workers to clean up.
                        while runtime.has_active_jobs() {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                        handle.exit(code.unwrap_or(0));
                    });
                }
            }
        }
    });
}

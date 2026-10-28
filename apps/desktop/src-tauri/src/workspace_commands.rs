use crate::{
    internal,
    runtime::{check_cancel, permit},
    Shared,
};
use diffkin_application::workspace::WorkspaceStore;
use diffkin_domain::*;
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};
use tauri::{Manager, State};

#[derive(Default)]
pub struct WorkspaceRuntime {
    gate: Mutex<()>,
}
type WorkspaceShared = Arc<WorkspaceRuntime>;
fn location(app: &tauri::AppHandle) -> Result<PathBuf, RepositoryError> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(internal)?
        .join("workspaces"))
}
async fn access<T: Send + 'static>(
    app: tauri::AppHandle,
    scope: WorkspaceShared,
    cancel: Arc<AtomicBool>,
    operation: impl FnOnce(WorkspaceStore) -> Result<T, RepositoryError> + Send + 'static,
) -> Result<T, RepositoryError> {
    let path = location(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = permit(&scope.gate, &cancel)?;
        check_cancel(&cancel)?;
        operation(WorkspaceStore::new(path)?)
    })
    .await
    .map_err(internal)?
}
fn token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[tauri::command]
pub async fn workspace_list(
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
) -> Result<Vec<WorkspaceSummary>, RepositoryError> {
    access(app, scope.inner().clone(), token(), |store| store.list()).await
}
#[tauri::command]
pub async fn workspace_save(
    config: WorkspaceConfig,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
) -> Result<WorkspaceSession, RepositoryError> {
    access(app, scope.inner().clone(), token(), move |store| {
        store.save_config(&config)
    })
    .await
}
#[tauri::command]
pub async fn workspace_capture(
    config: WorkspaceConfig,
    job_id: String,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
    state: State<'_, Shared>,
) -> Result<WorkspaceSession, RepositoryError> {
    let runtime = state.inner().clone();
    let cancel = runtime.begin(&job_id)?;
    let worker = runtime.clone();
    let cancelled = cancel.clone();
    let result = access(app, scope.inner().clone(), cancel, move |store| {
        let session = store.capture(&config, &cancelled)?;
        check_cancel(&cancelled)?;
        worker.install_workspace(Arc::new(store.restore(&session.config.id, None)?));
        store.load(&session.config.id)
    })
    .await;
    runtime.finish(&job_id);
    result
}
#[tauri::command]
pub async fn workspace_restore(
    id: String,
    snapshot_id: Option<String>,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
    state: State<'_, Shared>,
) -> Result<WorkspaceSession, RepositoryError> {
    let runtime = state.inner().clone();
    access(app, scope.inner().clone(), token(), move |store| {
        runtime.install_workspace(Arc::new(store.restore(&id, snapshot_id.as_deref())?));
        store.load(&id)
    })
    .await
}
#[tauri::command]
pub async fn workspace_basis(
    id: String,
    repository_id: String,
    file_id: String,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
    state: State<'_, Shared>,
) -> Result<EvidenceBasis, RepositoryError> {
    let runtime = state.inner().clone();
    access(app, scope.inner().clone(), token(), move |store| {
        let session = store.load(&id)?;
        let snapshot_id = session
            .active_snapshot_id
            .ok_or_else(|| RepositoryError::Stale {
                message: "Workspace has no active snapshot".into(),
            })?;
        let frozen = runtime
            .workspace(&snapshot_id)
            .or_else(|_| store.restore(&id, None).map(Arc::new))?;
        frozen.basis(&repository_id, &file_id)
    })
    .await
}

#[tauri::command]
pub async fn workspace_review(
    id: String,
    basis: EvidenceBasis,
    state: ReviewState,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
) -> Result<WorkspaceSession, RepositoryError> {
    access(app, scope.inner().clone(), token(), move |store| {
        store.set_review(&id, basis, state)
    })
    .await
}
#[tauri::command]
pub async fn workspace_relation(
    id: String,
    relation: ManualRelation,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
) -> Result<WorkspaceSession, RepositoryError> {
    access(app, scope.inner().clone(), token(), move |store| {
        store.add_relation(&id, relation)
    })
    .await
}
#[tauri::command]
pub async fn workspace_export(
    id: String,
    app: tauri::AppHandle,
    scope: State<'_, WorkspaceShared>,
) -> Result<WorkspaceExport, RepositoryError> {
    access(app, scope.inner().clone(), token(), move |store| {
        store.export(&id)
    })
    .await
}

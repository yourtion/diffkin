use crate::{
    internal,
    runtime::{check_cancel, permit},
    Shared,
};
use diffkin_domain::{RepositoryError, RepositoryRefs};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn list_repository_refs(
    repository: String,
    query: String,
    offset: u32,
    limit: u32,
    job_id: String,
    state: State<'_, Shared>,
) -> Result<RepositoryRefs, RepositoryError> {
    let runtime = state.inner().clone();
    let token = runtime.begin(&job_id)?;
    let worker = runtime.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit(&worker.capture_gate, &token)?;
        check_cancel(&token)?;
        diffkin_application::navigation::list_refs(&repository, &query, offset, limit, &token)
    })
    .await
    .map_err(internal);
    runtime.finish(&job_id);
    result?
}

#[tauri::command]
pub async fn pick_repository_directory(app: AppHandle) -> Result<Option<String>, RepositoryError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("选择 Git 仓库目录");
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog
            .blocking_pick_folder()
            .map(|path| {
                path.into_path()
                    .map_err(internal)?
                    .into_os_string()
                    .into_string()
                    .map_err(|_| RepositoryError::Unsupported {
                        message: "Selected path is not valid UTF-8".into(),
                    })
            })
            .transpose()
    })
    .await
    .map_err(internal)?
}

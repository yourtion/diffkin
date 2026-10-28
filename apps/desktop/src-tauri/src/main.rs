#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use diffkin_domain::{CompareError, CompareRequest, Comparison};

#[tauri::command]
async fn compare_text(request: CompareRequest) -> Result<Comparison, CompareError> {
    tauri::async_runtime::spawn_blocking(move || diffkin_application::compare_text(&request))
        .await
        .map_err(|error| CompareError::Internal {
            message: error.to_string(),
        })?
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![compare_text])
        .run(tauri::generate_context!())
        .expect("failed to run Diffkin desktop");
}

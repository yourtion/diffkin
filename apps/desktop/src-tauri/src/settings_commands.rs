use diffkin_application::preferences::PreferencesStore;
use diffkin_domain::{AppPreferences, PreferencesError};
use tauri::Manager;

fn io(error: impl std::fmt::Display) -> PreferencesError {
    PreferencesError::Io {
        message: error.to_string(),
    }
}

#[tauri::command]
pub async fn preferences_load(
    app: tauri::AppHandle,
) -> Result<Option<AppPreferences>, PreferencesError> {
    let directory = app.path().app_data_dir().map_err(io)?;
    tauri::async_runtime::spawn_blocking(move || PreferencesStore::new(directory).load())
        .await
        .map_err(io)?
}

#[tauri::command]
pub async fn preferences_save(
    app: tauri::AppHandle,
    preferences: AppPreferences,
) -> Result<(), PreferencesError> {
    let directory = app.path().app_data_dir().map_err(io)?;
    tauri::async_runtime::spawn_blocking(move || {
        PreferencesStore::new(directory).save(&preferences)
    })
    .await
    .map_err(io)?
}

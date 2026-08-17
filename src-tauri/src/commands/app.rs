//! App-wide odds and ends: the Mutagen version probe, the activity snapshot, and settings.

use tauri::State;

use crate::activity::types::Task;
use crate::activity::TaskRegistry;
use crate::error::AppError;
use crate::settings;
use crate::settings::types::AppSettings;
use crate::sync;

/// Smoke test for the bundled Mutagen sidecar: returns its version string.
#[tauri::command]
pub async fn mutagen_version(app: tauri::AppHandle) -> Result<String, AppError> {
    Ok(sync::version(&app).await?)
}

/// Snapshot activity registry (infallible).
#[tauri::command]
pub fn list_tasks(activity: State<'_, TaskRegistry>) -> Vec<Task> {
    activity.snapshot()
}

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, AppError> {
    Ok(settings::load(&app)?)
}

#[tauri::command]
pub fn set_settings(settings: AppSettings, app: tauri::AppHandle) -> Result<(), AppError> {
    settings::save(&app, &settings)?;
    Ok(())
}

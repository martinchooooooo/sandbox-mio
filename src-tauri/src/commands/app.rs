//! App-wide odds and ends: the activity snapshot and settings.

use tauri::State;

use crate::activity::types::Task;
use crate::activity::TaskRegistry;
use crate::error::AppError;
use crate::settings;
use crate::settings::types::AppSettings;

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

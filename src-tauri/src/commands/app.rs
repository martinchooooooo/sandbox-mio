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

/// Writes a first-run milestone (a short fixed name, no user data) to the application log, so the
/// time to the first model reply can be read off the log
/// (docs/features/fastest-time-to-wow/02_dev.md, Phase H).
#[tauri::command]
pub fn log_milestone(name: String, elapsed_ms: u64) -> Result<(), AppError> {
    let name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(48).collect();
    tracing::info!(target: "milestone", %name, elapsed_ms, "first-run milestone");
    Ok(())
}

//! App-global settings store. Persists [`AppSettings`] as JSON at `<app_config>/settings.json`.
//! Read by `LlamaServerEngine::start` (engine knobs) and the `/settings` route. Registered via
//! `mod settings` in `lib.rs`.

pub mod types;

use tauri::{AppHandle, Manager};

use types::{AppSettings, SettingsError};

fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, SettingsError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| SettingsError::Io(e.to_string()))?;
    std::fs::create_dir_all(&dir).map_err(|e| SettingsError::Io(e.to_string()))?;
    Ok(dir.join("settings.json"))
}

/// Load persisted settings, returning defaults when the file is absent.
pub fn load(app: &AppHandle) -> Result<AppSettings, SettingsError> {
    let path = settings_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| SettingsError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(e) => Err(SettingsError::Io(e.to_string())),
    }
}

/// Persist `settings`, overwriting the file.
pub fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), SettingsError> {
    let path = settings_path(app)?;
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| SettingsError::Parse(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| SettingsError::Io(e.to_string()))
}

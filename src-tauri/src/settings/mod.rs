//! App-global settings store. Persists [`AppSettings`] as JSON at `<app_config>/settings.json`.
//! Read by `LlamaServerEngine::start` (engine knobs) and the `/settings` route. Registered via
//! `mod settings` in `lib.rs`.

pub mod types;

use tauri::AppHandle;

use crate::env_ext::env_dir;
use types::{AppSettings, SettingsError};

/// Directory holding `settings.json`. `MIO_SETTINGS_DIR` relocates it (see
/// [`crate::env_ext::env_dir`]), else `MIO_APP_DATA_DIR` (see [`crate::paths::app_config_root`]):
/// an e2e run resetting settings to a known state would otherwise wipe the developer's real ones.
fn settings_path(app: &AppHandle) -> Result<std::path::PathBuf, SettingsError> {
    let dir = match env_dir("MIO_SETTINGS_DIR") {
        Some(dir) => dir,
        None => crate::paths::app_config_root(app).map_err(|e| SettingsError::Io(e.to_string()))?,
    };
    std::fs::create_dir_all(&dir).map_err(|e| SettingsError::Io(e.to_string()))?;
    Ok(dir.join("settings.json"))
}

/// Load persisted settings, returning defaults when the file is absent. `MIO_MACHINE_PROVIDER`
/// overrides `default_provider` for the process without touching the file, so an e2e run can point
/// the create flow at a provider without persisting that choice into the developer's settings.
pub fn load(app: &AppHandle) -> Result<AppSettings, SettingsError> {
    let path = settings_path(app)?;
    let mut settings = match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| SettingsError::Parse(e.to_string()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
        Err(e) => return Err(SettingsError::Io(e.to_string())),
    };
    if let Ok(tag) = std::env::var("MIO_MACHINE_PROVIDER") {
        match tag.parse() {
            Ok(provider) => settings.default_provider = provider,
            Err(_) => {
                tracing::warn!(target: "settings", %tag, "ignoring unknown MIO_MACHINE_PROVIDER")
            }
        }
    }
    Ok(settings)
}

/// Persist `settings`, overwriting the file.
pub fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), SettingsError> {
    let path = settings_path(app)?;
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| SettingsError::Parse(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| SettingsError::Io(e.to_string()))
}

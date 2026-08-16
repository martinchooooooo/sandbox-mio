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

/// Load persisted settings verbatim, returning defaults when the file is absent. No
/// `MIO_MACHINE_PROVIDER` override applied — this is what `get_settings` and `set_settings`'s
/// round-trip must see, so the override never gets written back into the developer's real file.
pub fn load(app: &AppHandle) -> Result<AppSettings, SettingsError> {
    let path = settings_path(app)?;
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| SettingsError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppSettings::default()),
        Err(e) => Err(SettingsError::Io(e.to_string())),
    }
}

/// The provider the create flow should default to: `MIO_MACHINE_PROVIDER` if set and valid,
/// else the persisted `default_provider`. Never persisted — an e2e run can point the create
/// flow at a provider without that choice leaking into the developer's settings.json.
pub fn resolve_default_provider(
    app: &AppHandle,
) -> Result<crate::provider::types::ProviderId, SettingsError> {
    let persisted = load(app)?.default_provider;
    match std::env::var("MIO_MACHINE_PROVIDER") {
        Ok(tag) => match tag.parse() {
            Ok(provider) => Ok(provider),
            Err(_) => {
                tracing::warn!(target: "settings", %tag, "ignoring unknown MIO_MACHINE_PROVIDER");
                Ok(persisted)
            }
        },
        Err(_) => Ok(persisted),
    }
}

/// Persist `settings`, overwriting the file.
pub fn save(app: &AppHandle, settings: &AppSettings) -> Result<(), SettingsError> {
    let path = settings_path(app)?;
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| SettingsError::Parse(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| SettingsError::Io(e.to_string()))
}

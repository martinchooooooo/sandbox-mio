//! `MIO_APP_DATA_DIR` override layer over `AppHandle::path()`'s Roaming-AppData resolution.
//! Read at call time and never cached, matching [`crate::env_ext::env_dir`]'s contract. Subsystems
//! that already carry their own more specific override (`MIO_SETTINGS_DIR`, `MIO_CREDENTIALS_DIR`)
//! check that variable first and fall back to this module, not the other way round.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

use crate::env_ext::env_dir;

/// `MIO_APP_DATA_DIR` when set, otherwise the compiled-in `app_data_dir()` (Roaming AppData).
pub fn app_data_root<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, tauri::Error> {
    match env_dir("MIO_APP_DATA_DIR") {
        Some(dir) => Ok(dir),
        None => app.path().app_data_dir(),
    }
}

/// `MIO_APP_DATA_DIR` when set, otherwise the compiled-in `app_config_dir()`. The two collapse to
/// one directory under the override, matching Windows, where both already resolve to the same
/// Roaming path.
pub fn app_config_root<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, tauri::Error> {
    match env_dir("MIO_APP_DATA_DIR") {
        Some(dir) => Ok(dir),
        None => app.path().app_config_dir(),
    }
}

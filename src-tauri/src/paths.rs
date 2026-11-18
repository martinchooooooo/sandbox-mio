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

/// A sandbox's SSH identity directory: the keypair and the pinned `known_hosts`. Shared by
/// `ensure_sandbox_identity` (`src-tauri/src/backends/mod.rs`) and the destroy cleanup in
/// `src-tauri/src/services/sandbox.rs`, so create and teardown name the same directory.
pub fn sandbox_key_dir<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<PathBuf, tauri::Error> {
    Ok(app_data_root(app)?.join("ssh-keys").join(name))
}

/// A sandbox's host-side pane state, kept out of the synced workspace so a login secret never
/// reaches a remote or version control (`docs/features/techdebt-sweep/02_dev.md`, Resolved
/// Decision 7).
pub fn sandbox_pane_dir<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<PathBuf, tauri::Error> {
    Ok(app_data_root(app)?.join("panes").join(name))
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

/// Per-sandbox resource usage history, one JSON-lines file per sandbox
/// (`docs/features/sandbox-resource-monitor/02_dev.md`, Resolved Decision 16).
pub fn resource_history_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, tauri::Error> {
    Ok(app_data_root(app)?.join("resources"))
}

/// A sandbox's GitHub-access state: the opt-out and the last apply outcome. Host-side, because a
/// file in the workspace is writable by code inside the sandbox
/// (`docs/features/github-cli-auth/02_dev.md`, Resolved Decision 10).
pub fn sandbox_github_state_path<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<PathBuf, tauri::Error> {
    Ok(app_data_root(app)?.join("github").join(format!("{name}.json")))
}

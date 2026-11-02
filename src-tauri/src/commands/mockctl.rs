//! The e2e light tier's mock control plane (`src-tauri/src/mockctl/mod.rs`).

use std::sync::Arc;

use tauri::State;

use crate::backends;
use crate::error::AppError;
use crate::mockctl;

/// Install one scripted mock behaviour (see `src-tauri/src/mockctl/mod.rs`). Test-only: hard-fails
/// outside the light e2e tier so a stray call against a real backend is loud, not a silent no-op.
#[tauri::command]
pub fn mock_configure(
    key: String,
    behaviour: mockctl::types::Behaviour,
    director: State<'_, Arc<mockctl::types::MockDirector>>,
) -> Result<(), AppError> {
    if !backends::mock_backend_enabled() {
        return Err(AppError::MockControl(
            "mock_configure is only available under MIO_MOCK_BACKEND=1".to_string(),
        ));
    }
    director.configure(&key, behaviour).map_err(AppError::MockControl)
}

/// Clear every installed mock override. See [`mock_configure`].
#[tauri::command]
pub fn mock_reset(director: State<'_, Arc<mockctl::types::MockDirector>>) -> Result<(), AppError> {
    if !backends::mock_backend_enabled() {
        return Err(AppError::MockControl(
            "mock_reset is only available under MIO_MOCK_BACKEND=1".to_string(),
        ));
    }
    director.reset();
    Ok(())
}

/// Journalled scripted calls for `key` since the last reset, carrying variable names but never
/// values. See [`mock_configure`].
#[tauri::command]
pub fn mock_calls(
    key: String,
    director: State<'_, Arc<mockctl::types::MockDirector>>,
) -> Result<Vec<mockctl::types::RecordedCall>, AppError> {
    if !backends::mock_backend_enabled() {
        return Err(AppError::MockControl(
            "mock_calls is only available under MIO_MOCK_BACKEND=1".to_string(),
        ));
    }
    Ok(director.calls(&key))
}

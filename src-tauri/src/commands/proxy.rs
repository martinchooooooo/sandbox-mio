//! Traffic-inspection sidecar status, flow watching, and secret values
//! (`docs/features/network-proxy/02_dev.md`). No command here ever returns a secret value.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::provider::ProviderRegistry;
use crate::proxy;
use crate::proxy::flowlog::FlowWatchRegistry;
use crate::proxy::types::ProxyStatus;

#[tauri::command]
pub async fn proxy_status(
    name: String,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<ProxyStatus, AppError> {
    Ok(proxy::status(&app, &registry, &name).await?)
}

/// Stores (or overwrites) the value a secret binding's `secretId` resolves to. Never logged or
/// returned.
#[tauri::command]
pub async fn set_proxy_secret(
    secret_id: String,
    value: String,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    Ok(proxy::set_secret(&app, &secret_id, &value)?)
}

#[tauri::command]
pub async fn delete_proxy_secret(secret_id: String, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(proxy::delete_secret(&app, &secret_id)?)
}

/// Starts the flow-log poll task behind `proxy://flows-changed` for `name`. Idempotent: a second
/// call while the task is live is a no-op, not an error.
#[tauri::command]
pub async fn watch_proxy_flows(
    name: String,
    registry: State<'_, FlowWatchRegistry>,
    providers: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    proxy::flowlog::start_watch(&app, &registry, &providers, &name);
    Ok(())
}

/// Restarts the sidecar from its last written state, for one that stopped or crashed.
#[tauri::command]
pub async fn restart_proxy(
    name: String,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    Ok(proxy::restart_proxy_container(&app, &registry, &name).await?)
}

//! Traffic-inspection sidecar status, flow watching, and secret values
//! (`docs/features/network-proxy/02_dev.md`). No command here ever returns a secret value.

use tauri::State;

use crate::error::AppError;
use crate::proxy;
use crate::proxy::flowlog::FlowWatchRegistry;
use crate::proxy::types::ProxyStatus;
use crate::services::proxy::locate_sidecar;
use crate::services::routing::SandboxRouter;

#[tauri::command]
pub async fn proxy_status(
    name: String,
    router: State<'_, SandboxRouter>,
    app: tauri::AppHandle,
) -> Result<ProxyStatus, AppError> {
    let site = locate_sidecar(&router, &name).await?;
    Ok(proxy::status(&app, &site, &name).await?)
}

/// How many secret values deploying `name` would copy to the target host, for the deploy picker's
/// notice. A count only: no value or secret id is returned.
#[tauri::command]
pub fn deploy_secret_count(
    name: String,
    workspace_path: String,
    app: tauri::AppHandle,
) -> Result<usize, AppError> {
    crate::backends::validate_name(&name)?;
    Ok(proxy::iron::deploy_secret_count(&app, &name, &workspace_path)?)
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

/// Starts the flow-log poll task behind `proxy://flows-changed` for `name`, or counts one more
/// watcher of the live one. Each call is released by one `unwatch_proxy_flows`.
#[tauri::command]
pub async fn watch_proxy_flows(
    name: String,
    registry: State<'_, FlowWatchRegistry>,
    router: State<'_, SandboxRouter>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let site = locate_sidecar(&router, &name).await?;
    proxy::flowlog::start_watch(&app, &registry, &site, &name);
    Ok(())
}

/// Releases one `watch_proxy_flows` call for `name`; the poll task stops with the last one.
#[tauri::command]
pub async fn unwatch_proxy_flows(
    name: String,
    registry: State<'_, FlowWatchRegistry>,
) -> Result<(), AppError> {
    proxy::flowlog::stop_watch(&registry, &name);
    Ok(())
}

/// Restarts the sidecar from its last written state, for one that stopped or crashed.
#[tauri::command]
pub async fn restart_proxy(
    name: String,
    router: State<'_, SandboxRouter>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let site = locate_sidecar(&router, &name).await?;
    Ok(proxy::restart_proxy_container(&app, &site, &name).await?)
}

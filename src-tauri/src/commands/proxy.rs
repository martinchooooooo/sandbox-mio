//! Traffic-inspection sidecar status, flow watching, and secret values
//! (`docs/features/network-proxy/02_dev.md`). No command here ever returns a secret value.

use tauri::State;

use crate::error::AppError;
use crate::proxy;
use crate::proxy::flowlog::FlowWatchRegistry;
use crate::proxy::store::ProxyStore;
use crate::proxy::types::ProxyStatus;
use crate::services::proxy::locate_sidecar;
use crate::services::routing::SandboxRouter;

#[tauri::command]
pub async fn proxy_status(
    name: String,
    router: State<'_, SandboxRouter>,
    store: State<'_, ProxyStore>,
) -> Result<ProxyStatus, AppError> {
    let site = locate_sidecar(&router, &name).await?;
    Ok(proxy::sidecar::status(&store, &site, &name).await?)
}

/// How many secret values deploying `name` would copy to the target host, for the deploy picker's
/// notice. Counts the accepted policy only. A count only: no value or secret id is returned.
#[tauri::command]
pub fn deploy_secret_count(
    name: String,
    store: State<'_, ProxyStore>,
    app: tauri::AppHandle,
) -> Result<usize, AppError> {
    crate::domain::validate_name(&name)?;
    Ok(proxy::github::deploy_secret_count(&app, &store, &name)?)
}

/// Starts the flow-log poll task behind `proxy://flows-changed` for `name`, or counts one more
/// watcher of the live one. Each call is released by one `unwatch_proxy_flows`.
#[tauri::command]
pub async fn watch_proxy_flows(
    name: String,
    registry: State<'_, FlowWatchRegistry>,
    router: State<'_, SandboxRouter>,
    store: State<'_, ProxyStore>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let site = locate_sidecar(&router, &name).await?;
    proxy::flowlog::start_watch(&app, &store, &registry, &site, &name);
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
    store: State<'_, ProxyStore>,
) -> Result<(), AppError> {
    let site = locate_sidecar(&router, &name).await?;
    Ok(proxy::sidecar::restart_proxy_container(&store, &site, &name).await?)
}

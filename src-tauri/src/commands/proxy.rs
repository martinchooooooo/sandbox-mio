//! Traffic-inspection sidecar status, secret-injection rules, and secret values
//! (`docs/features/network-proxy/02_dev.md`). No command here ever returns a secret value.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;
use crate::proxy;
use crate::proxy::rules::ProxyRule;
use crate::proxy::types::ProxyStatus;

#[tauri::command]
pub async fn proxy_status(
    name: String,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<ProxyStatus, AppError> {
    Ok(proxy::status(&app, &registry, &name).await?)
}

/// Reads `.mio/proxy-rules.json` — rule *definitions* only, never a resolved secret value.
#[tauri::command]
pub async fn list_proxy_rules(workspace_path: String) -> Result<Vec<ProxyRule>, AppError> {
    Ok(proxy::rules::load_rules(&workspace_path)?)
}

/// Overwrites `.mio/proxy-rules.json`, then — if `name` currently has a proxy sidecar —
/// re-materialises the proxy-side `rules.json` and restarts the container so the change takes
/// effect (`docs/features/network-proxy/02_dev.md`, Resolved Decision 11). A no-op restart when the
/// sandbox has no proxy sidecar: the definitions are still saved for a future proxy-enabled create.
#[tauri::command]
pub async fn set_proxy_rules(
    name: String,
    workspace_path: String,
    rules: Vec<ProxyRule>,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    proxy::rules::save_rules(&workspace_path, &rules)?;
    if proxy::exists(&registry, &name).await {
        proxy::apply_rules(&app, &registry.local(ProviderId::Podman), &name, &workspace_path).await?;
    }
    Ok(())
}

/// Stores (or overwrites) the value a rule's `secret_id` resolves to. Never logged or returned.
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

/// Explicit restart, for a rule/secret change the caller wants applied without going through
/// `set_proxy_rules`.
#[tauri::command]
pub async fn restart_proxy(name: String, registry: State<'_, Arc<ProviderRegistry>>) -> Result<(), AppError> {
    Ok(proxy::restart_proxy_container(&registry.local(ProviderId::Podman), &name).await?)
}

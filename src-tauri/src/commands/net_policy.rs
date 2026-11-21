//! The workspace network policy (`.mio/net-policy.json`) and its applied snapshot.

use std::sync::Arc;

use tauri::State;

use crate::backends::types::BackendError;
use crate::error::AppError;
use crate::net_policy::{self, NetPolicy};
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;

#[tauri::command]
pub fn get_net_policy(workspace_path: String) -> Result<NetPolicy, AppError> {
    Ok(net_policy::load(&workspace_path).map_err(BackendError::from)?)
}

/// Validates and saves the workspace file. Never touches the running sandbox: the policy takes
/// effect only through `apply_sandbox_policy`.
#[tauri::command]
pub fn set_net_policy(
    workspace_path: String,
    policy: NetPolicy,
    provider: ProviderId,
    registry: State<'_, Arc<ProviderRegistry>>,
) -> Result<(), AppError> {
    let caps = registry.local(provider).capabilities();
    net_policy::validate(&policy, caps).map_err(BackendError::from)?;
    Ok(net_policy::save(&workspace_path, &policy).map_err(BackendError::from)?)
}

/// Whether the workspace file differs from what `name` was created with, or last applied for a
/// Podman sandbox. A sandbox with no snapshot (no proxy, or created before this feature) never
/// reports drift.
#[tauri::command]
pub fn net_policy_drift(
    name: String,
    workspace_path: String,
    app: tauri::AppHandle,
) -> Result<bool, AppError> {
    let Some(applied) = net_policy::read_applied(&app, &name).map_err(BackendError::from)? else {
        return Ok(false);
    };
    let current = net_policy::load(&workspace_path).map_err(BackendError::from)?;
    Ok(current != applied.policy)
}

/// The bindings in `workspace_path`'s policy whose variable `name` never received, because they
/// were added after create (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 7).
#[tauri::command]
pub fn net_policy_unstamped_bindings(
    name: String,
    workspace_path: String,
    app: tauri::AppHandle,
) -> Result<Vec<net_policy::UnstampedBinding>, AppError> {
    let Some(applied) = net_policy::read_applied(&app, &name).map_err(BackendError::from)? else {
        return Ok(Vec::new());
    };
    let current = net_policy::load(&workspace_path).map_err(BackendError::from)?;
    Ok(net_policy::unstamped_bindings(&current, &applied))
}

/// Applies `name`'s workspace policy file to its iron-proxy sidecar without a recreate. Podman only
/// (`docs/features/podman-egress-boundary/02_dev.md`, Phase D).
#[tauri::command]
pub async fn apply_sandbox_policy(
    name: String,
    workspace_path: String,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<crate::proxy::iron::ApplyReport, AppError> {
    let registry = registry.inner().clone();
    let (provider, _) = registry.provider_of(&name).await;
    if provider != ProviderId::Podman {
        return Err(BackendError::CommandFailed(format!(
            "{name:?}: applying a policy without a recreate is only supported for Podman sandboxes"
        ))
        .into());
    }
    let policy = net_policy::load(&workspace_path).map_err(BackendError::from)?;
    net_policy::validate(&policy, registry.local(provider).capabilities())
        .map_err(BackendError::from)?;
    Ok(crate::proxy::iron::apply_policy(&app, &registry, &name, policy)
        .await
        .map_err(BackendError::from)?)
}

//! The workspace network policy (`.mio/net-policy.json`) and its applied snapshot
//! (`docs/features/microsandbox-network-policy/02_dev.md`, Phase D).

use std::sync::Arc;

use tauri::State;

use crate::backends::types::{BackendError, Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::net_policy::{self, NetPolicy};
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;
use crate::sync::types::SyncEngine;
use crate::network::types::NetworkManager;

#[tauri::command]
pub fn get_net_policy(workspace_path: String) -> Result<NetPolicy, AppError> {
    Ok(net_policy::load(&workspace_path).map_err(BackendError::from)?)
}

/// Validates and saves the workspace file. Never recreates the sandbox: a running microsandbox
/// sandbox keeps its create-time policy until it is re-applied.
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

/// Validates and saves `policy` as `name`'s workspace file, then applies it to the sandbox's
/// running iron-proxy sidecar without a recreate. Podman only
/// (`docs/features/podman-egress-boundary/02_dev.md`, Phase D).
#[tauri::command]
pub async fn apply_sandbox_policy(
    name: String,
    workspace_path: String,
    policy: NetPolicy,
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
    net_policy::validate(&policy, registry.local(provider).capabilities())
        .map_err(BackendError::from)?;
    net_policy::save(&workspace_path, &policy).map_err(BackendError::from)?;
    Ok(crate::proxy::iron::apply_policy(&app, &registry, &name, &workspace_path)
        .await
        .map_err(BackendError::from)?)
}

/// Recreates `name` so its current policy file and the requested viewer state take effect;
/// `viewer: None` keeps the viewer as it is. Reported through the activity center.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn reapply_sandbox_network(
    name: String,
    viewer: Option<bool>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    let (store, runtime, engine, network, registry) =
        (store.inner().clone(), runtime.inner().clone(), engine.inner().clone(), network.inner().clone(), registry.inner().clone());
    let id = format!("network-reapply-{name}");
    let kind = crate::activity::types::TaskKind::NetworkReapply;
    crate::activity::scope(&app.clone(), &id, kind, Some(name.clone()), |_scope| async move {
        crate::services::sandbox::reapply_network(
            &name, viewer, &store, &runtime, &engine, &network, &registry, &app,
        )
        .await
        .map_err(crate::activity::TaskFailure::same)
    })
    .await
}

/// The newest blocked secret uses microsandbox logged for `name`, newest first.
#[tauri::command]
pub fn net_policy_violations(name: String) -> Vec<crate::provider::types::SecretViolation> {
    crate::provider::msb::secret_violations(&name, 50)
}

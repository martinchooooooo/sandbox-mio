//! The accepted network policy of a sandbox, the workspace file as a proposal, and the apply that
//! turns a reviewed policy into the running one (`docs/features/net-policy-review/02_dev.md`).

use std::sync::Arc;

use tauri::State;

use crate::backends::types::SandboxBackend;
use crate::domain::types::BackendError;
use crate::error::AppError;
use crate::net_policy::{self, NetPolicy, PolicyChange, UnstampedBinding};
use crate::domain::types::ProviderId;
use crate::provider::ProviderRegistry;

/// A workspace policy file that differs from the accepted policy, with what changed.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub policy: NetPolicy,
    pub changes: Vec<PolicyChange>,
    pub unstamped: Vec<UnstampedBinding>,
}

/// Everything the network editor shows: the accepted policy and, when the workspace file differs,
/// the proposal. `proposal_error` is set for a file that does not parse or validate.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetPolicyState {
    pub accepted: NetPolicy,
    pub revision: u64,
    pub unstamped: Vec<UnstampedBinding>,
    pub proposal: Option<Proposal>,
    pub proposal_error: Option<String>,
}

/// A secret value to store in the sandbox's own store as part of an apply. Deliberately not
/// `Debug`, so it cannot reach a log.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreSecret {
    pub secret_id: String,
    pub value: String,
}

/// The sandbox's workspace path from its record, never from the frontend.
async fn workspace_of(store: &Arc<dyn SandboxBackend>, name: &str) -> Result<String, AppError> {
    crate::domain::validate_name(name)?;
    crate::services::sandbox::list_user(store)
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.workspace_path)
        .ok_or_else(|| BackendError::CommandFailed(format!("no sandbox named {name:?}")).into())
}

#[tauri::command]
pub async fn net_policy_state(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<NetPolicyState, AppError> {
    let workspace = workspace_of(&store, &name).await?;
    let accepted = net_policy::read_accepted(&app, &name).map_err(BackendError::from)?;
    let (provider, _) = registry.provider_of(&name).await;
    let caps = registry.local(provider).capabilities();
    let (proposal, proposal_error) = match net_policy::load_if_present(&workspace) {
        Ok(Some(file)) if file != accepted.policy => match net_policy::validate(&file, caps) {
            Ok(()) => (
                Some(Proposal {
                    changes: net_policy::diff(&accepted.policy, &file),
                    unstamped: net_policy::unstamped_bindings(&file, &accepted),
                    policy: file,
                }),
                None,
            ),
            Err(e) => (None, Some(e.0)),
        },
        Ok(_) => (None, None),
        Err(e) => (None, Some(e.0)),
    };
    Ok(NetPolicyState {
        unstamped: net_policy::unstamped_bindings(&accepted.policy, &accepted),
        revision: accepted.revision,
        accepted: accepted.policy,
        proposal,
        proposal_error,
    })
}

/// Whether the workspace file holds a proposal other than `except`: a parsable file that differs
/// from it. A broken file counts as pending too, so it is never silently overwritten.
fn proposal_pending(workspace: &str, except: &NetPolicy) -> bool {
    match net_policy::load_if_present(workspace) {
        Ok(Some(file)) => file != *except,
        Ok(None) => false,
        Err(_) => true,
    }
}

/// Writes the accepted policy over the workspace file so the file shows what runs. A failure is
/// logged, never returned: the apply already took effect.
fn mirror(workspace: &str, name: &str, policy: &NetPolicy) {
    if let Err(e) = net_policy::save(workspace, policy) {
        tracing::warn!(target: "net-policy", %name, error = %e, "failed to mirror the accepted policy to the workspace file");
    }
}

/// Makes `policy`, exactly as displayed, the accepted policy and applies it without a recreate.
/// Never reads the workspace file; refused when `base_revision` is stale. Podman only.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn apply_sandbox_policy(
    name: String,
    policy: NetPolicy,
    base_revision: u64,
    store_secret: Option<StoreSecret>,
    drop_secret_id: Option<String>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    registry: State<'_, Arc<ProviderRegistry>>,
    router: State<'_, crate::services::routing::SandboxRouter>,
    app: tauri::AppHandle,
) -> Result<crate::proxy::iron::ApplyReport, AppError> {
    let workspace = workspace_of(&store, &name).await?;
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
    let site = crate::services::proxy::locate_sidecar(&router, &name).await?;
    // Read before the apply, which can change what counts as pending: a proposal the user has not
    // decided on survives an apply that does not take it in.
    let prev = net_policy::read_accepted(&app, &name).map_err(BackendError::from)?.policy;
    let keep_file = proposal_pending(&workspace, &prev) && proposal_pending(&workspace, &policy);
    let edit = crate::proxy::iron::ValueEdit {
        store: store_secret.map(|s| (s.secret_id, s.value)),
        drop: drop_secret_id,
    };
    let report =
        crate::proxy::iron::apply_policy(&app, &site, &name, policy.clone(), base_revision, edit)
            .await
            .map_err(BackendError::from)?;
    if !keep_file {
        mirror(&workspace, &name, &policy);
    }
    Ok(report)
}

/// Rejects the workspace file's changes by writing the accepted policy over it.
#[tauri::command]
pub async fn discard_net_policy_proposal(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let workspace = workspace_of(&store, &name).await?;
    let accepted = net_policy::read_accepted(&app, &name).map_err(BackendError::from)?;
    Ok(net_policy::save(&workspace, &accepted.policy).map_err(BackendError::from)?)
}

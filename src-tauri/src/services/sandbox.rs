//! Sandbox create and teardown sequencing. Every ordering constraint in this file is load-bearing;
//! the comments say which and why.

use std::sync::Arc;

use crate::backends;
use crate::backends::types::{Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::ProviderId;
use crate::runtime::types::RuntimeManager;
use crate::sync::types::{SyncEngine, SyncStatus};
use crate::workspace;

/// Creates a sandbox on `provider` and brings its workspace up.
pub async fn create(
    name: &str,
    existing_path: Option<&str>,
    provider: ProviderId,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
) -> Result<Sandbox, AppError> {
    // Stage markers, so a failed create is attributable to a step from the log alone. The
    // microsandbox path spans four subsystems (provider, ssh, runtime, sync) and its failures
    // surface as one flattened string in the UI toast, which cannot say how far it got.
    tracing::info!(target: "create", %name, %provider, "create_sandbox: ensuring base image");
    store.ensure_base_image(provider).await?;
    tracing::info!(target: "create", %name, "create_sandbox: creating instance");
    let mut sandbox = store.create(name, existing_path, provider).await?;
    tracing::info!(
        target: "create",
        %name,
        workspace = %sandbox.workspace_path,
        "create_sandbox: instance created, initializing runtime"
    );
    runtime.init(&sandbox.workspace_path).await?;
    if provider == ProviderId::Microsandbox {
        // A microsandbox workspace lives in a VM disk volume, not a host bind mount, so it exists
        // only once Mutagen syncs into it — and the instance must already be running for `msb ssh
        // serve` (the alias's ProxyCommand) to answer (`docs/features/microsandbox-provider/02_dev.md`, Phase H).
        store.start(name).await?;
        let session = backends::msb_sync_session(name);
        let beta = backends::msb_sync_beta(name);
        tracing::info!(
            target: "create",
            %name,
            %session,
            alpha = %sandbox.workspace_path,
            %beta,
            "create_sandbox: creating workspace sync session"
        );
        if let Err(e) = engine.create(&session, &sandbox.workspace_path, &beta).await {
            // Mutagen reports a beta it cannot reach as one flattened chain (`unable to connect to
            // beta: ... unable to run SCP process: exit status 1`) whose root cause is always the
            // SSH transport; logging it beside the alias's `ProxyCommand` makes the two diagnosable together.
            tracing::error!(target: "create", %name, %beta, error = %e, "create_sandbox: sync create failed");
            // Mutagen never shows what the transport actually said, so ask the transport directly
            // while the sandbox is still up. Purely diagnostic — the original error is what the
            // user gets either way.
            crate::ssh::probe_alias(name).await;
            // Match the provider's own rollback discipline: leaving the instance behind means a
            // retry under the same name hits `NameTaken` instead of the sync error above.
            if let Err(destroy_err) = store.destroy(name).await {
                tracing::error!(
                    target: "create", %name, error = %destroy_err,
                    "create_sandbox: rollback after sync failure also failed"
                );
            }
            return Err(e.into());
        }
        tracing::info!(target: "create", %name, "create_sandbox: sync session created");
        sandbox.status = "running".to_string();
    }
    Ok(sandbox)
}

/// Closes panes and forwards before anything touches the SSH channel they run over, which would
/// otherwise orphan them. Shared by the undeploy and destroy flows, which order the steps around it
/// differently (`docs/features/lib-decomposition/02_dev.md`, Resolved Decision 2).
async fn close_panes_and_forwards(
    name: &str,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) {
    workspace::close_app(app, name).await;
    network.close_sandbox(name).await;
}

/// Reverses a deploy: flushes remote edits and tears the remote session down.
pub async fn undeploy(
    name: &str,
    store: &Arc<dyn SandboxBackend>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    let session = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| AppError::NotDeployed(name.to_string()))?;
    close_panes_and_forwards(name, network, app).await;
    // Order is load-bearing: flush pending remote writes first, and remove the container before
    // terminating the session — terminating drops Mutagen's SSH connections, which can briefly
    // make the machine unresponsive to a following `podman rm`.
    engine.flush(name).await?;
    store.remove_remote(name, &session.host).await?;
    engine.terminate(name).await?;
    Ok(store.start(name).await?)
}

/// Removes a sandbox. For a deployed sandbox, guards against discarding unsynced remote work
/// unless `force`.
pub async fn destroy(
    name: &str,
    force: bool,
    store: &Arc<dyn SandboxBackend>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    let session = engine
        .sessions()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name);
    let Some(session) = session else {
        close_panes_and_forwards(name, network, app).await;
        store.destroy(name).await?;
        // Best-effort, and a no-op for a Podman sandbox, which has no workspace session: a
        // microsandbox sandbox's volume is gone with the instance, so its sync has nothing left to
        // reconcile and would otherwise keep retrying a dead ProxyCommand.
        let _ = engine.terminate(&backends::msb_sync_session(name)).await;
        return Ok(());
    };
    let _ = engine.flush(name).await;
    if !force && engine.status(name).await? != SyncStatus::UpToDate {
        return Err(AppError::UnsyncedChanges {
            name: name.to_string(),
            host: session.host.clone(),
        });
    }
    close_panes_and_forwards(name, network, app).await;
    // The session is terminated before the remote removal here, the reverse of `undeploy`. The two
    // orderings differ deliberately (`docs/features/lib-decomposition/02_dev.md`, Resolved
    // Decision 2) — do not unify them.
    engine.terminate(name).await?;
    store.remove_remote(name, &session.host).await?;
    Ok(store.destroy(name).await?)
}

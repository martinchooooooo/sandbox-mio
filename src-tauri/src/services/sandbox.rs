//! Sandbox create and teardown sequencing. Every ordering constraint in this file is load-bearing;
//! the comments say which and why.

use std::sync::Arc;

use crate::activity::types::TaskKind;
use crate::activity::{self, TaskFailure, TaskScope};
use crate::backends;
use crate::backends::types::{BackendError, EgressRequest, Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;
use crate::sync::types::{SyncEngine, SyncStatus};
use crate::workspace;

/// Removes the host-side state a sandbox leaves outside its container: the managed SSH alias, the
/// identity directory (whose `known_hosts` is the pinned host key), the pane state directory, and
/// the GitHub access state file.
/// Best-effort and warn-logged, so it can never fail a destroy
/// (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 8). The alias removal strips only
/// `name`'s own block from the host-global config, which is what keeps concurrent worktrees safe.
fn cleanup_host_state(name: &str, app: &tauri::AppHandle) {
    // The name becomes a path component below; destroy takes it from IPC unvalidated.
    if backends::validate_name(name).is_err() {
        tracing::warn!(target: "destroy", %name, "skipping host state cleanup for an invalid sandbox name");
        return;
    }
    if let Err(e) = crate::ssh::remove_alias(name) {
        tracing::warn!(target: "destroy", %name, error = %e, "could not remove the sandbox ssh alias");
    }
    remove_dir_best_effort(name, "ssh identity", crate::paths::sandbox_key_dir(app, name));
    remove_dir_best_effort(name, "pane state", crate::paths::sandbox_pane_dir(app, name));
    // Spawned because this fn is sync; `forget` takes the reconcile lock so a reconcile in flight
    // cannot recreate the file for a same-name sandbox.
    let (app_for_github, name_for_github) = (app.clone(), name.to_string());
    tauri::async_runtime::spawn(async move {
        super::github_access::forget(&app_for_github, &name_for_github).await;
    });
    // A sandbox recreated under the same name must not inherit this one's usage history.
    if let Some(monitor) =
        tauri::Manager::try_state::<Arc<crate::services::resources::ResourceMonitor>>(app)
    {
        monitor.forget(app, name);
    }
}

fn remove_dir_best_effort(name: &str, what: &str, dir: Result<std::path::PathBuf, tauri::Error>) {
    let dir = match dir {
        Ok(dir) => dir,
        Err(e) => {
            tracing::warn!(target: "destroy", %name, error = %e, "could not resolve the {what} directory");
            return;
        }
    };
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(target: "destroy", %name, dir = %dir.display(), error = %e, "could not remove the {what} directory"),
    }
}

/// The sandboxes a user manages: everything the backend lists except auth containers and proxy
/// sidecars.
pub async fn list_user(store: &Arc<dyn SandboxBackend>) -> Result<Vec<Sandbox>, AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .filter(|s| !s.name.starts_with(crate::credentials::types::AUTH_CONTAINER_PREFIX))
        .filter(|s| !crate::proxy::is_sidecar(s))
        .collect())
}

/// Creates a sandbox on `provider` and brings its workspace up. Every sandbox gets a per-sandbox
/// proxy sidecar (`docs/features/network-proxy/02_dev.md`); for Podman that is the iron-proxy
/// egress boundary (`docs/features/podman-egress-boundary/02_dev.md`).
///
/// Runs as the `sandbox-create:<name>` activity task once the name is known to be usable, so the
/// UI can show the current step; a rejected name fails fast without opening a task.
pub async fn create(
    name: &str,
    existing_path: Option<&str>,
    provider: ProviderId,
    egress: Option<&EgressRequest>,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    registry: &Arc<ProviderRegistry>,
    app: &tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    // Checked before the sidecar is created, not only in `store.create`: a sidecar create on a taken
    // name fails, and its rollback removes `<name>-proxy` by name, which is then the existing
    // sandbox's live sidecar.
    backends::validate_new_name(name)?;
    if let Some(existing) = store.list().await?.into_iter().find(|s| s.name == name) {
        return Err(crate::backends::types::BackendError::NameTaken {
            name: name.to_string(),
            provider: existing.provider,
        }
        .into());
    }

    let id = format!("sandbox-create:{name}");
    activity::scope(app, &id, TaskKind::SandboxCreate, Some(name.to_string()), |task| async move {
        create_steps(&task, name, existing_path, provider, egress, store, runtime, registry, app)
            .await
            .map_err(TaskFailure::same)
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn create_steps(
    task: &TaskScope,
    name: &str,
    existing_path: Option<&str>,
    provider: ProviderId,
    egress: Option<&EgressRequest>,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    registry: &Arc<ProviderRegistry>,
    app: &tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    // Stage markers, so a failed create is attributable to a step from the log alone: failures
    // surface as one flattened string in the UI toast, which cannot say how far it got.
    tracing::info!(target: "create", %name, %provider, "create_sandbox: ensuring base image");
    task.progress(None, "Preparing base image");
    store.ensure_base_image(provider).await?;

    // The sidecar runs on the sandbox's own provider.
    let sidecar = registry.local(provider);
    tracing::info!(target: "create", %name, "create_sandbox: ensuring proxy image");
    task.progress(None, "Preparing network proxy");
    crate::proxy::ensure_proxy_image(app, &sidecar, provider).await?;
    tracing::info!(target: "create", %name, "create_sandbox: creating proxy sidecar");
    task.progress(None, "Starting network proxy");
    let attachment = match crate::proxy::create_proxy(app, &sidecar, provider, name).await {
        Ok(a) => a,
        Err(e) => {
            crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            return Err(e.into());
        }
    };

    tracing::info!(target: "create", %name, "create_sandbox: creating instance");
    task.progress(None, "Creating container");
    let sandbox = match store.create(name, existing_path, provider, Some(&attachment), egress).await {
        Ok(s) => s,
        Err(e) => {
            // The proxy container and network exist ahead of the sandbox they attach to
            // (`docs/features/network-proxy/02_dev.md`, Concrete Model); a failed sandbox create
            // must not strand them.
            crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            return Err(e.into());
        }
    };
    // Order is load-bearing: the sidecar denies everything until the first apply (Resolved
    // Decision 18), and the policy can only be read once `store.create` resolved the workspace.
    task.progress(None, "Applying network policy");
    if let Err(e) = apply_policy_and_inject_ca(app, registry, &sidecar, name, &sandbox.workspace_path).await {
        tracing::error!(target: "create", %name, error = %e, "create_sandbox: proxy setup failed, rolling back");
        match store.destroy(name).await {
            Ok(()) => cleanup_host_state(name, app),
            Err(destroy_err) => tracing::error!(
                target: "create", %name, error = %destroy_err,
                "create_sandbox: rollback after proxy setup failure also failed"
            ),
        }
        crate::net_policy::remove_applied(app, name);
        crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
        return Err(e);
    }
    tracing::info!(
        target: "create",
        %name,
        workspace = %sandbox.workspace_path,
        "create_sandbox: instance created, initializing runtime"
    );
    task.progress(None, "Setting up runtime");
    runtime.init(&sandbox.workspace_path).await?;
    super::inference_reach::spawn_ensure_for_sandbox(app, name);
    super::github_access::spawn_reconcile(app, name);
    Ok(sandbox)
}

/// Applies the workspace policy to a new Podman sandbox's iron-proxy sidecar, then pipes the
/// sidecar's CA into the sandbox.
async fn apply_policy_and_inject_ca(
    app: &tauri::AppHandle,
    registry: &Arc<ProviderRegistry>,
    podman: &Arc<dyn crate::provider::types::MachineProvider>,
    name: &str,
    workspace_path: &str,
) -> Result<(), AppError> {
    tracing::info!(target: "create", %name, "create_sandbox: applying network policy");
    let policy = crate::net_policy::load(workspace_path).map_err(BackendError::from)?;
    crate::net_policy::validate(&policy, podman.capabilities()).map_err(BackendError::from)?;
    crate::proxy::iron::apply_policy(app, registry, name, policy).await?;
    tracing::info!(target: "create", %name, "create_sandbox: injecting proxy CA");
    let pem = crate::proxy::read_ca_pem(app, name, ProviderId::Podman)?;
    crate::proxy::inject_ca(podman, name, &pem).await?;
    Ok(())
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
    registry: &Arc<ProviderRegistry>,
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
        crate::net_policy::remove_applied(app, name);
        cleanup_host_state(name, app);
        // Best-effort, so the leftovers of a half-created sandbox are cleaned up too.
        let (id, provider) = crate::proxy::sidecar_provider(app, registry, name);
        crate::proxy::remove_proxy(app, &provider, id, name).await;
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
    store.destroy(name).await?;
    cleanup_host_state(name, app);
    Ok(())
}

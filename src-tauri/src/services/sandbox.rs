//! Sandbox create and teardown sequencing. Every ordering constraint in this file is load-bearing;
//! the comments say which and why.

use std::sync::Arc;

use crate::activity::types::TaskKind;
use crate::activity::{self, TaskFailure, TaskScope};
use crate::backends;
use crate::backends::types::{BackendError, EgressRequest, Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::{MachineRef, ProviderId};
use crate::provider::ProviderRegistry;
use crate::proxy::flowlog::FlowWatchRegistry;
use crate::proxy::types::SidecarSite;
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
    // Decision 18 of podman-egress-boundary).
    task.progress(None, "Applying network policy");
    let site = crate::proxy::types::SidecarSite::local(registry, provider);
    if let Err(e) = apply_policy_and_inject_ca(app, &site, name).await {
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
    site: &crate::proxy::types::SidecarSite,
    name: &str,
) -> Result<(), AppError> {
    let podman = &site.provider;
    tracing::info!(target: "create", %name, "create_sandbox: applying network policy");
    crate::proxy::iron::apply_accepted(app, site, name).await?;
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

/// Starts a sandbox, making sure its sidecar runs first so the sandbox has its network: after a
/// machine restart, or a remote host reboot, the sidecar is stopped and the remote one has lost its
/// secrets (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 5).
pub async fn start(
    name: &str,
    store: &Arc<dyn SandboxBackend>,
    router: &super::routing::SandboxRouter,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    let site = super::proxy::locate_sidecar(router, name).await?;
    crate::proxy::ensure_sidecar_running(app, &site, name).await?;
    if site.local {
        return Ok(store.start(name).await?);
    }
    // A deployed sandbox runs on its target; its local container stays stopped.
    site.provider.start(name).await.map_err(BackendError::from)?;
    Ok(())
}

/// The site of a deployed sandbox's remote sidecar on `host`.
fn remote_site(registry: &ProviderRegistry, host: &str) -> SidecarSite {
    let machine = MachineRef { provider: ProviderId::Podman, id: host.to_string() };
    SidecarSite { provider: registry.provider_for(&machine), machine, local: false }
}

/// Deploys a sandbox to `host`: recreates its network boundary on the target, then moves the sandbox
/// there and starts syncing. Runs as the `sandbox-deploy:<name>` activity task once the plan has
/// validated, so a refused target fails fast without opening one.
#[allow(clippy::too_many_arguments)]
pub async fn deploy(
    name: &str,
    host: &str,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    registry: &Arc<ProviderRegistry>,
    flows: &FlowWatchRegistry,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    // A second deploy would meet the live deployment's containers, which a rollback must never touch.
    if engine.sessions().await?.iter().any(|s| s.name == name) {
        return Err(BackendError::CommandFailed(format!(
            "{name:?} is already deployed; undeploy it first"
        ))
        .into());
    }
    // Deploy always moves the sidecar; a sandbox without one has no boundary to recreate
    // (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 3).
    crate::proxy::require_sidecar(app, registry, name).await?;
    let plan = store.plan_deploy(name, host).await?;
    let id = format!("sandbox-deploy:{name}");
    activity::scope(app, &id, TaskKind::SandboxDeploy, Some(name.to_string()), |task| async move {
        deploy_steps(&task, name, host, &plan, store, runtime, engine, registry, flows, app)
            .await
            .map_err(TaskFailure::same)
    })
    .await
}

#[allow(clippy::too_many_arguments)]
async fn deploy_steps(
    task: &TaskScope,
    name: &str,
    host: &str,
    plan: &crate::backends::types::DeployPlan,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    registry: &Arc<ProviderRegistry>,
    flows: &FlowWatchRegistry,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    let local = SidecarSite::local(registry, ProviderId::Podman);
    let remote = remote_site(registry, host);
    let mut undo = DeployUndo::default();
    match deploy_to_target(task, name, plan, store, runtime, engine, flows, app, &local, &remote, &mut undo).await {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::error!(target: "deploy", %name, %host, error = %e, "deploy failed, rolling back");
            task.progress(None, "Rolling back");
            rollback(name, host, store, flows, app, &local, &remote, &undo).await;
            Err(e)
        }
    }
}

/// What a failed deploy has to undo: one flag per resource this run created or stopped.
#[derive(Default)]
struct DeployUndo {
    /// Remote network, sidecar and laptop staging, all created by this run.
    remote_proxy: bool,
    /// Set before `store.deploy` runs, as it creates the container in several steps.
    remote_sandbox: bool,
    watch_retargeted: bool,
    local_sidecar_stopped: bool,
    local_sandbox_stopped: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum UndoStep {
    RemoveRemoteSandbox,
    RemoveRemoteProxy,
    RetargetWatchLocal,
    StartLocalSidecar,
    StartLocalSandbox,
}

impl DeployUndo {
    /// The rollback steps in execution order, restricted to what this run did.
    fn steps(&self) -> Vec<UndoStep> {
        let wanted = [
            (self.remote_sandbox, UndoStep::RemoveRemoteSandbox),
            (self.remote_proxy, UndoStep::RemoveRemoteProxy),
            (self.watch_retargeted, UndoStep::RetargetWatchLocal),
            (self.local_sidecar_stopped, UndoStep::StartLocalSidecar),
            (self.local_sandbox_stopped, UndoStep::StartLocalSandbox),
        ];
        wanted.into_iter().filter_map(|(done, step)| done.then_some(step)).collect()
    }
}

/// The first of `names` already present on the target, which a deploy must not adopt or remove.
fn remote_collision<'a>(existing: impl IntoIterator<Item = &'a str>, names: &[String]) -> Option<String> {
    existing.into_iter().find(|e| names.iter().any(|n| n == e)).map(str::to_string)
}

#[allow(clippy::too_many_arguments)]
async fn deploy_to_target(
    task: &TaskScope,
    name: &str,
    plan: &crate::backends::types::DeployPlan,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    flows: &FlowWatchRegistry,
    app: &tauri::AppHandle,
    local: &SidecarSite,
    remote: &SidecarSite,
    undo: &mut DeployUndo,
) -> Result<(), AppError> {
    let target = remote.machine.clone();
    let existing = remote.provider.list_instances().await.map_err(BackendError::from)?;
    let owned = [name.to_string(), crate::proxy::proxy_container_name(name)];
    if let Some(clash) = remote_collision(existing.iter().map(|i| i.name.as_str()), &owned) {
        return Err(BackendError::CommandFailed(format!(
            "the host already has a container named {clash:?}; remove it before deploying {name:?}"
        ))
        .into());
    }
    task.progress(None, "Preparing network proxy on the host");
    crate::proxy::ensure_proxy_image(app, &remote.provider, ProviderId::Podman).await?;
    task.progress(None, "Starting network proxy on the host");
    let attachment = crate::proxy::create_remote_proxy(app, remote, name).await?;
    undo.remote_proxy = true;
    // The remote sandbox carries the placeholders of the applied snapshot, so one snapshot stays
    // valid for both containers (Resolved Decision 10).
    let guest_env = crate::net_policy::read_applied(app, name)
        .map_err(BackendError::from)?
        .and_then(|a| a.guest_env)
        .unwrap_or_default();
    let extra_env: Vec<(String, String)> =
        guest_env.iter().map(|v| (v.clone(), crate::net_policy::placeholder(v))).collect();

    task.progress(None, "Creating the sandbox on the host");
    undo.remote_sandbox = true;
    undo.local_sandbox_stopped = true;
    let deployment = store.deploy(name, plan, &attachment, &extra_env).await?;

    // Order is load-bearing: the remote sidecar denies everything until this apply (Resolved
    // Decision 18 of podman-egress-boundary), so `reproduce_remote` must come after it. Only the
    // accepted policy is deployed (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 7).
    task.progress(None, "Applying network policy");
    crate::proxy::iron::apply_accepted(app, remote, name).await?;
    let pem = crate::proxy::read_remote_ca_pem(app, name)?;
    crate::proxy::inject_ca(&remote.provider, name, &pem).await?;
    // Stopped, never removed: the stopped local sandbox's network is fixed at create
    // (Resolved Decision 12).
    undo.local_sidecar_stopped = true;
    if let Err(e) = local.provider.stop_sidecar(&crate::proxy::proxy_container_name(name)).await {
        tracing::warn!(target: "deploy", %name, error = %e, "could not stop the local proxy sidecar");
    }
    crate::proxy::flowlog::retarget(app, flows, name, remote);
    undo.watch_retargeted = true;

    task.progress(None, "Installing tools on the host");
    runtime.reproduce_remote(name, &target, &deployment.alpha).await?;
    engine.create(name, &deployment.alpha, &deployment.beta).await?;
    Ok(())
}

/// Reverses a failed deploy, best-effort and warn-logged, undoing only what `undo` recorded
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 16).
#[allow(clippy::too_many_arguments)]
async fn rollback(
    name: &str,
    host: &str,
    store: &Arc<dyn SandboxBackend>,
    flows: &FlowWatchRegistry,
    app: &tauri::AppHandle,
    local: &SidecarSite,
    remote: &SidecarSite,
    undo: &DeployUndo,
) {
    for step in undo.steps() {
        match step {
            UndoStep::RemoveRemoteSandbox => {
                if let Err(e) = store.remove_remote(name, host).await {
                    tracing::warn!(target: "deploy", %name, error = %e, "rollback: could not remove the remote sandbox");
                }
            }
            UndoStep::RemoveRemoteProxy => {
                crate::proxy::remove_remote_proxy(remote, name).await;
                crate::proxy::remove_remote_state(app, name);
            }
            UndoStep::RetargetWatchLocal => crate::proxy::flowlog::retarget(app, flows, name, local),
            UndoStep::StartLocalSidecar => {
                if let Err(e) = crate::proxy::ensure_sidecar_running(app, local, name).await {
                    tracing::warn!(target: "deploy", %name, error = %e, "rollback: could not restart the local proxy sidecar");
                }
            }
            UndoStep::StartLocalSandbox => {
                if let Err(e) = store.start(name).await {
                    tracing::warn!(target: "deploy", %name, error = %e, "rollback: could not restart the local sandbox");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_created_means_nothing_to_undo() {
        assert!(DeployUndo::default().steps().is_empty());
    }

    #[test]
    fn rollback_undoes_only_recorded_resources() {
        let undo = DeployUndo { remote_proxy: true, ..Default::default() };
        assert_eq!(undo.steps(), vec![UndoStep::RemoveRemoteProxy]);
        let undo = DeployUndo { remote_sandbox: true, local_sandbox_stopped: true, ..Default::default() };
        assert_eq!(undo.steps(), vec![UndoStep::RemoveRemoteSandbox, UndoStep::StartLocalSandbox]);
    }

    #[test]
    fn full_rollback_runs_remote_first_and_local_last() {
        let undo = DeployUndo {
            remote_proxy: true,
            remote_sandbox: true,
            watch_retargeted: true,
            local_sidecar_stopped: true,
            local_sandbox_stopped: true,
        };
        assert_eq!(
            undo.steps(),
            vec![
                UndoStep::RemoveRemoteSandbox,
                UndoStep::RemoveRemoteProxy,
                UndoStep::RetargetWatchLocal,
                UndoStep::StartLocalSidecar,
                UndoStep::StartLocalSandbox,
            ]
        );
    }

    #[test]
    fn collision_names_the_foreign_container() {
        let owned = ["web".to_string(), "web-proxy".to_string()];
        assert_eq!(remote_collision(["db", "web-proxy"], &owned), Some("web-proxy".to_string()));
        assert_eq!(remote_collision(["db"], &owned), None);
    }
}

/// Reverses a deploy: flushes remote edits, removes the remote sandbox and sidecar, tears the
/// session down and restarts the local sidecar and sandbox.
#[allow(clippy::too_many_arguments)]
pub async fn undeploy(
    name: &str,
    store: &Arc<dyn SandboxBackend>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
    registry: &Arc<ProviderRegistry>,
    flows: &FlowWatchRegistry,
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
    let remote = remote_site(registry, &session.host);
    crate::proxy::remove_remote_proxy(&remote, name).await;
    engine.terminate(name).await?;
    let local = SidecarSite::local(registry, ProviderId::Podman);
    crate::proxy::flowlog::retarget(app, flows, name, &local);
    // Order is load-bearing: the local sidecar was stopped by deploy and the local sandbox has no
    // network until it runs again.
    crate::proxy::ensure_sidecar_running(app, &local, name).await?;
    store.start(name).await?;
    crate::proxy::remove_remote_state(app, name);
    Ok(())
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
    crate::proxy::remove_remote_proxy(&remote_site(registry, &session.host), name).await;
    store.destroy(name).await?;
    crate::net_policy::remove_applied(app, name);
    cleanup_host_state(name, app);
    // Removes the local sidecar, its network and the whole state directory, `remote/` included.
    let (id, provider) = crate::proxy::sidecar_provider(app, registry, name);
    crate::proxy::remove_proxy(app, &provider, id, name).await;
    Ok(())
}

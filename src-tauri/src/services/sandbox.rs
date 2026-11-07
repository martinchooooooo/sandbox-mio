//! Sandbox create and teardown sequencing. Every ordering constraint in this file is load-bearing;
//! the comments say which and why.

use std::sync::Arc;

use crate::backends;
use crate::backends::types::{EgressRequest, Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;
use crate::sync::types::{SyncEngine, SyncStatus};
use crate::workspace;

/// Removes the host-side state a sandbox leaves outside its container: the managed SSH alias, the
/// identity directory (whose `known_hosts` is the pinned host key), and the pane state directory.
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

/// Creates a sandbox on `provider` and brings its workspace up. `proxy` opts the sandbox into a
/// per-sandbox mitmproxy sidecar (`docs/features/network-proxy/02_dev.md`); refused by `store.create`
/// for a provider/image that isn't exec-capable.
pub async fn create(
    name: &str,
    existing_path: Option<&str>,
    provider: ProviderId,
    proxy: bool,
    egress: Option<&EgressRequest>,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
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

    // Stage markers, so a failed create is attributable to a step from the log alone. The
    // microsandbox path spans four subsystems (provider, ssh, runtime, sync) and its failures
    // surface as one flattened string in the UI toast, which cannot say how far it got.
    tracing::info!(target: "create", %name, %provider, "create_sandbox: ensuring base image");
    store.ensure_base_image(provider).await?;

    // The sidecar runs on the sandbox's own provider
    // (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 2).
    let sidecar = registry.local(provider);
    let attachment = if proxy {
        tracing::info!(target: "create", %name, "create_sandbox: ensuring proxy image");
        crate::proxy::ensure_proxy_image(app, &sidecar).await?;
        tracing::info!(target: "create", %name, "create_sandbox: creating proxy sidecar");
        match crate::proxy::create_proxy(app, &sidecar, provider, name).await {
            Ok(a) => Some(a),
            Err(e) => {
                crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
                return Err(e.into());
            }
        }
    } else {
        None
    };

    tracing::info!(target: "create", %name, "create_sandbox: creating instance");
    let mut sandbox = match store.create(name, existing_path, provider, attachment.as_ref(), egress).await {
        Ok(s) => s,
        Err(e) => {
            // The proxy container and network exist ahead of the sandbox they attach to
            // (`docs/features/network-proxy/02_dev.md`, Concrete Model); a failed sandbox create
            // must not strand them.
            if proxy {
                crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            }
            return Err(e.into());
        }
    };
    if proxy && provider == ProviderId::Microsandbox {
        // No CA injection: the guest already trusts microsandbox's own interception CA
        // (`docs/features/microsandbox-network-policy/spike/01_egress-baseline.md`, Item 1).
        if let Err(e) = mark_secret_hosts(app, registry, name).await {
            tracing::error!(target: "create", %name, error = %e, "create_sandbox: secret-host marking failed, rolling back");
            if store.destroy(name).await.is_ok() {
                cleanup_host_state(name, app);
            }
            crate::net_policy::remove_applied(app, name);
            crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            return Err(e);
        }
    } else if proxy {
        tracing::info!(target: "create", %name, "create_sandbox: injecting proxy CA");
        if let Err(e) = inject_proxy_ca(app, &sidecar, name).await {
            tracing::error!(target: "create", %name, error = %e, "create_sandbox: CA injection failed, rolling back");
            match store.destroy(name).await {
                Ok(()) => cleanup_host_state(name, app),
                Err(destroy_err) => tracing::error!(
                    target: "create", %name, error = %destroy_err,
                    "create_sandbox: rollback after CA injection failure also failed"
                ),
            }
            crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            return Err(e);
        }
    }
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
            match store.destroy(name).await {
                Ok(()) => cleanup_host_state(name, app),
                Err(destroy_err) => tracing::error!(
                    target: "create", %name, error = %destroy_err,
                    "create_sandbox: rollback after sync failure also failed"
                ),
            }
            return Err(e.into());
        }
        tracing::info!(target: "create", %name, "create_sandbox: sync session created");
        sandbox.status = "running".to_string();
    }
    Ok(sandbox)
}

/// Recreates a microsandbox sandbox around its preserved workspace volume so the current
/// `.mio/net-policy.json` and the requested viewer state (`viewer: None` keeps it) take effect —
/// microsandbox fixes network rules at create (`docs/features/microsandbox-network-policy/02_dev.md`,
/// Phase G). Toolchains installed outside `/root/work` are reinstalled from the mise lock.
#[allow(clippy::too_many_arguments)]
pub async fn reapply_network(
    name: &str,
    viewer: Option<bool>,
    store: &Arc<dyn SandboxBackend>,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
    registry: &Arc<ProviderRegistry>,
    app: &tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    let (provider, _) = registry.provider_of(name).await;
    if provider != ProviderId::Microsandbox {
        return Err(crate::backends::types::BackendError::CommandFailed(format!(
            "{name:?}: re-applying network settings is only supported for microsandbox sandboxes"
        ))
        .into());
    }
    let had_viewer = crate::proxy::exists(app, registry, name).await;
    let want_viewer = viewer.unwrap_or(had_viewer);
    let sidecar = registry.local(provider);
    // Order is load-bearing: a new sidecar must exist before the agent, which pins its port and CA.
    let attachment = match (want_viewer, had_viewer) {
        (true, true) => Some(crate::proxy::msb_attachment(app, name)?),
        (true, false) => {
            crate::proxy::ensure_proxy_image(app, &sidecar).await?;
            match crate::proxy::create_proxy(app, &sidecar, provider, name).await {
                Ok(a) => Some(a),
                Err(e) => {
                    crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
                    return Err(e.into());
                }
            }
        }
        (false, _) => None,
    };

    close_panes_and_forwards(name, network, app).await;
    let session = backends::msb_sync_session(name);
    // Pull guest edits back to the host before the old VM goes away.
    let _ = engine.flush(&session).await;
    let _ = engine.terminate(&session).await;

    let sandbox = match store.recreate(name, attachment.as_ref(), None).await {
        Ok(s) => s,
        Err(e) => {
            if want_viewer && !had_viewer {
                crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
            }
            return Err(e.into());
        }
    };
    if had_viewer && !want_viewer {
        // Removed only after the agent: a stopped sidecar makes the agent fail closed, never bypass.
        crate::proxy::remove_proxy(app, &sidecar, provider, name).await;
    }
    if want_viewer {
        let applied = crate::net_policy::read_applied(app, name)
            .map_err(crate::backends::types::BackendError::from)?
            .unwrap_or_else(|| crate::net_policy::AppliedPolicy {
                policy: crate::net_policy::NetPolicy::default(),
                chained_proxy: true,
            });
        crate::proxy::write_secret_hosts(app, name, &applied)?;
        crate::proxy::restart_proxy_container(app, registry, name).await?;
    }

    runtime.init(&sandbox.workspace_path).await?;
    store.start(name).await?;
    engine.create(&session, &sandbox.workspace_path, &backends::msb_sync_beta(name)).await?;
    runtime.reproduce_remote(name, &registry.local_ref(provider), &sandbox.workspace_path).await?;
    Ok(sandbox)
}

/// Reads the proxy's CA cert off the host and pipes it into the freshly created sandbox.
async fn inject_proxy_ca(
    app: &tauri::AppHandle,
    podman: &Arc<dyn crate::provider::types::MachineProvider>,
    name: &str,
) -> Result<(), AppError> {
    let pem = crate::proxy::read_ca_pem(app, name)?;
    crate::proxy::inject_ca(podman, name, &pem).await?;
    Ok(())
}

/// Writes the sidecar's secret-host marker file from the applied policy and restarts the sidecar
/// so the addon reads it, only when a secret is bound (Resolved Decision 10).
async fn mark_secret_hosts(
    app: &tauri::AppHandle,
    registry: &Arc<ProviderRegistry>,
    name: &str,
) -> Result<(), AppError> {
    let applied = crate::net_policy::read_applied(app, name).map_err(crate::backends::types::BackendError::from)?;
    let Some(applied) = applied.filter(|a| !a.policy.secrets.is_empty()) else {
        return Ok(());
    };
    crate::proxy::write_secret_hosts(app, name, &applied)?;
    crate::proxy::restart_proxy_container(app, registry, name).await?;
    Ok(())
}

/// Removes `name`'s proxy sidecar (container, network, state directory) if one exists. Checked
/// first rather than called unconditionally, so destroying the overwhelming majority of
/// non-proxied sandboxes doesn't log a "container not found" warning on every teardown.
async fn remove_proxy_if_present(app: &tauri::AppHandle, registry: &Arc<ProviderRegistry>, name: &str) {
    if crate::proxy::exists(app, registry, name).await {
        let (id, provider) = crate::proxy::sidecar_provider(app, registry, name);
        crate::proxy::remove_proxy(app, &provider, id, name).await;
    }
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
        // Best-effort, and a no-op for a Podman sandbox, which has no workspace session: a
        // microsandbox sandbox's volume is gone with the instance, so its sync has nothing left to
        // reconcile and would otherwise keep retrying a dead ProxyCommand.
        let _ = engine.terminate(&backends::msb_sync_session(name)).await;
        remove_proxy_if_present(app, registry, name).await;
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

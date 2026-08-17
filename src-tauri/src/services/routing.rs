//! Sandbox addressing: which provider runs a sandbox, which machine its operations go to, and which
//! SSH alias its Mutagen forwards tunnel over.

use std::sync::Arc;

use crate::backends::types::SandboxBackend;
use crate::error::AppError;
use crate::provider::types::{MachineRef, ProviderId};
use crate::provider::ProviderRegistry;
use crate::sync::types::SyncEngine;

/// The facts every transport-sensitive command branches on, resolved in one pass. The provider
/// running the sandbox is `machine.provider` — `resolve_machine` carries it through both branches,
/// so it is not repeated here.
#[derive(Debug, Clone)]
pub struct SandboxRoute {
    /// The machine the sandbox's operations address.
    pub machine: MachineRef,
    /// Whether the sandbox was built for the exec transport rather than the publish-relay model.
    pub exec_capable: bool,
    /// The SSH alias the sandbox's Mutagen forwards tunnel over. Naming it does not create it —
    /// only [`SandboxRouter::resolve`] and [`SandboxRouter::resolve_for_ports`] write `~/.ssh`.
    pub forward_alias: String,
}

/// Resolves a sandbox name to a [`SandboxRoute`]. Registered once as Tauri state, so a command
/// spanning the backend, the sync engine, and the provider registry names one handle instead of
/// three (`docs/features/lib-decomposition/02_dev.md`, State Narrowing Rule).
///
/// Holds no cache: a route is re-read on every call, because a container stopped or removed outside
/// the app offers no invalidation hook (Resolved Decision 1).
pub struct SandboxRouter {
    store: Arc<dyn SandboxBackend>,
    engine: Arc<dyn SyncEngine>,
    registry: Arc<ProviderRegistry>,
}

impl SandboxRouter {
    pub fn new(
        store: Arc<dyn SandboxBackend>,
        engine: Arc<dyn SyncEngine>,
        registry: Arc<ProviderRegistry>,
    ) -> Self {
        Self { store, engine, registry }
    }

    /// Resolves a route without touching `~/.ssh`. For callers that need the machine or the
    /// provider but open no forward of their own.
    pub async fn locate(&self, name: &str) -> Result<SandboxRoute, AppError> {
        let (provider, exec_capable) = sandbox_facts(&self.store, name).await?;
        let machine = resolve_machine(name, provider, &self.engine, &self.registry).await?;
        let forward_alias = alias_name(name, &machine, exec_capable);
        Ok(SandboxRoute { machine, exec_capable, forward_alias })
    }

    /// [`Self::locate`] plus the managed `~/.ssh` block for the route's alias. Every forward opened
    /// against a route needs this: unlike the container, the SSH config is not guaranteed to
    /// survive between app runs, so the block is rewritten on every call.
    pub async fn resolve(
        &self,
        name: &str,
        app: &tauri::AppHandle,
    ) -> Result<SandboxRoute, AppError> {
        let route = self.locate(name).await?;
        ensure_forward_alias(name, &route.machine, route.exec_capable, app).await?;
        Ok(route)
    }

    /// [`Self::resolve`] for the port paths, which need the alias only for an exec-capable sandbox.
    /// A publish-relay sandbox is reached through its published port instead, and never had its
    /// alias written here.
    pub async fn resolve_for_ports(
        &self,
        name: &str,
        app: &tauri::AppHandle,
    ) -> Result<SandboxRoute, AppError> {
        let route = self.locate(name).await?;
        if route.exec_capable {
            ensure_forward_alias(name, &route.machine, route.exec_capable, app).await?;
        }
        Ok(route)
    }
}

/// The provider running `name` and which of the two reachability models it was built for — the
/// pair every transport-sensitive command branches on, read in one `list()` pass
/// (`docs/features/podman-exec-transport/02_dev.md`). A sandbox the backend no longer lists is
/// treated as a pre-feature Podman one, the conservative default.
async fn sandbox_facts(
    store: &Arc<dyn SandboxBackend>,
    name: &str,
) -> Result<(ProviderId, bool), AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map_or((ProviderId::Podman, false), |s| (s.provider, s.supports_exec_transport())))
}

/// The machine a sandbox's operations address: its deploy target when one exists, else the local
/// machine of its own provider.
async fn resolve_machine(
    name: &str,
    provider: ProviderId,
    engine: &Arc<dyn SyncEngine>,
    registry: &Arc<ProviderRegistry>,
) -> Result<MachineRef, AppError> {
    if provider != ProviderId::Podman {
        // Only Podman has remote targets (`ProviderCapabilities.remote_targets`). A microsandbox
        // sandbox's Mutagen session syncs its workspace volume rather than marking a deploy, and is
        // named apart from the sandbox for exactly that reason (`backends::msb_sync_session`).
        return Ok(registry.local_ref(provider));
    }
    Ok(engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| MachineRef { provider: ProviderId::Podman, id: s.host })
        .unwrap_or_else(|| registry.local_ref(ProviderId::Podman)))
}

/// The alias a sandbox's Mutagen forwards tunnel over: the sandbox itself when it is reached
/// through its own transport (microsandbox always, Podman when exec-capable), the shared Podman
/// connection otherwise. Must agree with the branch [`ensure_forward_alias`] writes.
fn alias_name(name: &str, machine: &MachineRef, exec_capable: bool) -> String {
    match machine.provider {
        ProviderId::Microsandbox => name.to_string(),
        ProviderId::Podman if exec_capable => name.to_string(),
        ProviderId::Podman => machine.id.clone(),
    }
}

/// Writes the managed `~/.ssh` block for the alias [`alias_name`] picks. Rewrites it on every call —
/// unlike the container, that file is not guaranteed to survive between app runs.
async fn ensure_forward_alias(
    name: &str,
    machine: &MachineRef,
    exec_capable: bool,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    match machine.provider {
        ProviderId::Microsandbox => {
            let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
            crate::backends::ensure_msb_sandbox_alias(name, &identity_path).await?;
        }
        ProviderId::Podman if exec_capable => {
            let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
            crate::backends::ensure_sandbox_alias(name, &machine.id, &identity_path).await?;
        }
        ProviderId::Podman => {
            crate::backends::ensure_connection_alias(&machine.id).await?;
        }
    }
    Ok(())
}

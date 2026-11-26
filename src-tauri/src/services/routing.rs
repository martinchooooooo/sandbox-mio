//! Sandbox addressing: which provider runs a sandbox, which machine its operations go to, and which
//! SSH alias its Mutagen forwards tunnel over.

use std::sync::Arc;

use crate::backends::types::SandboxBackend;
use crate::error::AppError;
use mio_domain::types::ProviderId;
use mio_provider::types::MachineRef;
use mio_provider::ProviderRegistry;
use crate::sync::types::SyncEngine;

/// Where a sandbox lives and how it is reached, resolved in one pass. The provider running the
/// sandbox is `machine.provider` — `resolve_machine` carries it through both branches, so it is
/// not repeated here.
#[derive(Debug, Clone)]
pub struct SandboxRoute {
    /// The machine the sandbox's operations address.
    pub machine: MachineRef,
    /// The SSH alias the sandbox's Mutagen forwards tunnel over. Naming it does not create it —
    /// only [`SandboxRouter::resolve`] writes `~/.ssh`.
    pub forward_alias: String,
    /// Whether the sandbox is known to be running: the backend lists it as `running` and the
    /// machine is its provider's local machine. A deployed sandbox is never `running` by this rule
    /// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 11).
    pub running: bool,
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
        let (provider, listed_running) = sandbox_provider(&self.store, name).await?;
        let machine = resolve_machine(name, provider, &self.engine, &self.registry).await?;
        let running = listed_running && self.is_local(&machine);
        // Every sandbox is reached through an alias named after itself.
        Ok(SandboxRoute { machine, forward_alias: name.to_string(), running })
    }

    /// The provider driving `machine`, for a caller that execs in the sandbox itself.
    pub fn provider_for(&self, machine: &MachineRef) -> Arc<dyn mio_provider::types::MachineProvider> {
        self.registry.provider_for(machine)
    }

    /// Whether `machine` is its provider's local machine, i.e. not a deploy target.
    pub fn is_local(&self, machine: &MachineRef) -> bool {
        *machine == self.registry.local_ref(machine.provider)
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
        ensure_forward_alias(name, &route.machine, app).await?;
        Ok(route)
    }
}

/// The provider running `name` and whether the backend lists it as running. A sandbox the backend
/// no longer lists is treated as a stopped Podman one.
async fn sandbox_provider(
    store: &Arc<dyn SandboxBackend>,
    name: &str,
) -> Result<(ProviderId, bool), AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map_or((ProviderId::Podman, false), |s| (s.provider, s.status == "running")))
}

/// The machine a sandbox's operations address: its deploy target when one exists, else the local
/// machine of its own provider.
async fn resolve_machine(
    name: &str,
    provider: ProviderId,
    engine: &Arc<dyn SyncEngine>,
    registry: &Arc<ProviderRegistry>,
) -> Result<MachineRef, AppError> {
    Ok(engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| MachineRef { provider, id: s.host })
        .unwrap_or_else(|| registry.local_ref(provider)))
}

/// Writes the managed `~/.ssh` block for the sandbox's own alias. Rewrites it on every call —
/// unlike the container, that file is not guaranteed to survive between app runs.
async fn ensure_forward_alias(
    name: &str,
    machine: &MachineRef,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    match machine.provider {
        ProviderId::Podman => {
            let identity_path = crate::ssh::identity::ensure_sandbox_identity(app, name)?;
            crate::ssh::identity::ensure_sandbox_alias(name, &machine.id, &identity_path).await?;
        }
    }
    Ok(())
}

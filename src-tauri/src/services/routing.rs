//! Sandbox addressing: which provider runs a sandbox, which machine its operations go to, and which
//! SSH alias its Mutagen forwards tunnel over.

use std::sync::Arc;

use crate::backends::types::SandboxBackend;
use crate::error::AppError;
use crate::provider::types::{MachineRef, ProviderId};
use crate::provider::ProviderRegistry;
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
        let provider = sandbox_provider(&self.store, name).await?;
        let machine = resolve_machine(name, provider, &self.engine, &self.registry).await?;
        // Every sandbox is reached through an alias named after itself.
        Ok(SandboxRoute { machine, forward_alias: name.to_string() })
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

/// The provider running `name`. A sandbox the backend no longer lists is treated as a Podman one.
async fn sandbox_provider(
    store: &Arc<dyn SandboxBackend>,
    name: &str,
) -> Result<ProviderId, AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map_or(ProviderId::Podman, |s| s.provider))
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

/// Writes the managed `~/.ssh` block for the sandbox's own alias. Rewrites it on every call —
/// unlike the container, that file is not guaranteed to survive between app runs.
async fn ensure_forward_alias(
    name: &str,
    machine: &MachineRef,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    match machine.provider {
        ProviderId::Microsandbox => {
            let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
            crate::backends::ensure_msb_sandbox_alias(name, &identity_path).await?;
        }
        ProviderId::Podman => {
            let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
            crate::backends::ensure_sandbox_alias(name, &machine.id, &identity_path).await?;
        }
    }
    Ok(())
}

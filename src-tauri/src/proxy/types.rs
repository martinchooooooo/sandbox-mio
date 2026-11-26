//! Types for the per-sandbox proxy sidecar: the subsystem's own error, the site a sidecar runs on,
//! and the frontend-facing status shape. See `docs/features/network-proxy/02_dev.md`.

use std::sync::Arc;

use crate::domain::types::{BackendError, ProviderId};
use crate::provider::types::{MachineProvider, MachineRef};
use crate::provider::ProviderRegistry;

/// Errors from the `proxy` subsystem. Converts into [`BackendError`] at the caller boundary — the
/// subsystem has no `AppError` variant of its own, matching `credentials::types::CredentialError`'s
/// shape.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("{0}")]
    CommandFailed(String),
    #[error("failed to read {path}: {error}")]
    Io { path: String, error: String },
    #[error(transparent)]
    Provider(#[from] crate::provider::types::ProviderError),
}

impl From<ProxyError> for BackendError {
    fn from(e: ProxyError) -> Self {
        BackendError::CommandFailed(e.to_string())
    }
}

/// Where a sandbox's active sidecar runs: the local machine, or the deploy target while the sandbox is
/// deployed. Callers resolve it (`services::proxy::locate_sidecar`) so `proxy/` never touches the sync
/// engine (`docs/features/proxy-remote-deploy/02_dev.md`, The Concrete Model).
#[derive(Clone)]
pub struct SidecarSite {
    pub machine: MachineRef,
    pub provider: Arc<dyn MachineProvider>,
    /// Whether `machine` is the provider's local machine rather than a deploy target.
    pub local: bool,
}

impl SidecarSite {
    /// The local machine of `provider_id`.
    pub fn local(registry: &ProviderRegistry, provider_id: ProviderId) -> Self {
        Self {
            machine: registry.local_ref(provider_id),
            provider: registry.local(provider_id),
            local: true,
        }
    }
}

/// Frontend-facing proxy status. Derived from the `<sandbox>-proxy` sidecar's presence rather
/// than persisted (`docs/features/network-proxy/02_dev.md`, Resolved Decision 3).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    /// Whether the sidecar exists, running or not. A stopped sidecar still means the viewer is on.
    pub exists: bool,
    pub running: bool,
    pub proxy_ip: String,
    pub network: String,
    pub provider: ProviderId,
    pub engine: ProxyEngine,
    /// The deploy target's connection when the sidecar runs remotely; `None` locally.
    pub host: Option<String>,
}

/// The proxy program a sidecar runs, recorded as `meta.json`'s `engine`. One variant today; kept
/// as the seam for another engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProxyEngine {
    IronProxy,
}

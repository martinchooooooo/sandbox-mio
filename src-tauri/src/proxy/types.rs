//! Types for the per-sandbox mitmproxy sidecar: the subsystem's own error, the attachment
//! `SandboxBackend::create` needs to build a proxy-enabled `InstanceSpec`, and the frontend-facing
//! status shape. See `docs/features/network-proxy/02_dev.md`.

use crate::backends::types::BackendError;

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

/// The container-name suffix that marks a Podman container as a proxy sidecar rather than a
/// sandbox — filtered out of `list_sandboxes` (`commands/sandbox.rs`) the same way
/// `credentials::types::AUTH_CONTAINER_PREFIX` filters out throwaway auth containers.
pub const PROXY_CONTAINER_SUFFIX: &str = "-proxy";

/// What `SandboxBackend::create` needs to chain a sandbox through its proxy sidecar.
pub enum ProxyAttachment {
    /// Podman: the network to join *instead of* `pasta`, and the environment (proxy vars, CA trust
    /// vars) to stamp on the sandbox container at create time.
    PodmanBridge { network: String, env: Vec<(String, String)> },
    /// Microsandbox: the sidecar's SOCKS5 port on host loopback, the variable the agent `msb`
    /// child reads its password from, and the sidecar CA the agent trusts upstream
    /// (`docs/features/microsandbox-network-policy/02_dev.md`, The Concrete Model).
    MsbChain { host_port: u16, password_env: String, upstream_ca_path: String },
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
    pub provider: crate::provider::types::ProviderId,
    /// The SOCKS5 port a microsandbox sidecar publishes; `None` for Podman.
    pub host_port: Option<u16>,
    /// Whether a running microsandbox sidecar answers a SOCKS5 greeting; `None` for Podman or a
    /// stopped sidecar.
    pub healthy: Option<bool>,
}

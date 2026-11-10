use serde::Serialize;

/// Event emitted after every forward mutation so the frontend stays live without polling.
pub const EVENT_NAME: &str = "network://forwards-changed";

/// What a forward carries. `Exposed` holds the user-chosen target port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, std::hash::Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "targetPort")]
pub enum ServiceKind {
    Terminal,
    AgentWeb,
    /// User-exposed port (the value is the target port inside sandbox).
    Exposed(u16),
    /// Reverse forward offering the host inference server on the sandbox's own loopback.
    Inference,
}

/// Errors from [`NetworkManager`]. Converts into [`crate::error::AppError`] via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("failed to allocate a Windows-host port: {0}")]
    PortAllocation(String),
    #[error(transparent)]
    Sync(#[from] crate::sync::types::SyncError),
    #[error("{0}")]
    CommandFailed(String),
}

impl From<crate::provider::types::ProviderError> for NetworkError {
    fn from(e: crate::provider::types::ProviderError) -> Self {
        NetworkError::CommandFailed(e.to_string())
    }
}

/// One active Mutagen forward, tracked so the frontend can display open ports and teardown
/// can reverse exactly what was opened.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub sandbox: String,
    pub kind: ServiceKind,
    /// Windows-host loopback port for frontend/webview connection. For an `Inference` forward, the
    /// host port the forward dials instead.
    pub host_port: u16,
    /// The SSH alias the forward tunnels over.
    pub connection: String,
    /// The sandbox-side port the forward's far end dials: the alias lands inside the sandbox, so
    /// no host-side mapping is involved.
    pub remote_port: u16,
    /// Mutagen session name (e.g., `terminal-<sandbox>`, `expose-<sandbox>-<port>`).
    pub forward_name: String,
    /// Short name from `.mio/ports.json` (container-declared) or frontend UI (user-exposed).
    /// `None` for auto-detected ports. Guides whether frontend shows manual close button.
    pub label: Option<String>,
    /// Port was auto-detected from listening sockets, not declared in `.mio/ports.json`.
    /// Persists as long as the port is listening (like file-declared entries).
    pub auto_detected: bool,
}

/// Mutagen-forward lifecycle and port allocation for terminal, agent-web, inference and user-exposed
/// ports.
#[async_trait::async_trait]
pub trait NetworkManager: Send + Sync {
    /// Allocate a fresh host loopback port, create/replace a Mutagen forward to the remote port,
    /// register it, and return the host port.
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        alias: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError>;

    /// Create or replace a reverse forward for (`sandbox`, `kind`): a listener on the sandbox's
    /// loopback at `guest_port`, reached over `alias`, relaying to host loopback `host_port`. The
    /// listener binds guest loopback only, so only processes inside the sandbox can dial it. In the
    /// registered [`ForwardInfo`], `host_port` is the destination port rather than a host listener,
    /// and `remote_port` is the guest listener.
    async fn open_reverse_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        alias: &str,
        guest_port: u16,
        host_port: u16,
    ) -> Result<(), NetworkError>;

    /// Tear down the forward for (`sandbox`, `kind`). Best-effort.
    async fn close_forward(&self, sandbox: &str, kind: ServiceKind);

    /// Tear down every forward for `sandbox` (Terminal, AgentWeb, Inference, and Exposed ports).
    async fn close_sandbox(&self, sandbox: &str);

    /// Return snapshot of all tracked forwards for the frontend's open-ports view.
    async fn list_forwards(&self) -> Vec<ForwardInfo>;

    /// Clean up Mutagen sessions orphaned by a crashed previous run (not in `live_sandboxes`).
    async fn sweep(&self, live_sandboxes: &[String]);

    /// Expose `target_port` from sandbox to a fresh host loopback port by forwarding straight to
    /// it over the sandbox's alias. Does not recreate the container. `workspace_path` initializes
    /// `.mio/ports.json` if absent.
    async fn expose_port(
        &self,
        sandbox: &str,
        machine: &crate::provider::types::MachineRef,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
        auto_detected: bool,
    ) -> Result<u16, NetworkError>;

    /// Remove the forward for `target_port`. Best-effort.
    async fn unexpose_port(&self, sandbox: &str, target_port: u16);

    /// Poll sandbox for port changes (`.mio/ports.json` and listening sockets). Idempotent.
    /// Spawned task holds `Arc<self>` so it can call `expose_port`/`unexpose_port` on mutations.
    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        machine: &crate::provider::types::MachineRef,
    );
}

use serde::Serialize;

/// Event emitted after every forward mutation so the frontend stays live without polling.
pub const EVENT_NAME: &str = "network://forwards-changed";

/// What a forward carries. `Exposed` holds the user-chosen target port bridged via relay-pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, std::hash::Hash, Serialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "targetPort")]
pub enum ServiceKind {
    Terminal,
    AgentWeb,
    /// User-exposed port (the value is the target port inside sandbox).
    Exposed(u16),
}

/// Errors from [`NetworkManager`]. Converts into [`crate::error::AppError`] via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    #[error("failed to allocate a Windows-host port: {0}")]
    PortAllocation(String),
    #[error("no free relay slot for sandbox {sandbox:?} (limit {limit})")]
    NoFreeSlot { sandbox: String, limit: usize },
    #[error(transparent)]
    Sync(#[from] crate::sync::types::SyncError),
    #[error("{0}")]
    CommandFailed(String),
}

/// One active Mutagen forward, tracked so the frontend can display open ports and teardown
/// can reverse exactly what was opened.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInfo {
    pub sandbox: String,
    pub kind: ServiceKind,
    /// Windows-host loopback port for frontend/webview connection.
    pub host_port: u16,
    /// Podman connection or SSH alias the forward tunnels over.
    pub connection: String,
    /// Container-side port (podman-published for Terminal/AgentWeb, relay-pool slot for Exposed).
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

/// Mutagen-forward lifecycle and port allocation for terminal, agent-web, and user-exposed ports.
#[async_trait::async_trait]
pub trait NetworkManager: Send + Sync {
    /// Allocate a fresh host loopback port, create/replace a Mutagen forward to the remote port,
    /// register it, and return the host port.
    async fn open_forward(
        &self,
        sandbox: &str,
        kind: ServiceKind,
        connection: &str,
        remote_port: u16,
    ) -> Result<u16, NetworkError>;

    /// Tear down the forward for (`sandbox`, `kind`). Best-effort.
    async fn close_forward(&self, sandbox: &str, kind: ServiceKind);

    /// Tear down every forward for `sandbox` (Terminal, AgentWeb, and Exposed ports).
    async fn close_sandbox(&self, sandbox: &str);

    /// Return snapshot of all tracked forwards for the frontend's open-ports view.
    async fn list_forwards(&self) -> Vec<ForwardInfo>;

    /// Clean up Mutagen sessions orphaned by a crashed previous run (not in `live_sandboxes`).
    async fn sweep(&self, live_sandboxes: &[String]);

    /// Expose `target_port` from sandbox to a fresh host loopback port via relay-pool slot.
    /// Does not recreate the container. `workspace_path` initializes `.mio/ports.json` if absent.
    async fn expose_port(
        &self,
        sandbox: &str,
        connection: &str,
        workspace_path: &str,
        target_port: u16,
        label: Option<String>,
        auto_detected: bool,
    ) -> Result<u16, NetworkError>;

    /// Remove the relay and forward for `target_port`, freeing its pool slot. Best-effort.
    async fn unexpose_port(&self, sandbox: &str, target_port: u16);

    /// Poll sandbox for port changes (`.mio/ports.json` and listening sockets). Idempotent.
    /// Spawned task holds `Arc<self>` so it can call `expose_port`/`unexpose_port` on mutations.
    async fn watch_ports_file(
        self: std::sync::Arc<Self>,
        sandbox: &str,
        workspace_path: &str,
        connection: &str,
    );
}

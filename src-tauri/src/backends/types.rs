use serde::Serialize;
use tauri::ipc::Channel;

use crate::ssh::types::SshError;

/// Errors from sandbox backend operations.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// Sandbox name failed Podman's character/length rules.
    #[error("invalid sandbox name {name:?}: use letters, digits, '_', '.' or '-', starting with a letter or digit")]
    InvalidName { name: String },
    /// Platform Documents directory not found (headless environment).
    #[error("could not resolve the Documents directory")]
    DocumentsDirNotFound,
    /// Failed to create sandbox's workspace directory.
    #[error("failed to create workspace {path}: {error}")]
    WorkspaceCreate { path: String, error: String },
    /// Provided existing_path does not exist.
    #[error("directory not found: {path}")]
    PathNotFound { path: String },
    /// Provided existing_path is a file, not a directory.
    #[error("not a directory: {path}")]
    PathNotADirectory { path: String },
    /// Provided existing_path is already tracked as a sandbox workspace.
    #[error("{path} is already a sandbox workspace")]
    PathAlreadyTracked { path: String },
    /// Podman or SSH process failed or exited with non-zero status.
    #[error("{0}")]
    CommandFailed(String),
    /// Failed to parse JSON or URL from podman output or SSH URL argument.
    #[error("{0}")]
    ParseError(String),
    /// Deploy target is the same connection as the local container.
    #[error("cannot deploy {name:?} onto its own connection {connection:?}; the local container already runs there")]
    SelfDeploy { name: String, connection: String },
    /// Deploy target has no SSH endpoint (e.g., unix-socket-only local connection).
    #[error("deploy target {connection:?} has no SSH endpoint to sync over")]
    NoSshEndpoint { connection: String },
    /// Error from SSH config management.
    #[error(transparent)]
    Ssh(#[from] SshError),
}

/// A sandbox instance (returned by all backend operations).
#[derive(Debug, Serialize, Clone)]
pub struct Sandbox {
    pub id: String,
    pub name: String,
    /// Backend-defined status (e.g., `"created"`, `"running"`, `"exited"`).
    pub status: String,
    /// Laptop workspace directory (the bind-mount source for local, sync alpha for deployed).
    pub workspace_path: String,
    /// Image version for updates (None for legacy/non-mio containers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_version: Option<u32>,
}

/// Mutagen endpoints for a freshly deployed sandbox (returned by `deploy`).
pub struct RemoteDeployment {
    /// Local (alpha) endpoint: the laptop workspace directory.
    pub alpha: String,
    /// Remote (beta) endpoint: scp-style URL (`alias:path`).
    pub beta: String,
}

/// Workspace location and sync strategy (used by deploy to compute bind-mount and SSH endpoint).
pub(super) struct WorkspacePlan {
    /// Path bind-mounted at `/root/work` in the container.
    pub bind_mount_source: String,
    /// Laptop workspace directory (sync alpha endpoint for deployed sandboxes).
    pub laptop_dir: String,
    pub strategy: SyncStrategy,
}

/// How a sandbox's bind-mount source is synchronized with its laptop directory.
pub(super) enum SyncStrategy {
    /// Local: both paths are the same directory (no sync needed).
    Local,
    /// Remote: bind-mount is on remote host, mirrored via sync engine over SSH.
    Remote(ConnectionInfo),
}

/// Live SSH endpoint for a Podman connection (read fresh, port changes at each boot).
#[derive(Debug, Clone)]
pub(super) struct ConnectionInfo {
    pub host: String,
    pub port: String,
    pub user: String,
    /// Private key registered with Podman for this connection.
    pub identity: String,
}

/// Primary interface for sandbox lifecycle management. Each method maps to a `podman` CLI call
/// on the configured system connection (local Podman machine or remote host over SSH).
#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Build base image if absent (idempotent; image is never in a registry).
    async fn ensure_base_image(&self) -> Result<(), BackendError>;
    /// Create a stopped container with workspace directory bind-mounted at `/root/work`.
    /// With `existing_path: None`, uses `<Documents>/sandbox-mio/<name>`; with Some, validates and uses it.
    async fn create(&self, name: &str, existing_path: Option<&str>) -> Result<Sandbox, BackendError>;
    /// Return all containers (running and stopped).
    async fn list(&self) -> Result<Vec<Sandbox>, BackendError>;
    /// Start a stopped container.
    async fn start(&self, name: &str) -> Result<(), BackendError>;
    /// Stop a running container.
    async fn stop(&self, name: &str) -> Result<(), BackendError>;
    /// Remove the local container (workspace directory is left intact).
    async fn destroy(&self, name: &str) -> Result<(), BackendError>;
    /// Create remote container and return Mutagen sync endpoints.
    /// Local container is left stopped (deploy is reversible).
    async fn deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<RemoteDeployment, BackendError>;
    /// Remove remote container (workspace directory left intact).
    async fn remove_remote(&self, name: &str, connection: &str) -> Result<(), BackendError>;
    /// Stream container events to frontend channel.
    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), BackendError>;

    /// Return names of all registered Podman system connections.
    async fn list_hosts(&self) -> Result<Vec<String>, BackendError>;
    /// Register a remote host as a Podman system connection.
    async fn add_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity: Option<&str>,
    ) -> Result<(), BackendError>;
    /// Enable rootless podman.socket and user lingering on a reachable remote host over SSH.
    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), BackendError>;
    /// Guided setup for external Linux server: generate SSH key, enable socket, register, verify.
    /// Prerequisite: public key must be pre-authorized (`ssh-copy-id` manually).
    async fn provision_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity_path: &str,
        on_progress: Channel<serde_json::Value>,
    ) -> Result<(), BackendError>;
}

/// Real [`SandboxBackend`] driving the `podman` CLI over a named system connection.
pub(super) struct PodmanCliBackend {
    pub connection: String,
    /// Serializes base image build to prevent concurrent races.
    pub(super) build_lock: tokio::sync::Mutex<()>,
}

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

impl Sandbox {
    /// Whether this sandbox was built from an image new enough to have `sshd`/
    /// `sshd_config.container` baked in, and so can use the podman-exec-transport reachability
    /// model (`SshAlias::Proxied` + `PassthroughPortTransport`) instead of publish+relay.
    pub fn supports_exec_transport(&self) -> bool {
        self.image_version
            .is_some_and(|v| v >= crate::backends::EXEC_TRANSPORT_MIN_IMAGE_VERSION)
    }
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
    /// A Mutagen-managed volume owned by the provider itself, not a bind-mountable directory.
    /// Unconstructed until a provider needs it (`docs/features/mutagen-transport/02_dev.md`).
    #[allow(dead_code)]
    Volume(ConnectionInfo),
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
    /// Whether the base image already exists on the active connection, without building it.
    /// Query-only counterpart to `ensure_base_image` for UI that must distinguish "already done"
    /// from "not started yet" (the welcome/onboarding replay screen).
    async fn base_image_ready(&self) -> Result<bool, BackendError>;
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

/// Real [`SandboxBackend`] driving the `podman` CLI over a named system connection. Container
/// lifecycle delegates to `provider`; the Podman-only orchestration (deploy, host registry,
/// event stream, exec-transport SSH aliases) stays here.
pub(super) struct PodmanCliBackend {
    pub connection: String,
    /// Container/VM primitive the lifecycle methods delegate to, pinned to `connection`.
    pub(super) provider: std::sync::Arc<dyn crate::provider::types::MachineProvider>,
    /// The process seam the still-Podman-only `&self` calls route through (host registry, event
    /// stream, connection lookup) — real in production, scripted under `MIO_MOCK_BACKEND=1`.
    /// See `docs/features/scriptable-mock-backend/02_dev.md`.
    pub(super) runner: std::sync::Arc<dyn crate::mockctl::types::ProcessRunner>,
    /// Resolves `crate::paths::app_data_root` for per-sandbox exec-transport SSH identities
    /// (`ensure_sandbox_identity`). Not used by any pre-existing method.
    pub(super) app: tauri::AppHandle,
}

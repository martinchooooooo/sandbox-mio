use serde::Serialize;
use tauri::ipc::Channel;

use crate::ssh::types::SshError;

/// Errors from sandbox backend operations.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// Sandbox name failed Podman's character/length rules.
    #[error("invalid sandbox name {name:?}: use letters, digits, '_', '.' or '-', starting with a letter or digit")]
    InvalidName { name: String },
    /// A sandbox of this name already exists on some provider. Names are unique across the union of
    /// every provider's instances, not per provider, because every other command addresses a
    /// sandbox by name alone.
    #[error("a sandbox named {name:?} already exists on {provider}")]
    NameTaken { name: String, provider: crate::provider::types::ProviderId },
    /// The name ends in the suffix that names a sandbox's proxy sidecar, so a sandbox called
    /// `x-proxy` would be taken for `x`'s sidecar (`crate::proxy::types::PROXY_CONTAINER_SUFFIX`).
    #[error("invalid sandbox name {name:?}: names ending in \"-proxy\" are reserved for traffic-inspection sidecars")]
    ReservedName { name: String },
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
    /// A podman invocation produced no exit status at all: it was still running when its bound
    /// elapsed, or it was not attempted because the seam is already known unresponsive. Distinct
    /// from `CommandFailed`, which always carries something podman actually said, and from
    /// `AppError::PodmanUnavailable`, whose remedies (`setupPodman`/`startPodman` in
    /// `src/lib/podman/setup.svelte.ts`) do not apply to a binary that never answers.
    #[error("{0}")]
    PodmanUnresponsive(String),
    /// Failed to parse JSON or URL from podman output or SSH URL argument.
    #[error("{0}")]
    ParseError(String),
    /// Deploy target is the same connection as the local container.
    #[error("cannot deploy {name:?} onto its own connection {connection:?}; the local container already runs there")]
    SelfDeploy { name: String, connection: String },
    /// Deploy was asked for a sandbox whose provider has no remote-target concept
    /// (`ProviderCapabilities.remote_targets`).
    #[error("{name:?} runs on a provider that cannot deploy to a remote host")]
    RemoteTargetsUnsupported { name: String },
    /// Deploy target has no SSH endpoint (e.g., unix-socket-only local connection).
    #[error("deploy target {connection:?} has no SSH endpoint to sync over")]
    NoSshEndpoint { connection: String },
    /// Error from SSH config management.
    #[error(transparent)]
    Ssh(#[from] SshError),
    /// `deploy` was asked for a sandbox that has a proxy sidecar. Remote proxy creation is out of
    /// scope for v1 (`docs/features/network-proxy/02_dev.md`, Resolved Decision 6) and deploying
    /// without the proxy would silently move an inspected sandbox onto an uninspected network.
    #[error("{name:?} has traffic inspection enabled, which cannot be deployed to a remote host")]
    ProxyDeployUnsupported { name: String },
}

/// Classifies a raw spawn/wait failure from the process seam. `TimedOut` is the marker
/// `crate::process` sets for "podman produced no exit status", so no caller has to match on
/// message text.
impl From<std::io::Error> for BackendError {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::TimedOut => BackendError::PodmanUnresponsive(e.to_string()),
            _ => BackendError::CommandFailed(e.to_string()),
        }
    }
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
    /// Image version for updates (None for non-mio containers).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_version: Option<u32>,
    /// Which engine runs this sandbox. Every command addresses a sandbox by name alone, so this is
    /// what lets the command layer resolve the name back to a provider without asking both.
    pub provider: crate::provider::types::ProviderId,
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
    /// A Mutagen-managed volume owned by the provider itself, not a bind-mountable directory (a
    /// microsandbox sandbox's disk volume). No payload: the sync `beta` URL is `<name>:<mount>`
    /// (`backends::msb_sync_beta`), resolved entirely through the sandbox's own `SshAlias::Proxied`
    /// (`ensure_msb_sandbox_alias`) — unlike `Remote`, there is no separate host/port/user/identity
    /// to carry, since the alias's `ProxyCommand` already is the full connection.
    /// `docs/features/mutagen-transport/02_dev.md` originally sketched this as `Volume(ConnectionInfo)`;
    /// corrected in `docs/features/microsandbox-provider/02_dev.md`, Phase H, once a concrete
    /// consumer showed `ConnectionInfo`'s host/port/identity fields don't apply here.
    #[allow(dead_code)]
    Volume,
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
    /// Build `provider`'s base image if absent (idempotent; image is never in a registry).
    async fn ensure_base_image(
        &self,
        provider: crate::provider::types::ProviderId,
    ) -> Result<(), BackendError>;
    /// Whether `provider`'s base image already exists, without building it. Query-only counterpart
    /// to `ensure_base_image` for UI that must distinguish "already done" from "not started yet"
    /// (the welcome/onboarding replay screen).
    async fn base_image_ready(
        &self,
        provider: crate::provider::types::ProviderId,
    ) -> Result<bool, BackendError>;
    /// Create a stopped instance on `provider` with the workspace reachable at `/root/work` — a
    /// host bind mount for Podman, a synced disk volume for microsandbox. With `existing_path:
    /// None`, uses `<Documents>/sandbox-mio/<name>`; with Some, validates and uses it. `proxy`,
    /// when `Some`, attaches the sandbox to its proxy sidecar's network and environment in place of
    /// the provider's default network mode (`docs/features/network-proxy/02_dev.md`).
    /// `egress` is the create dialog's network choice; only a provider that enforces
    /// `.mio/net-policy.json` itself accepts it (`docs/features/microsandbox-network-policy/02_dev.md`).
    async fn create(
        &self,
        name: &str,
        existing_path: Option<&str>,
        provider: crate::provider::types::ProviderId,
        proxy: Option<&crate::proxy::types::ProxyAttachment>,
        egress: Option<&EgressRequest>,
    ) -> Result<Sandbox, BackendError>;
    /// Replace `name`'s instance under the same name around its preserved workspace, with the
    /// current `.mio/net-policy.json` and the given viewer attachment. Microsandbox only for now
    /// (`docs/features/microsandbox-network-policy/02_dev.md`, Phase G).
    async fn recreate(
        &self,
        name: &str,
        proxy: Option<&crate::proxy::types::ProxyAttachment>,
        egress: Option<&EgressRequest>,
    ) -> Result<Sandbox, BackendError>;
    /// Return every provider's instances (running and stopped), each tagged with its provider.
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

/// Real [`SandboxBackend`]. Instance lifecycle dispatches through `registry` to whichever provider
/// owns the sandbox, so it is provider-neutral there; the orchestration that *is* Podman-only
/// (deploy, host registry, event stream) stays here and still drives the `podman` CLI over
/// `connection` directly — hence `Cli`, not `Podman`, in the name.
pub(super) struct CliBackend {
    pub connection: String,
    /// Resolves each operation to the provider that owns the sandbox
    /// (`docs/features/microsandbox-provider/02_dev.md`, Phase J).
    pub(super) registry: std::sync::Arc<crate::provider::ProviderRegistry>,
    /// The process seam the still-Podman-only `&self` calls route through (host registry, event
    /// stream, connection lookup) — real in production, scripted under `MIO_MOCK_BACKEND=1`.
    /// See `docs/features/scriptable-mock-backend/02_dev.md`.
    pub(super) runner: std::sync::Arc<dyn crate::process::types::ProcessRunner>,
    /// Resolves `crate::paths::app_data_root` for per-sandbox exec-transport SSH identities
    /// (`ensure_sandbox_identity`). Not used by any pre-existing method.
    pub(super) app: tauri::AppHandle,
    /// Stop signal for the live `listen_events` stream; the next call replaces it, ending the old
    /// stream. Needed because `Channel::send` into a reloaded webview still succeeds, so a stream
    /// never notices its subscriber is gone.
    pub(super) events_stop: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// The create dialog's network choice for [`SandboxBackend::create`]. `mode: None` keeps whatever
/// the workspace's `.mio/net-policy.json` says; `starter_hosts` are agent API hosts merged into a
/// new allowlist (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 13).
#[derive(Debug, Clone, Default)]
pub struct EgressRequest {
    pub mode: Option<crate::net_policy::EgressMode>,
    pub starter_hosts: Vec<String>,
}

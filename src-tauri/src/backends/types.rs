use tauri::ipc::Channel;

use crate::domain::types::{BackendError, ProviderId, Sandbox};

/// Mutagen endpoints for a freshly deployed sandbox (returned by `deploy`).
pub struct RemoteDeployment {
    /// Local (alpha) endpoint: the laptop workspace directory.
    pub alpha: String,
    /// Remote (beta) endpoint: scp-style URL (`alias:path`).
    pub beta: String,
}

/// What `SandboxBackend::plan_deploy` validated and resolved, handed back to `deploy` so the target
/// is read once. Mutating nothing, it is safe to drop.
pub struct DeployPlan {
    /// The target connection.
    pub target: String,
    pub(super) workspace: WorkspacePlan,
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
    /// Build `provider`'s base image if absent (idempotent; image is never in a registry).
    async fn ensure_base_image(
        &self,
        provider: ProviderId,
    ) -> Result<(), BackendError>;
    /// Whether `provider`'s base image already exists, without building it. Query-only counterpart
    /// to `ensure_base_image` for UI that must distinguish "already done" from "not started yet"
    /// (the welcome/onboarding replay screen).
    async fn base_image_ready(
        &self,
        provider: ProviderId,
    ) -> Result<bool, BackendError>;
    /// Create a stopped instance on `provider` with the workspace reachable at `/root/work` — a
    /// host bind mount for Podman. With `existing_path:
    /// None`, uses `<Documents>/sandbox-mio/<name>`; with Some, validates and uses it. `proxy`,
    /// when `Some`, attaches the sandbox to its proxy sidecar's network and environment in place of
    /// the provider's default network mode (`docs/features/network-proxy/02_dev.md`).
    /// `egress` is the create dialog's network choice; only a provider that enforces
    /// `.mio/net-policy.json` itself accepts it.
    /// A failure after the instance exists removes the instance, its SSH alias and keypair, and its
    /// accepted policy, best-effort (a failed step is only logged). The workspace directory and any
    /// `.mio/net-policy.json` written into it stay, including in a pre-existing project folder.
    async fn create(
        &self,
        name: &str,
        existing_path: Option<&str>,
        provider: ProviderId,
        proxy: Option<&crate::proxy::types::ProxyAttachment>,
        egress: Option<&EgressRequest>,
    ) -> Result<Sandbox, BackendError>;
    /// Replace `name`'s instance under the same name around its preserved workspace, with the
    /// current `.mio/net-policy.json` and the given viewer attachment. Refused for every provider
    /// until `docs/features/proxy-toggle-management/02_dev.md` builds the Podman recreate.
    #[allow(dead_code)]
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
    /// Validates a deploy of `name` to `target_connection` without mutating anything: the name, the
    /// provider's remote-target capability, self-deploy, the target's SSH endpoint, and that the
    /// target's Podman can host the egress boundary.
    async fn plan_deploy(&self, name: &str, target_connection: &str) -> Result<DeployPlan, BackendError>;
    /// Create remote container and return Mutagen sync endpoints.
    /// Local container is left stopped (deploy is reversible). `proxy` attaches the remote container
    /// to its remote sidecar; `extra_env` adds the secret-binding placeholders.
    async fn deploy(
        &self,
        name: &str,
        plan: &DeployPlan,
        proxy: &crate::proxy::types::ProxyAttachment,
        extra_env: &[(String, String)],
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
    /// Resolves each operation to the provider that owns the sandbox.
    pub(super) registry: std::sync::Arc<crate::provider::ProviderRegistry>,
    /// The process seam the still-Podman-only `&self` calls route through (host registry, event
    /// stream, connection lookup) — real in production, scripted under `MIO_MOCK_BACKEND=1`.
    /// See `docs/features/scriptable-mock-backend/02_dev.md`.
    pub(super) runner: std::sync::Arc<dyn crate::process::types::ProcessRunner>,
    /// Resolves `crate::paths::app_data_root` for per-sandbox exec-transport SSH identities
    /// (`ssh::identity::ensure_sandbox_identity`). Not used by any pre-existing method.
    pub(super) app: tauri::AppHandle,
    /// Stop signal for the live `listen_events` stream; the next call replaces it, ending the old
    /// stream. Needed because `Channel::send` into a reloaded webview still succeeds, so a stream
    /// never notices its subscriber is gone.
    pub(super) events_stop: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// The create dialog's network choice for [`SandboxBackend::create`]. `mode: None` keeps whatever
/// the workspace's `.mio/net-policy.json` says; `starter_hosts` are agent API hosts merged into a
/// new allowlist.
#[derive(Debug, Clone, Default)]
pub struct EgressRequest {
    pub mode: Option<crate::net_policy::EgressMode>,
    pub starter_hosts: Vec<String>,
}

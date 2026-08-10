use async_trait::async_trait;

/// One tool from the mise registry. Returned by `registry()` to populate agent install UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistryEntry {
    /// Short tool name (e.g., "node", "python").
    pub short: String,
    /// Human-readable description from the registry.
    pub description: Option<String>,
}

/// Errors from [`RuntimeManager`]. Converts into [`crate::error::AppError`] via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("invalid package name {name:?}: use letters, digits or '-', '_', '.', '/', '#', ':', '@', '+'")]
    InvalidPackage { name: String },
    #[error("{0}")]
    CommandFailed(String),
    #[error("failed to read mise manifest at {path}: {error}")]
    ManifestRead { path: String, error: String },
    #[error("failed to parse mise manifest: {0}")]
    ManifestParse(String),
    /// The web terminal could not be reached: token or published-port parsing failed, or the
    /// server did not come online. Distinct from [`RuntimeError::CommandFailed`] so the message
    /// names the terminal-specific cause.
    #[error("terminal unavailable: {0}")]
    TerminalUnavailable(String),
    #[error("agents UI unavailable: {0}")]
    AgentWebUnavailable(String),
}

impl From<crate::provider::types::ProviderError> for RuntimeError {
    fn from(e: crate::provider::types::ProviderError) -> Self {
        match e {
            crate::provider::types::ProviderError::CommandFailed(m) => RuntimeError::CommandFailed(m),
            crate::provider::types::ProviderError::ParseError(m) => RuntimeError::CommandFailed(m),
        }
    }
}

/// Manages the mise tool environment embedded in a sandbox workspace. Distinct from
/// [`crate::backends::types::SandboxBackend`], which owns container lifecycle. The sole
/// implementation is [`MiseRuntime`], held in Tauri state as `Arc<dyn RuntimeManager>`.
#[async_trait]
pub trait RuntimeManager: Send + Sync {
    /// Create `mise.toml` and empty `mise.lock` if absent. Idempotent.
    async fn init(&self, workspace_host_path: &str) -> Result<(), RuntimeError>;

    /// Install `package` via `mise use`, rewriting the manifest.
    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// Uninstall `package` via `mise rm`, rewriting the manifest.
    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// List installed tools from the manifest. Works for stopped and deployed sandboxes.
    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError>;

    /// Seed manifest onto remote container and run `mise install` to download lock-pinned binaries.
    async fn reproduce_remote(
        &self,
        name: &str,
        target_connection: &str,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError>;

    /// Start zellij web server in sandbox, mint login token, return `(token, container_port)`.
    /// Caller bridges container_port to Windows loopback via Mutagen forward. `exec_capable`
    /// selects which `PortTransport` resolves the returned port — see
    /// `docs/features/podman-exec-transport/02_dev.md`.
    async fn prepare_terminal(
        &self,
        name: &str,
        connection: &str,
        workspace_host_path: &str,
        exec_capable: bool,
    ) -> Result<(String, u16), RuntimeError>;

    /// Start paseo agent-web daemon in sandbox, return published container port. `exec_capable`
    /// selects which `PortTransport` resolves the returned port.
    async fn prepare_agent_web(
        &self,
        name: &str,
        connection: &str,
        password: &str,
        exec_capable: bool,
    ) -> Result<u16, RuntimeError>;

    /// The Podman connection this runtime is pinned to. Fallback for local sandboxes.
    fn local_connection(&self) -> &str;

    /// List available tools from the mise registry (process-wide cached).
    async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>;

    /// Return the install directory for `tool` in sandbox (used by agent postinstall).
    async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, RuntimeError>;

    /// Execute a command in sandbox's workspace and return stdout (starts container if needed).
    async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, RuntimeError>;
}

/// Drives `mise` through a [`crate::provider::types::MachineProvider`] pinned to a named Podman
/// **system connection**, mirroring [`crate::backends::types::PodmanCliBackend`]. The connection is
/// the same one the backend is pinned to; operations against an arbitrary target connection
/// (`reproduce_remote`, `prepare_terminal`, `prepare_agent_web`) build a target-pinned provider
/// from `runner`, exactly as `deploy` does in `backends`.
pub struct MiseRuntime {
    pub connection: String,
    /// Provider pinned to `connection`, driving every local `podman exec`/`cp`/`start`.
    pub provider: std::sync::Arc<dyn crate::provider::types::MachineProvider>,
    /// The process seam used to build a fresh provider pinned to an arbitrary target connection,
    /// mirroring `deploy`'s `self.runner`.
    pub runner: std::sync::Arc<dyn crate::mockctl::types::ProcessRunner>,
    pub registry_cache: tokio::sync::OnceCell<Vec<RegistryEntry>>,
    /// `podman port`-based lookup, used for a sandbox on the publish+relay reachability model.
    pub published_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
    /// Trivial passthrough, used for a sandbox on the podman-exec-transport reachability model.
    pub exec_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
}

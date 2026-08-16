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
            crate::provider::types::ProviderError::PodmanUnavailable(m) => {
                RuntimeError::CommandFailed(m)
            }
            // No distinct `RuntimeError` variant: nothing on the runtime path branches on the
            // condition, and the message already names it. The kind survives where it is acted on
            // (`BackendError::PodmanUnresponsive`).
            crate::provider::types::ProviderError::Unresponsive(m) => RuntimeError::CommandFailed(m),
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
        target: &crate::provider::types::MachineRef,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError>;

    /// Start zellij web server in sandbox, mint login token, return `(token, container_port)`.
    /// Caller bridges container_port to Windows loopback via Mutagen forward. `exec_capable`
    /// selects which `PortTransport` resolves the returned port — see
    /// `docs/features/podman-exec-transport/02_dev.md`.
    async fn prepare_terminal(
        &self,
        name: &str,
        machine: &crate::provider::types::MachineRef,
        workspace_host_path: &str,
        exec_capable: bool,
    ) -> Result<(String, u16), RuntimeError>;

    /// Start paseo agent-web daemon in sandbox, return published container port. `exec_capable`
    /// selects which `PortTransport` resolves the returned port.
    async fn prepare_agent_web(
        &self,
        name: &str,
        machine: &crate::provider::types::MachineRef,
        password: &str,
        exec_capable: bool,
    ) -> Result<u16, RuntimeError>;

    /// List available tools from the mise registry (process-wide cached).
    async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>;

    /// Return the install directory for `tool` in sandbox (used by agent postinstall).
    async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, RuntimeError>;

    /// Execute a command in sandbox's workspace and return stdout (starts container if needed).
    async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, RuntimeError>;
}

/// Drives `mise` through whichever [`crate::provider::types::MachineProvider`] owns the sandbox.
/// The methods that take a [`crate::provider::types::MachineRef`] (`reproduce_remote`,
/// `prepare_terminal`, `prepare_agent_web`) resolve it directly; the name-only methods resolve the
/// sandbox's owner through `ProviderRegistry::provider_of`.
pub struct MiseRuntime {
    /// Resolves each call to the provider running the sandbox
    /// (`docs/features/microsandbox-provider/02_dev.md`, Phase J).
    pub providers: std::sync::Arc<crate::provider::ProviderRegistry>,
    pub registry_cache: tokio::sync::OnceCell<Vec<RegistryEntry>>,
    /// `podman port`-based lookup, used for a sandbox on the publish+relay reachability model.
    pub published_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
    /// Trivial passthrough, used for a sandbox on the podman-exec-transport reachability model.
    pub exec_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
}

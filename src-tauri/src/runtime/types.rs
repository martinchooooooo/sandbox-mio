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
    /// The web terminal could not be reached: token parsing failed, or the
    /// server did not come online. Distinct from [`RuntimeError::CommandFailed`] so the message
    /// names the terminal-specific cause.
    #[error("terminal unavailable: {0}")]
    TerminalUnavailable(String),
    #[error("agents UI unavailable: {0}")]
    AgentWebUnavailable(String),
}

impl RuntimeError {
    /// [`RuntimeError::TerminalUnavailable`] from any displayable cause.
    pub fn terminal_unavailable(cause: impl std::fmt::Display) -> Self {
        RuntimeError::TerminalUnavailable(cause.to_string())
    }

    /// [`RuntimeError::AgentWebUnavailable`] from any displayable cause.
    pub fn agent_web_unavailable(cause: impl std::fmt::Display) -> Self {
        RuntimeError::AgentWebUnavailable(cause.to_string())
    }
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
            e @ crate::provider::types::ProviderError::PortInUse(_) => {
                RuntimeError::CommandFailed(e.to_string())
            }
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
    /// Caller bridges container_port to Windows loopback via Mutagen forward.
    async fn prepare_terminal(
        &self,
        name: &str,
        machine: &crate::provider::types::MachineRef,
        workspace_host_path: &str,
    ) -> Result<(String, u16), RuntimeError>;

    /// Start paseo agent-web daemon in sandbox, return its container port.
    async fn prepare_agent_web(
        &self,
        name: &str,
        machine: &crate::provider::types::MachineRef,
        password: &str,
    ) -> Result<u16, RuntimeError>;

    /// List available tools from the mise registry (process-wide cached).
    async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>;

    /// Return the install directory for `tool` in sandbox (used by agent postinstall).
    async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, RuntimeError>;

    /// Execute a command in sandbox's workspace and return stdout (starts container if needed).
    /// Carries the toolchain secret environment, since the agent postinstall `mise exec` can
    /// download tools.
    async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, RuntimeError>;
}

/// Secret environment for in-sandbox toolchain calls that resolve or download tools (the GitHub
/// token of `docs/features/techdebt-sweep/02_dev.md`, Resolved Decision 3). Implemented over the
/// credential store by `services::toolchain_secrets`, so this module never reads app state itself.
/// Empty when nothing is saved; values are never logged.
pub trait ToolchainSecrets: Send + Sync {
    fn env(&self) -> Vec<(String, String)>;
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
    /// Carried by every call that resolves or downloads tools.
    pub secrets: std::sync::Arc<dyn ToolchainSecrets>,
}

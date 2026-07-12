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

/// Manages the mise tool environment embedded in a sandbox workspace. Distinct from
/// [`crate::backends::types::SandboxBackend`], which owns container lifecycle. The sole
/// implementation is [`PodmanMiseRuntime`], held in Tauri state as `Arc<dyn RuntimeManager>`.
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
    /// Caller bridges container_port to Windows loopback via Mutagen forward.
    async fn prepare_terminal(
        &self,
        name: &str,
        connection: &str,
        workspace_host_path: &str,
    ) -> Result<(String, u16), RuntimeError>;

    /// Start paseo agent-web daemon in sandbox, return published container port.
    async fn prepare_agent_web(
        &self,
        name: &str,
        connection: &str,
        password: &str,
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

/// Drives `mise` (over `podman exec`) against a named Podman **system connection**, mirroring
/// [`crate::backends::types::PodmanCliBackend`]. The connection is the same one the backend is
/// pinned to; deploy passes an explicit target connection per call.
pub struct PodmanMiseRuntime {
    pub connection: String,
    pub registry_cache: tokio::sync::OnceCell<Vec<RegistryEntry>>,
}

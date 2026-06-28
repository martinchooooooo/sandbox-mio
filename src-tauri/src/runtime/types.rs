use async_trait::async_trait;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RegistryEntry {
    pub short: String,
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
}

/// Manages the mise tool environment embedded in a sandbox workspace. Distinct from
/// [`crate::backends::types::SandboxBackend`], which owns container lifecycle. The sole
/// implementation is [`PodmanMiseRuntime`], held in Tauri state as `Arc<dyn RuntimeManager>`.
#[async_trait]
pub trait RuntimeManager: Send + Sync {
    /// Idempotent: ensures the workspace holds a `mise.toml` (intent) and an empty `mise.lock`
    /// (so installs maintain the lockfile); a no-op if `mise.toml` already exists there.
    async fn init(&self, workspace_host_path: &str) -> Result<(), RuntimeError>;

    /// Install `package` via `mise use`, downloading the prebuilt binary and rewriting the manifest.
    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// Remove `package` via `mise rm`, rewriting the manifest.
    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// Read `[tools]` entry names from `<workspace_host_path>/mise.toml`. Reads the host filesystem
    /// directly, so it works for stopped and deployed sandboxes.
    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError>;

    /// Seed `mise.toml`/`mise.lock` onto the remote container and run `mise install` to download the
    /// lock-pinned binaries from the upstream release hosts. Called after the remote container is created.
    async fn reproduce_remote(
        &self,
        name: &str,
        target_connection: &str,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError>;

    /// Start the zellij web server in sandbox `name` over `connection` (idempotently), mint a
    /// one-time login token, and return `(token, container_port)` where `container_port` is the
    /// ephemeral port on `connection`'s host loopback that the container published for the zellij
    /// web server. The caller creates the Mutagen forwarding session that bridges a Windows-host
    /// port to `container_port`, then persists the complete session (token + URL) host-side.
    /// Works for any Podman connection.
    async fn prepare_terminal(
        &self,
        name: &str,
        connection: &str,
        workspace_host_path: &str,
    ) -> Result<(String, u16), RuntimeError>;

    /// The Podman connection this runtime is pinned to, read from `PODMAN_CONNECTION` (or the
    /// compiled-in default). Used by `open_terminal` as the fallback connection for local sandboxes.
    fn local_connection(&self) -> &str;

    /// Run `mise registry --json` in container `name` and return the full tool list.
    /// Implementations cache the result process-wide; the first call pays the `podman exec` cost.
    async fn registry(&self, name: &str) -> Result<Vec<RegistryEntry>, RuntimeError>;

    /// Return the absolute install directory for `tool` inside container `name` (via `mise where`).
    /// Used by agent postinstall steps that need a path into the tool's installation.
    async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, RuntimeError>;

    /// Run an arbitrary command inside container `name` in the workspace directory and return stdout.
    /// Starts the container first (idempotent). Extension point for agent postinstall steps.
    async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, RuntimeError>;
}

/// Drives `mise` (over `podman exec`) against a named Podman **system connection**, mirroring
/// [`crate::backends::types::PodmanCliBackend`]. The connection is the same one the backend is
/// pinned to; deploy passes an explicit target connection per call.
pub struct PodmanMiseRuntime {
    pub connection: String,
    pub registry_cache: tokio::sync::OnceCell<Vec<RegistryEntry>>,
}

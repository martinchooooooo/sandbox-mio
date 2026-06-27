use async_trait::async_trait;

/// Errors from [`RuntimeManager`]. Converts into [`crate::error::AppError`] via `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("invalid package name {name:?}: use letters, digits, '-', '_', '.', '/' or '#'")]
    InvalidPackage { name: String },
    #[error("{0}")]
    CommandFailed(String),
    #[error("failed to read flox manifest at {path}: {error}")]
    ManifestRead { path: String, error: String },
    #[error("failed to parse flox manifest: {0}")]
    ManifestParse(String),
    /// The web terminal could not be reached: token or published-port parsing failed, or the
    /// server did not come online. Distinct from [`RuntimeError::CommandFailed`] so the message
    /// names the terminal-specific cause.
    #[error("terminal unavailable: {0}")]
    TerminalUnavailable(String),
}

/// Result of [`RuntimeManager::open_terminal`]: the loopback URL of the running zellij web client
/// and the one-time login token minted for it. Serialized to `{ url, token }` for the frontend,
/// which opens `url` in a `WebviewWindow` and surfaces `token` for the one-time login.
#[derive(serde::Serialize)]
pub struct TerminalSession {
    pub url: String,
    pub token: String,
}

/// Manages the flox software environment embedded in a sandbox workspace. Distinct from
/// [`crate::backends::types::SandboxBackend`], which owns container lifecycle. The sole
/// implementation is [`PodmanFloxRuntime`], held in Tauri state as `Arc<dyn RuntimeManager>`.
#[async_trait]
pub trait RuntimeManager: Send + Sync {
    /// Idempotent: runs `flox init` in a throwaway container bound to `workspace_host_path`;
    /// skips if `.flox/` already exists there. `image` is the flox-baked base image.
    async fn init(&self, image: &str, workspace_host_path: &str) -> Result<(), RuntimeError>;

    /// Install `package` via `flox install`, realizing its closure and rewriting the manifest.
    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// Remove `package` via `flox uninstall`, rewriting the manifest.
    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError>;

    /// Read `[install]` entry names from `<workspace_host_path>/.flox/env/manifest.toml`.
    /// Reads the host filesystem directly, so it works for stopped and deployed sandboxes.
    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError>;

    /// Seed `.flox/env/` onto the remote container and run `flox activate` to substitute the
    /// locked closure from the binary cache. Called after the remote container is created.
    async fn reproduce_remote(
        &self,
        name: &str,
        target_connection: &str,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError>;

    /// Start the zellij web server in sandbox `name` (idempotently), mint a one-time login token,
    /// read the host port Podman published for the container's web port, and return the `{ url,
    /// token }` pair. Also persists the token host-side to `<workspace_host_path>/.mio/zellij.json`
    /// (a deliberate, documented workaround — see `docs/techdebt.md`). Local sandboxes only:
    /// `create` publishes the port for local sandboxes alone.
    async fn open_terminal(
        &self,
        name: &str,
        workspace_host_path: &str,
    ) -> Result<TerminalSession, RuntimeError>;
}

/// Drives `flox` (over `podman exec`) against a named Podman **system connection**, mirroring
/// [`crate::backends::types::PodmanCliBackend`]. The connection is the same one the backend is
/// pinned to; deploy passes an explicit target connection per call.
pub struct PodmanFloxRuntime {
    pub connection: String,
}

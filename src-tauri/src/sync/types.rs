use serde::Serialize;

/// Errors produced by the sync engine ([`SyncEngine`] and its helpers).
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Creating or resolving the app-private Mutagen data directory failed.
    #[error("failed to create Mutagen data directory: {0}")]
    DataDir(String),
    /// The bundled Mutagen sidecar could not be resolved or spawned.
    #[error("sidecar error: {0}")]
    Sidecar(String),
    /// A Mutagen command exited with a non-zero status. The captured stderr is in the payload.
    #[error("{0}")]
    CommandFailed(String),
    /// A JSON parse step failed on output from the Mutagen daemon.
    #[error("{0}")]
    ParseError(String),
    /// No active sync session exists for the given sandbox name.
    #[error("no sync session named {name:?}")]
    SessionNotFound { name: String },
}

/// Sync state for one session, collapsed by [`crate::sync::status_from_session`] from Mutagen's
/// finer states into the four values the frontend renders as a coloured dot.
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SyncStatus {
    Syncing,
    UpToDate,
    Conflict,
    Error,
}

/// One active sync session, the unit of deployment state. A session named after a sandbox means
/// that sandbox is deployed to `host` (the OpenSSH alias = Podman connection parsed from the beta
/// URL). The frontend overlays this onto the local sandbox list; `destroy_sandbox` and
/// `undeploy_sandbox` read `host` to reach the remote container.
#[derive(Debug, Serialize, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub host: String,
    pub status: SyncStatus,
}

/// Live file-sync engine between a remote sandbox workspace and the laptop directory.
///
/// Tauri state holds an `Arc<dyn SyncEngine>`; commands delegate to it via [`tauri::State`], the
/// same pattern as [`crate::backends::types::SandboxBackend`]. The only implementation is
/// [`MutagenCli`]. Each method maps to one Mutagen `sync` subcommand on the session named `name`
/// (one named session per sandbox).
#[async_trait::async_trait]
pub trait SyncEngine: Send + Sync {
    /// Create a two-way-resolved session named `name` between local path `alpha` and remote
    /// endpoint `beta` (a `host-alias:path` scp-style URL). Terminates any prior session of the
    /// same name first, so the call is idempotent across restarts.
    async fn create(&self, name: &str, alpha: &str, beta: &str) -> Result<(), SyncError>;
    /// Pause the session, halting propagation while preserving the session and its conflict
    /// state. Backs the "sync with remote" toggle's off position.
    async fn pause(&self, name: &str) -> Result<(), SyncError>;
    /// Resume a paused session without re-scanning from scratch. Backs the toggle's on position.
    async fn resume(&self, name: &str) -> Result<(), SyncError>;
    /// Permanently remove the session. Reserved for sandbox destruction and undeploy, never the
    /// toggle.
    async fn terminate(&self, name: &str) -> Result<(), SyncError>;
    /// Force a synchronous reconcile and block until it completes, so a caller can read a
    /// settled state instead of racing the filesystem watcher's debounce.
    async fn flush(&self, name: &str) -> Result<(), SyncError>;
    /// Report the collapsed [`SyncStatus`] for `name`.
    async fn status(&self, name: &str) -> Result<SyncStatus, SyncError>;
    /// List every active session as deployment state — name, host alias parsed from the beta URL,
    /// and collapsed status. Backs the `sandbox_deployments` command and the destroy/undeploy
    /// lookups that need a deployed sandbox's target connection.
    async fn sessions(&self) -> Result<Vec<SessionInfo>, SyncError>;
    /// Best-effort stop of the app-private daemon on exit, so the app does not orphan the daemon
    /// that auto-started on its first session command. The daemon is pinned to the app's
    /// `MUTAGEN_DATA_DIRECTORY`, so this never touches a user-installed Mutagen.
    async fn shutdown(&self);
}

/// Drives the bundled Mutagen sidecar via [`crate::sync::command`]. Holds the [`tauri::AppHandle`]
/// the sidecar is resolved through.
pub struct MutagenCli {
    pub app: tauri::AppHandle,
}

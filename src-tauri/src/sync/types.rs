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

/// Collapsed sync state for one session (rendered as frontend status dot).
#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SyncStatus {
    Syncing,
    UpToDate,
    Conflict,
    Error,
}

/// One active sync session (the unit of deployment state). Session name = sandbox name means
/// the sandbox is deployed to `host`. Used by destroy/undeploy to reach the remote container.
#[derive(Debug, Serialize, Clone)]
pub struct SessionInfo {
    pub name: String,
    pub host: String,
    pub status: SyncStatus,
}

/// Live file-sync engine between remote and local sandboxes. Each method maps to a Mutagen
/// `sync` subcommand on a named session (one per sandbox).
#[async_trait::async_trait]
pub trait SyncEngine: Send + Sync {
    /// Create a two-way session between local `alpha` and remote `beta` (scp-style URL).
    /// Idempotent: terminates any prior session of the same name first.
    async fn create(&self, name: &str, alpha: &str, beta: &str) -> Result<(), SyncError>;
    /// Pause the session (halt propagation, preserve conflict state). Toggle off.
    async fn pause(&self, name: &str) -> Result<(), SyncError>;
    /// Resume a paused session without re-scanning. Toggle on.
    async fn resume(&self, name: &str) -> Result<(), SyncError>;
    /// Permanently remove the session (destroy/undeploy only, not the toggle).
    async fn terminate(&self, name: &str) -> Result<(), SyncError>;
    /// Force synchronous reconcile; block until settled (don't race the watcher's debounce).
    async fn flush(&self, name: &str) -> Result<(), SyncError>;
    /// Return collapsed status for the session.
    async fn status(&self, name: &str) -> Result<SyncStatus, SyncError>;
    /// Return all active sessions with their names, hosts, and statuses.
    async fn sessions(&self) -> Result<Vec<SessionInfo>, SyncError>;
    /// Best-effort daemon shutdown on app exit (prevent orphaning).
    async fn shutdown(&self);
}

/// Real [`SyncEngine`] driving the bundled Mutagen sidecar. Runs unmodified in the e2e `light` tier
/// against a scripted `runner`; there is no separate mock implementation of this trait.
pub struct MutagenCli {
    /// Every `sync` subcommand this struct issues goes through here — real sidecar in production,
    /// scripted in the `light` tier (`docs/features/scriptable-mock-backend/02_dev.md`, Phase C).
    /// The runner owns the [`tauri::AppHandle`] the sidecar is resolved through.
    pub runner: std::sync::Arc<dyn crate::mockctl::types::ProcessRunner>,
}

use serde::Serialize;

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
    // Wired to remote sandbox creation in the Phase D lifecycle integration; no caller yet.
    #[allow(dead_code)]
    async fn create(&self, name: &str, alpha: &str, beta: &str) -> Result<(), String>;
    /// Pause the session, halting propagation while preserving the session and its conflict
    /// state. Backs the "sync with remote" toggle's off position.
    async fn pause(&self, name: &str) -> Result<(), String>;
    /// Resume a paused session without re-scanning from scratch. Backs the toggle's on position.
    async fn resume(&self, name: &str) -> Result<(), String>;
    /// Permanently remove the session. Reserved for sandbox destruction, never the toggle.
    // Wired to remote sandbox destruction in the Phase D lifecycle integration; no caller yet.
    #[allow(dead_code)]
    async fn terminate(&self, name: &str) -> Result<(), String>;
    /// Force a synchronous reconcile and block until it completes, so a caller can read a
    /// settled state instead of racing the filesystem watcher's debounce.
    // Wired to the destroy guard / status checks in the Phase D lifecycle integration; no caller yet.
    #[allow(dead_code)]
    async fn flush(&self, name: &str) -> Result<(), String>;
    /// Report the collapsed [`SyncStatus`] for `name`.
    async fn status(&self, name: &str) -> Result<SyncStatus, String>;
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

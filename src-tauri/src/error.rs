use crate::agents::types::AgentError;
use crate::backends::types::BackendError;
use crate::credentials::types::CredentialError;
use crate::inference::types::InferenceError;
use crate::network::types::NetworkError;
use crate::runtime::types::RuntimeError;
use crate::settings::types::SettingsError;
use crate::sync::types::SyncError;
use crate::workspace::types::WorkspaceError;

/// Top-level error type for Tauri commands (the only type crossing IPC boundary).
/// Sub-module errors convert via `#[from]`, so handlers can use `?` freely.
/// Serializes as `{ kind: string, message: string }` so frontend branches on `kind`, not message text.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Transparent: Backend subsystem error.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// Transparent: Sync engine error.
    #[error(transparent)]
    Sync(#[from] SyncError),
    /// Transparent: Runtime (mise/container) error.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// Transparent: Network (forwards) error.
    #[error(transparent)]
    Network(#[from] NetworkError),
    /// Transparent: Agent auth or install error.
    #[error(transparent)]
    Agent(#[from] AgentError),
    /// Transparent: Local inference error.
    #[error(transparent)]
    Inference(#[from] InferenceError),
    /// Transparent: Settings persistence error.
    #[error(transparent)]
    Settings(#[from] SettingsError),
    /// Transparent: Workspace/pane error.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    /// Transparent: Credential store error.
    #[error(transparent)]
    Credential(#[from] CredentialError),
    /// Sandbox is not deployed (no active sync session). Returned by undeploy/destroy when checking deployment state.
    #[error("sandbox {0:?} is not deployed")]
    NotDeployed(String),
    /// Sandbox has unsynced remote changes. Returned by destroy_sandbox when force=false and changes exist.
    #[error("sandbox {name:?} has unsynced changes on {host}; confirm to destroy and discard them")]
    UnsyncedChanges { name: String, host: String },
    /// Podman machine unavailable (stopped or rootless socket unresponsive). Returned by machine-readiness checks.
    #[error("Podman is not ready: {0}")]
    PodmanUnavailable(String),
    /// WSL2 installed; reboot required before machine init can proceed. Returned by provision_podman when install succeeds.
    #[error("Windows must be restarted to finish installing WSL2: {0}")]
    PodmanRebootRequired(String),
    /// Mock control plane misuse: called outside `MIO_MOCK_BACKEND=1`, an unknown override key, or
    /// a payload that doesn't match the key's real error/return-value shape.
    #[error("mock control plane: {0}")]
    MockControl(String),
}

impl AppError {
    /// Return a stable discriminant matching the variant name. Used by frontend to branch on error kind
    /// with full type safety (AppErrorKind union in src/lib/errors.ts must stay in sync).
    fn kind(&self) -> &'static str {
        match self {
            AppError::Backend(_) => "Backend",
            AppError::Sync(_) => "Sync",
            AppError::Runtime(_) => "Runtime",
            AppError::Network(_) => "Network",
            AppError::Agent(_) => "Agent",
            AppError::Inference(_) => "Inference",
            AppError::Settings(_) => "Settings",
            AppError::Workspace(_) => "Workspace",
            AppError::Credential(_) => "Credential",
            AppError::NotDeployed(_) => "NotDeployed",
            AppError::UnsyncedChanges { .. } => "UnsyncedChanges",
            AppError::PodmanUnavailable(_) => "PodmanUnavailable",
            AppError::PodmanRebootRequired(_) => "PodmanRebootRequired",
            AppError::MockControl(_) => "MockControl",
        }
    }
}

/// Serialize as `{ kind, message }` for IPC. Frontend parses `kind` (stable discriminant) and handles
/// variant-specific logic; `message` is the human-readable error for display. Every variant arm in
/// `kind()` must be mirrored in frontend's `AppErrorKind` union (AGENTS.md Common Gotchas).
impl serde::Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("kind", self.kind())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::provider::types::MachineRef;

/// Everything one attach needs, resolved by the caller so the subsystem reads no app state.
#[derive(Debug, Clone)]
pub struct AttachRequest {
    /// The sandbox, which also names its zellij session.
    pub name: String,
    pub machine: MachineRef,
    /// Whether the sandbox must be started before the exec
    /// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 11).
    pub start: bool,
    /// The sandbox's private key, installed for `root` inside the sandbox.
    pub identity_path: String,
    /// The per-sandbox known-hosts file the server key is checked against and learned into.
    pub known_hosts_path: String,
    pub cols: u32,
    pub rows: u32,
}

/// A session-level event on an attach's JSON channel. Terminal bytes never travel here. Mirrored
/// by `TerminalEvent` in `src/lib/terminal/TerminalView.svelte`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TerminalEvent {
    /// The session ended; the zellij session inside the sandbox may still run.
    Exited { reason: String, exit_status: Option<u32> },
}

/// Errors from [`TerminalHost`]. Every variant converts into `AppError::Terminal` (kind
/// `"Terminal"`), since each failure offers the same Retry
/// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 15).
#[derive(Debug, thiserror::Error)]
pub enum TerminalError {
    /// The engine exec carrying the SSH transport could not be spawned.
    #[error("could not start the terminal transport: {0}")]
    Spawn(String),
    /// The SSH handshake, channel open or pseudo-terminal request failed.
    #[error("the terminal connection failed: {0}")]
    Handshake(String),
    /// The sandbox presented a host key that differs from the one pinned for it.
    #[error("the sandbox's host key changed; delete its known_hosts file to trust the new key")]
    HostKeyChanged,
    /// The sandbox refused the sandbox identity.
    #[error("the sandbox refused the terminal's key: {0}")]
    Auth(String),
    /// Creating or resurrecting the zellij session failed.
    #[error("could not create the terminal session: {0}")]
    SessionCreate(String),
    /// No live session exists for the sandbox.
    #[error("the terminal is not connected")]
    NotAttached,
    /// The attach was ended by a detach or replaced by a newer attach before it finished.
    #[error("the terminal attach was cancelled")]
    Cancelled,
    /// An engine call (start, exec) failed.
    #[error("{0}")]
    Provider(String),
}

impl From<crate::provider::types::ProviderError> for TerminalError {
    fn from(e: crate::provider::types::ProviderError) -> Self {
        TerminalError::Provider(e.to_string())
    }
}

/// Host side of the native terminal: holds at most one SSH session per sandbox, attached to the
/// sandbox's zellij session. Real impl `SshTerminalHost` carries SSH over the engine's streamed
/// exec; `MockTerminalHost` echoes input under `MIO_MOCK_BACKEND=1`.
#[async_trait::async_trait]
pub trait TerminalHost: Send + Sync {
    /// Opens a session for `req.name`, replacing any earlier one. A detach or newer attach while
    /// this one is opening cancels it with [`TerminalError::Cancelled`]. Raw output bytes go to
    /// `on_output`; the final [`TerminalEvent::Exited`] goes to `on_event`. Returns once `sshd` has
    /// accepted the zellij client command.
    async fn attach(
        &self,
        req: AttachRequest,
        on_output: Channel<InvokeResponseBody>,
        on_event: Channel<TerminalEvent>,
    ) -> Result<(), TerminalError>;
    /// Writes input bytes to the live session.
    async fn input(&self, name: &str, bytes: Vec<u8>) -> Result<(), TerminalError>;
    /// Sends a window-change to the live session.
    async fn resize(&self, name: &str, cols: u32, rows: u32) -> Result<(), TerminalError>;
    /// Ends the live session for `name`, if any. The zellij session keeps running.
    async fn detach(&self, name: &str);
    /// Ends every live session and waits (bounded) for each to release its exec child; called on
    /// app exit.
    async fn detach_all(&self);
}

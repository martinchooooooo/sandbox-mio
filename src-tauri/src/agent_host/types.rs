use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;

use crate::provider::types::MachineRef;

/// A position in one supervisor generation's journal. A cursor from an older generation is served
/// from offset 0, because pi restarted and the old offsets mean nothing in the new journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub generation: String,
    pub offset: u64,
}

/// What `pi-host.sh ensure` reports: the running generation, the pi version it started, and the
/// dialogs pi is blocked on, derived from the journal and the input log
/// (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 3).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostStatus {
    pub generation: String,
    /// Empty when `pi --version` printed nothing recognisable.
    pub pi_version: String,
    /// Whether this ensure started a new supervisor.
    pub started: bool,
    /// Raw `extension_ui_request` records with no matching response, oldest first.
    pub pending_dialogs: Vec<serde_json::Value>,
}

/// The `mio_attach` header of one attach stream. `writer` is false when another client holds the
/// supervisor's writer lock; the stream is then read-only and `send` is refused.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AttachInfo {
    pub generation: String,
    pub offset: u64,
    pub writer: bool,
}

/// One message on an attach's channel: a journal record with its byte span, or a synthetic
/// `mio_detached` record (both offsets `None`) when the stream ends.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AttachEvent {
    pub offset: Option<u64>,
    pub end: Option<u64>,
    pub record: serde_json::Value,
}

/// Errors from [`AgentHost`]. Converts into `AppError::AgentHost` (kind `"AgentHost"`), except
/// `Deployed`, which keeps its own kind so the chat can show an empty state for it.
#[derive(Debug, thiserror::Error)]
pub enum AgentHostError {
    /// The supervisor is not running, or pi failed to start (stderr tail attached).
    #[error("the agent is not running: {0}")]
    NotRunning(String),
    /// This client is a reader: another client holds the writer lock.
    #[error("another window controls this agent")]
    ReadOnly,
    /// The sandbox is deployed to a remote host, which the native chat does not support in v1
    /// (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 11).
    #[error("the chat is not available for a deployed sandbox")]
    Deployed,
    /// Installing the supervisor scripts into the sandbox failed.
    #[error("installing the agent supervisor failed: {0}")]
    ScriptInstall(String),
    /// The attach stream could not be opened or read, or there is no live attach to write to.
    #[error("agent stream error: {0}")]
    Stream(String),
    /// The engine exec itself failed.
    #[error("{0}")]
    Provider(String),
}

impl From<crate::provider::types::ProviderError> for AgentHostError {
    fn from(e: crate::provider::types::ProviderError) -> Self {
        AgentHostError::Provider(e.to_string())
    }
}

/// Host side of the native chat: drives the in-sandbox supervisor (`pi-host.sh`) and holds at most
/// one live attach stream (`pi-attach.sh`) per sandbox. Real impl `ExecAgentHost` streams through
/// the engine's exec; `MockAgentHost` replays recorded traces under `MIO_MOCK_BACKEND=1`.
#[async_trait::async_trait]
pub trait AgentHost: Send + Sync {
    /// Installs the scripts if missing or outdated, starts the supervisor if it is not running,
    /// and reports its status. Idempotent.
    async fn ensure(&self, name: &str, machine: &MachineRef) -> Result<HostStatus, AgentHostError>;
    /// Opens an attach stream from `cursor` (or the journal's end when `None`), replacing any
    /// earlier attach for `name`. Every journal record and the final `mio_detached` record go to
    /// `on_event`.
    async fn attach(
        &self,
        name: &str,
        machine: &MachineRef,
        cursor: Option<Cursor>,
        on_event: Channel<AttachEvent>,
    ) -> Result<AttachInfo, AgentHostError>;
    /// Writes one JSON line to pi through the live attach. Refused when that attach is a reader.
    async fn send(&self, name: &str, line: String) -> Result<(), AgentHostError>;
    /// Ends the live attach for `name`, if any. pi keeps running.
    async fn detach(&self, name: &str);
    /// Ends every live attach; called on app exit.
    async fn detach_all(&self);
    /// Ends whichever client holds the writer lock, so the next attach becomes the writer.
    async fn takeover(&self, name: &str, machine: &MachineRef) -> Result<(), AgentHostError>;
    /// Stops pi in order (killing it after a timeout) and ends every attach stream to it.
    async fn stop(&self, name: &str, machine: &MachineRef) -> Result<(), AgentHostError>;
}

//! The native agent chat: attach to, write to and control the pi supervisor inside a sandbox.

use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

use crate::agent_host::sessions::{list_sessions, ChatSession};
use crate::agent_host::types::{AgentHost, AgentHostError, AttachEvent, AttachInfo, Cursor, HostStatus};
use crate::error::AppError;
use crate::provider::types::MachineRef;
use crate::services::routing::SandboxRouter;

/// What `agent_chat_attach` returns: the stream's header and the supervisor's status.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachResult {
    pub attach: AttachInfo,
    pub status: HostStatus,
}

/// The sandbox's local machine, refusing a deployed sandbox
/// (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 11).
async fn local_machine(name: &str, router: &SandboxRouter) -> Result<MachineRef, AppError> {
    let route = router.locate(name).await?;
    if !router.is_local(&route.machine) {
        return Err(AgentHostError::Deployed.into());
    }
    Ok(route.machine)
}

/// Ensures the supervisor runs and opens an attach stream. A cursor from an older generation is
/// dropped, so the stream starts at the journal's end and the caller rebuilds history from pi.
#[tauri::command]
pub async fn agent_chat_attach(
    name: String,
    cursor: Option<Cursor>,
    on_event: Channel<AttachEvent>,
    router: State<'_, SandboxRouter>,
    host: State<'_, Arc<dyn AgentHost>>,
) -> Result<AttachResult, AppError> {
    let machine = local_machine(&name, &router).await?;
    let status = host.ensure(&name, &machine).await?;
    let cursor = cursor.filter(|c| c.generation == status.generation);
    let attach = host.attach(&name, &machine, cursor, on_event).await?;
    Ok(AttachResult { attach, status })
}

/// Writes one JSON command line to pi.
#[tauri::command]
pub async fn agent_chat_send(
    name: String,
    line: String,
    host: State<'_, Arc<dyn AgentHost>>,
) -> Result<(), AppError> {
    Ok(host.send(&name, line).await?)
}

/// Ends this app's attach stream; pi keeps running.
#[tauri::command]
pub async fn agent_chat_detach(name: String, host: State<'_, Arc<dyn AgentHost>>) -> Result<(), AppError> {
    host.detach(&name).await;
    Ok(())
}

/// Ends the client holding the writer lock, so this app's next attach becomes the writer.
#[tauri::command]
pub async fn agent_chat_takeover(
    name: String,
    router: State<'_, SandboxRouter>,
    host: State<'_, Arc<dyn AgentHost>>,
) -> Result<(), AppError> {
    let machine = local_machine(&name, &router).await?;
    Ok(host.takeover(&name, &machine).await?)
}

/// Stops pi inside the sandbox and ends every attach stream to it.
#[tauri::command]
pub async fn agent_chat_stop(
    name: String,
    router: State<'_, SandboxRouter>,
    host: State<'_, Arc<dyn AgentHost>>,
) -> Result<(), AppError> {
    let machine = local_machine(&name, &router).await?;
    Ok(host.stop(&name, &machine).await?)
}

/// The native chat's sessions, read from the host workspace so it works while the sandbox is stopped.
#[tauri::command]
pub fn agent_chat_sessions(workspace_path: String) -> Result<Vec<ChatSession>, AppError> {
    Ok(list_sessions(std::path::Path::new(&workspace_path)))
}

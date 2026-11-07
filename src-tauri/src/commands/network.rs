//! Forward listing, and exposing or revoking a sandbox port on the host.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::network::types::{ForwardInfo, NetworkManager};
use crate::services::routing::SandboxRouter;

/// Snapshot of every open Mutagen forward (terminal, agent-web, user-exposed sandbox ports).
#[tauri::command]
pub async fn list_open_forwards(
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<Vec<ForwardInfo>, AppError> {
    Ok(network.list_forwards().await)
}

/// Expose a target port to Windows-host loopback via Mutagen. Returns the host port.
#[tauri::command]
pub async fn expose_port(
    name: String,
    workspace_path: String,
    target_port: u16,
    router: State<'_, SandboxRouter>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<u16, AppError> {
    let route = router.resolve(&name, &app).await?;
    Ok(network
        .expose_port(&name, &route.machine, &workspace_path, target_port, None, false)
        .await?)
}

/// Stop exposing a target port. Best-effort.
#[tauri::command]
pub async fn unexpose_port(
    name: String,
    target_port: u16,
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<(), AppError> {
    network.unexpose_port(&name, target_port).await;
    Ok(())
}

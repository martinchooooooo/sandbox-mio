//! Deploy and undeploy of a sandbox to a remote host, plus sync status and toggles.

use std::sync::Arc;

use tauri::State;

use crate::backends::types::SandboxBackend;
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::{MachineRef, ProviderId};
use crate::runtime::types::RuntimeManager;
use crate::services;
use crate::sync::types::{SessionInfo, SyncEngine, SyncStatus};

/// Deploy a local sandbox to a registered host.
#[tauri::command]
pub async fn deploy_sandbox(
    name: String,
    host: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<(), AppError> {
    let deployment = store.deploy(&name, &host).await?;
    let target = MachineRef { provider: ProviderId::Podman, id: host };
    runtime
        .reproduce_remote(&name, &target, &deployment.alpha)
        .await?;
    engine
        .create(&name, &deployment.alpha, &deployment.beta)
        .await?;
    Ok(())
}

/// Reverse a deploy: flush remote edits and tear down remote session.
#[tauri::command]
pub async fn undeploy_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    services::sandbox::undeploy(&name, &store, &engine, &network, &app).await
}

/// Report active sync sessions as deployment state.
#[tauri::command]
pub async fn sandbox_deployments(
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<Vec<SessionInfo>, AppError> {
    Ok(engine.sessions().await?)
}

/// Report the collapsed sync status for a sandbox's session. Backs the frontend status dot.
#[tauri::command]
pub async fn sync_status(
    name: String,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<SyncStatus, AppError> {
    Ok(engine.status(&name).await?)
}

/// Turn syncing on for a sandbox — resumes the paused session, preserving conflict state.
#[tauri::command]
pub async fn enable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), AppError> {
    Ok(engine.resume(&name).await?)
}

/// Turn syncing off for a sandbox — pauses the session rather than tearing it down.
#[tauri::command]
pub async fn disable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), AppError> {
    Ok(engine.pause(&name).await?)
}

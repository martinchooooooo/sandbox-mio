//! Sandbox create, listing, start/stop/destroy, and the container event stream.

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::credentials;
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::ProviderId;
use crate::runtime::types::RuntimeManager;
use crate::services;
use crate::settings;
use crate::sync::types::SyncEngine;

/// Create a sandbox on `provider`, defaulting to the persisted `default_provider` setting.
#[tauri::command]
pub async fn create_sandbox(
    name: String,
    existing_path: Option<String>,
    provider: Option<ProviderId>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    app: tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    let provider = match provider {
        Some(p) => p,
        None => settings::resolve_default_provider(&app)?,
    };
    services::sandbox::create(
        &name,
        existing_path.as_deref(),
        provider,
        &store,
        &runtime,
        &engine,
    )
    .await
}

#[tauri::command]
pub async fn list_sandboxes(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<Sandbox>, AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .filter(|s| !s.name.starts_with(credentials::types::AUTH_CONTAINER_PREFIX))
        .collect())
}

#[tauri::command]
pub async fn start_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.start(&name).await?)
}

#[tauri::command]
pub async fn stop_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.stop(&name).await?)
}

/// Remove a sandbox. For deployed sandboxes, guard against discarding unsynced remote work.
#[tauri::command]
pub async fn destroy_sandbox(
    name: String,
    force: bool,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    services::sandbox::destroy(&name, force, &store, &engine, &network, &app).await
}

#[tauri::command]
pub async fn listen_events(on_event: Channel<serde_json::Value>, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.listen_events(on_event).await?)
}

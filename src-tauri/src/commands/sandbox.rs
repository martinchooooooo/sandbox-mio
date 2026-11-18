//! Sandbox create, listing, start/stop/destroy, and the container event stream.

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::provider::types::ProviderId;
use crate::provider::ProviderRegistry;
use crate::runtime::types::RuntimeManager;
use crate::services;
use crate::settings;
use crate::sync::types::SyncEngine;

/// Create a sandbox on `provider`, defaulting to the persisted `default_provider` setting. `proxy`
/// opts the sandbox into a per-sandbox traffic-inspection sidecar (`docs/features/network-proxy/02_dev.md`);
/// defaults to off. `egress` and `starter_hosts` are the independent network-policy choice
/// (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 5).
#[tauri::command]
pub async fn create_sandbox(
    name: String,
    existing_path: Option<String>,
    provider: Option<ProviderId>,
    proxy: Option<bool>,
    egress: Option<crate::net_policy::EgressMode>,
    starter_hosts: Option<Vec<String>>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<Sandbox, AppError> {
    let provider = match provider {
        Some(p) => p,
        None => settings::resolve_default_provider(&app)?,
    };
    let egress = egress.map(|mode| crate::backends::types::EgressRequest {
        mode: Some(mode),
        starter_hosts: starter_hosts.unwrap_or_default(),
    });
    services::sandbox::create(
        &name,
        existing_path.as_deref(),
        provider,
        proxy.unwrap_or(false),
        egress.as_ref(),
        &store,
        &runtime,
        &engine,
        &registry,
        &app,
    )
    .await
}

#[tauri::command]
pub async fn list_sandboxes(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<Sandbox>, AppError> {
    services::sandbox::list_user(&store).await
}

#[tauri::command]
pub async fn start_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    store.start(&name).await?;
    services::inference_reach::spawn_ensure_for_sandbox(&app, &name);
    services::panes::spawn_prewarm_agent_web(&app, &name);
    services::ssh_access::spawn_pin_host_keys(&app, &name);
    Ok(())
}

#[tauri::command]
pub async fn stop_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<(), AppError> {
    // Order is load-bearing: a live forward keeps dialling the stopped sandbox, booting a microsandbox
    // VM (`docs/features/local-llm-reachability/02_dev.md`, Resolved Decision 1).
    services::inference_reach::close_for_sandbox(&network, &name).await;
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
    registry: State<'_, Arc<ProviderRegistry>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    services::sandbox::destroy(&name, force, &store, &engine, &network, &registry, &app).await
}

#[tauri::command]
pub async fn listen_events(on_event: Channel<serde_json::Value>, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.listen_events(on_event).await?)
}

/// The latest resource snapshot, so a reloaded frontend hydrates without waiting for a tick
/// (`docs/features/sandbox-resource-monitor/02_dev.md`).
#[tauri::command]
pub fn get_resource_snapshot(
    monitor: State<'_, Arc<services::resources::ResourceMonitor>>,
) -> Result<services::resources::ResourceSnapshot, AppError> {
    Ok(monitor.snapshot())
}

/// One sandbox's minute rollups over `window`, or every sandbox's and the Podman machine's when
/// `name` is absent.
#[tauri::command]
pub fn get_resource_history(
    name: Option<String>,
    window: services::resources::HistoryWindow,
    app: tauri::AppHandle,
) -> Result<services::resources::ResourceHistory, AppError> {
    services::resources::history(&app, name.as_deref(), window)
}

mod backends;
mod sync;

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::backends::types::{Sandbox, SandboxBackend};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Smoke test for the bundled Mutagen sidecar: returns its version string.
#[tauri::command]
async fn mutagen_version(app: tauri::AppHandle) -> Result<String, String> {
    sync::version(&app).await
}

#[tauri::command]
async fn create_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<Sandbox, String> {
    store.create(&name).await
}

#[tauri::command]
async fn list_sandboxes(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<Sandbox>, String> {
    store.list().await
}

#[tauri::command]
async fn start_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), String> {
    store.start(&name).await
}

#[tauri::command]
async fn stop_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), String> {
    store.stop(&name).await
}

#[tauri::command]
async fn listen_events(on_event: Channel<serde_json::Value>, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), String> {
    store.listen_events(on_event).await
}

#[tauri::command]
async fn list_sandbox_hosts(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<String>, String> {
    store.list_hosts().await
}

#[tauri::command]
async fn add_sandbox_host(
    name: String,
    ssh_url: String,
    identity: Option<String>,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), String> {
    store.add_host(&name, &ssh_url, identity.as_deref()).await
}

#[tauri::command]
async fn prepare_sandbox_host(
    ssh_url: String,
    identity_path: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), String> {
    store.prepare_host(&ssh_url, &identity_path).await
}

#[tauri::command]
async fn provision_sandbox_host(
    name: String,
    ssh_url: String,
    identity_path: String,
    on_progress: Channel<serde_json::Value>,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), String> {
    store
        .provision_host(&name, &ssh_url, &identity_path, on_progress)
        .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    let devtools = tauri_plugin_devtools::Builder::default().init();

    let backend = backends::default_backend();

    let mut builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(devtools);
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(backend)
        .invoke_handler(tauri::generate_handler![
            greet,
            mutagen_version,
            create_sandbox,
            list_sandboxes,
            start_sandbox,
            stop_sandbox,
            listen_events,
            list_sandbox_hosts,
            add_sandbox_host,
            prepare_sandbox_host,
            provision_sandbox_host
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

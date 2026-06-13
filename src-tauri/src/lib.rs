mod backends;

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::backends::{PodmanBollardBackend, PodmanShellBackend};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    let devtools = tauri_plugin_devtools::Builder::default().init();

    let mut backend: Arc<dyn SandboxBackend> = Arc::new(PodmanBollardBackend);
    if false {
        backend = Arc::new(PodmanShellBackend);
    }

    let mut builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(devtools);
    }

    builder
        .plugin(tauri_plugin_opener::init())
        .manage(backend)
        .invoke_handler(tauri::generate_handler![greet, create_sandbox, list_sandboxes, start_sandbox, stop_sandbox, listen_events])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

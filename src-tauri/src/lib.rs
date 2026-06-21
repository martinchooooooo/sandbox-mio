mod backends;
mod ssh;
mod sync;

use std::sync::Arc;

use tauri::{ipc::Channel, Manager, State};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::sync::types::{SessionInfo, SyncEngine, SyncStatus};

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

/// Deploy a local sandbox to `host` (a registered Podman connection): the backend migrates the
/// container and returns the sync endpoints, then the engine seeds and starts the named session.
#[tauri::command]
async fn deploy_sandbox(
    name: String,
    host: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<(), String> {
    let deployment = store.deploy(&name, &host).await?;
    engine
        .create(&name, &deployment.alpha, &deployment.beta)
        .await
}

/// Reverse a deploy: flush remote edits back to the laptop, tear down the session and remote
/// container, and restart the kept-stopped local container so the laptop bind-mount resumes.
#[tauri::command]
async fn undeploy_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<(), String> {
    let session = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("sandbox {name:?} is not deployed"))?;
    // Flush before removing so any pending remote writes land locally first.
    // Remove the container before terminating the Mutagen session: terminating closes
    // Mutagen's SSH connections to the machine, which can briefly make it unresponsive
    // to a subsequent `podman rm` SSH connection. Keeping the session alive until after
    // the container is gone avoids that window.
    engine.flush(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    engine.terminate(&name).await?;
    store.start(&name).await
}

/// Remove a sandbox. For a deployed sandbox (a session named after it exists) this is a guard: it
/// settles the session, refuses to discard unsynced remote work unless `force` is set, then tears
/// down the session and remote container before removing the local one. A purely local sandbox
/// (no session) is removed directly. The workspace directories are never deleted.
#[tauri::command]
async fn destroy_sandbox(
    name: String,
    force: bool,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<(), String> {
    // Absent session ⇒ purely local sandbox ⇒ no sync guard and no remote container.
    let session = engine
        .sessions()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name);
    let Some(session) = session else {
        return store.destroy(&name).await;
    };
    // Settle pending propagation, then refuse to drop unsynced remote work unless forced. The
    // frontend surfaces this as a confirmation and retries with `force` set.
    let _ = engine.flush(&name).await;
    if !force && engine.status(&name).await? != SyncStatus::UpToDate {
        return Err(format!(
            "sandbox {name:?} has unsynced changes on {}; confirm to destroy and discard them",
            session.host
        ));
    }
    engine.terminate(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    store.destroy(&name).await
}

/// Report active sync sessions as deployment state. The frontend overlays these onto the local
/// sandbox list to mark which sandboxes are deployed, to which host, and their live sync status.
#[tauri::command]
async fn sandbox_deployments(
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<Vec<SessionInfo>, String> {
    engine.sessions().await
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

/// Report the collapsed sync status for a sandbox's session. Backs the frontend status dot.
#[tauri::command]
async fn sync_status(
    name: String,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<SyncStatus, String> {
    engine.status(&name).await
}

/// Turn syncing on for a sandbox — resumes the paused session, preserving conflict state.
#[tauri::command]
async fn enable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), String> {
    engine.resume(&name).await
}

/// Turn syncing off for a sandbox — pauses the session rather than tearing it down.
#[tauri::command]
async fn disable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), String> {
    engine.pause(&name).await
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

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(backend)
        .setup(|app| {
            // Constructed in setup because the sync engine needs the AppHandle to resolve the
            // bundled Mutagen sidecar.
            app.manage(sync::default_sync_engine(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            mutagen_version,
            create_sandbox,
            list_sandboxes,
            start_sandbox,
            stop_sandbox,
            deploy_sandbox,
            undeploy_sandbox,
            destroy_sandbox,
            sandbox_deployments,
            listen_events,
            list_sandbox_hosts,
            add_sandbox_host,
            prepare_sandbox_host,
            provision_sandbox_host,
            sync_status,
            enable_sync,
            disable_sync
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // The app-private Mutagen daemon auto-starts on the first session command; stop it on
        // exit so we never orphan it. Best-effort: a failed stop must not block shutdown.
        if let tauri::RunEvent::Exit = event {
            let engine = app_handle.state::<Arc<dyn SyncEngine>>();
            tauri::async_runtime::block_on(engine.shutdown());
        }
    });
}

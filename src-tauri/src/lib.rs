mod backends;
mod error;
mod runtime;
mod ssh;
mod sync;

use std::sync::{Arc, Mutex};

use tauri::{ipc::Channel, Manager, State};

use crate::backends::types::{Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::runtime::types::{RuntimeManager, TerminalSession};
use crate::sync::types::{SessionInfo, SyncEngine, SyncStatus};

/// Maps sandbox name → the Windows-host loopback port of its active Mutagen forwarding session.
/// Held in Tauri state so undeploy/destroy can terminate the session and reclaim the port.
type ForwardPortMap = Arc<Mutex<std::collections::HashMap<String, u16>>>;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn base_image_version() -> u32 {
    backends::BASE_IMAGE_VERSION
}

#[tauri::command]
fn get_dismissed_image_version(workspace_path: String) -> Option<u32> {
    let path = std::path::Path::new(&workspace_path).join(".mio").join("dismissed_image_version");
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[tauri::command]
fn dismiss_image_update(workspace_path: String, version: u32) -> Result<(), AppError> {
    let mio_dir = std::path::Path::new(&workspace_path).join(".mio");
    std::fs::create_dir_all(&mio_dir)
        .and_then(|_| std::fs::write(mio_dir.join("dismissed_image_version"), version.to_string()))
        .map_err(|e| crate::backends::types::BackendError::CommandFailed(e.to_string()).into())
}

/// Smoke test for the bundled Mutagen sidecar: returns its version string.
#[tauri::command]
async fn mutagen_version(app: tauri::AppHandle) -> Result<String, AppError> {
    Ok(sync::version(&app).await?)
}

#[tauri::command]
async fn create_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Sandbox, AppError> {
    // Build the mise-baked base image on first use; a no-op once staged. Done here, not in
    // `create`, so the one-time build is an explicit step of the create command.
    store.ensure_base_image().await?;
    let sandbox = store.create(&name).await?;
    runtime.init(&sandbox.workspace_path).await?;
    Ok(sandbox)
}

#[tauri::command]
async fn install_package(
    name: String,
    package: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    Ok(runtime.install(&name, &package).await?)
}

#[tauri::command]
async fn uninstall_package(
    name: String,
    package: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    Ok(runtime.uninstall(&name, &package).await?)
}

/// Open the in-app web terminal for sandbox `name`. Works for both local and deployed sandboxes:
/// starts the zellij web server over the appropriate Podman connection, mints a one-time token,
/// creates a Mutagen forwarding session from a Windows-host loopback port to the container's
/// published zellij port, and returns the Windows-side URL plus token for the frontend.
#[tauri::command]
async fn open_terminal(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    forward_ports: State<'_, ForwardPortMap>,
    app: tauri::AppHandle,
) -> Result<TerminalSession, AppError> {
    // A sync session named after the sandbox means it is deployed to a remote host; use that
    // host's connection alias. Otherwise fall back to the local machine's default connection.
    let connection = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.host)
        .unwrap_or_else(|| runtime.local_connection().to_string());

    // Refresh the SSH alias for this connection so Mutagen can resolve it. Read fresh every
    // time: the local machine's SSH port changes on each boot.
    crate::backends::ensure_connection_alias(&connection).await?;

    let (token, container_port) = runtime.prepare_terminal(&name, &connection, &workspace_path).await?;

    // Allocate an ephemeral Windows-side port by binding and immediately releasing it before
    // Mutagen binds it. The race window is negligible on loopback.
    let windows_port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
        listener
            .local_addr()
            .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?
            .port()
    };

    let forward_name = format!("terminal-{name}");
    let url = format!("http://127.0.0.1:{windows_port}/");
    sync::forward_create(&app, &forward_name, windows_port, &connection, container_port)
        .await
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    forward_ports.lock().unwrap().insert(name.clone(), windows_port);

    // Persist token + URL host-side (docs/techdebt.md workaround). Writing host-side avoids
    // shell-escaping JSON through `podman exec`; the workspace is bind-mounted into the container
    // at /root/work/.mio/. Written after forward_create so the URL is the live Mutagen port.
    let mio_dir = std::path::Path::new(&workspace_path).join(".mio");
    std::fs::create_dir_all(&mio_dir)
        .and_then(|_| {
            let json = serde_json::json!({ "token": &token, "url": &url });
            std::fs::write(mio_dir.join("zellij.json"), json.to_string())
        })
        .map_err(|e| {
            crate::runtime::types::RuntimeError::TerminalUnavailable(format!(
                "failed to write session to {}: {e}",
                mio_dir.join("zellij.json").to_string_lossy()
            ))
        })?;

    Ok(TerminalSession { url, token })
}

#[tauri::command]
async fn list_packages(
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Vec<String>, AppError> {
    Ok(runtime.list(&workspace_path).await?)
}

#[tauri::command]
async fn list_sandboxes(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<Sandbox>, AppError> {
    Ok(store.list().await?)
}

#[tauri::command]
async fn start_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.start(&name).await?)
}

#[tauri::command]
async fn stop_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.stop(&name).await?)
}

/// Deploy a local sandbox to `host` (a registered Podman connection): the backend migrates the
/// container and returns the sync endpoints, then the engine seeds and starts the named session.
#[tauri::command]
async fn deploy_sandbox(
    name: String,
    host: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<(), AppError> {
    let deployment = store.deploy(&name, &host).await?;
    runtime
        .reproduce_remote(&name, &host, &deployment.alpha)
        .await?;
    engine
        .create(&name, &deployment.alpha, &deployment.beta)
        .await?;
    Ok(())
}

/// Reverse a deploy: flush remote edits back to the laptop, tear down the session and remote
/// container, and restart the kept-stopped local container so the laptop bind-mount resumes.
#[tauri::command]
async fn undeploy_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    forward_ports: State<'_, ForwardPortMap>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let session = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| AppError::NotDeployed(name.clone()))?;
    // Tear down any active forwarding session before the sync session closes its SSH channel.
    sync::forward_terminate(&app, &format!("terminal-{name}")).await;
    forward_ports.lock().unwrap().remove(&name);
    // Flush before removing so any pending remote writes land locally first.
    // Remove the container before terminating the Mutagen session: terminating closes
    // Mutagen's SSH connections to the machine, which can briefly make it unresponsive
    // to a subsequent `podman rm` SSH connection. Keeping the session alive until after
    // the container is gone avoids that window.
    engine.flush(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    engine.terminate(&name).await?;
    Ok(store.start(&name).await?)
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
    forward_ports: State<'_, ForwardPortMap>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    // Absent session ⇒ purely local sandbox ⇒ no sync guard and no remote container.
    let session = engine
        .sessions()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name);
    let Some(session) = session else {
        // Still clean up any forwarding session from a local terminal.
        sync::forward_terminate(&app, &format!("terminal-{name}")).await;
        forward_ports.lock().unwrap().remove(&name);
        return Ok(store.destroy(&name).await?);
    };
    // Settle pending propagation, then refuse to drop unsynced remote work unless forced. The
    // frontend surfaces this as a confirmation and retries with `force` set.
    let _ = engine.flush(&name).await;
    if !force && engine.status(&name).await? != SyncStatus::UpToDate {
        return Err(AppError::UnsyncedChanges {
            name: name.clone(),
            host: session.host.clone(),
        });
    }
    sync::forward_terminate(&app, &format!("terminal-{name}")).await;
    forward_ports.lock().unwrap().remove(&name);
    engine.terminate(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    Ok(store.destroy(&name).await?)
}

/// Report active sync sessions as deployment state. The frontend overlays these onto the local
/// sandbox list to mark which sandboxes are deployed, to which host, and their live sync status.
#[tauri::command]
async fn sandbox_deployments(
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<Vec<SessionInfo>, AppError> {
    Ok(engine.sessions().await?)
}

#[tauri::command]
async fn listen_events(on_event: Channel<serde_json::Value>, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.listen_events(on_event).await?)
}

#[tauri::command]
async fn list_sandbox_hosts(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<String>, AppError> {
    Ok(store.list_hosts().await?)
}

#[tauri::command]
async fn add_sandbox_host(
    name: String,
    ssh_url: String,
    identity: Option<String>,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), AppError> {
    Ok(store.add_host(&name, &ssh_url, identity.as_deref()).await?)
}

#[tauri::command]
async fn prepare_sandbox_host(
    ssh_url: String,
    identity_path: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), AppError> {
    Ok(store.prepare_host(&ssh_url, &identity_path).await?)
}

#[tauri::command]
async fn provision_sandbox_host(
    name: String,
    ssh_url: String,
    identity_path: String,
    on_progress: Channel<serde_json::Value>,
    store: State<'_, Arc<dyn SandboxBackend>>,
) -> Result<(), AppError> {
    Ok(store
        .provision_host(&name, &ssh_url, &identity_path, on_progress)
        .await?)
}

/// Report the collapsed sync status for a sandbox's session. Backs the frontend status dot.
#[tauri::command]
async fn sync_status(
    name: String,
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<SyncStatus, AppError> {
    Ok(engine.status(&name).await?)
}

/// Turn syncing on for a sandbox — resumes the paused session, preserving conflict state.
#[tauri::command]
async fn enable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), AppError> {
    Ok(engine.resume(&name).await?)
}

/// Turn syncing off for a sandbox — pauses the session rather than tearing it down.
#[tauri::command]
async fn disable_sync(name: String, engine: State<'_, Arc<dyn SyncEngine>>) -> Result<(), AppError> {
    Ok(engine.pause(&name).await?)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(debug_assertions)]
    let devtools = tauri_plugin_devtools::Builder::default().init();

    let backend = backends::default_backend();
    let runtime = runtime::default_runtime();

    let mut builder = tauri::Builder::default();

    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(devtools);
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .manage(backend)
        .manage(runtime)
        .setup(|app| {
            // Constructed in setup because the sync engine needs the AppHandle to resolve the
            // bundled Mutagen sidecar.
            app.manage(sync::default_sync_engine(app.handle().clone()));
            app.manage(ForwardPortMap::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            base_image_version,
            get_dismissed_image_version,
            dismiss_image_update,
            mutagen_version,
            create_sandbox,
            install_package,
            uninstall_package,
            open_terminal,
            list_packages,
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

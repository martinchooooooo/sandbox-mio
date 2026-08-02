mod activity;
mod agents;
mod backends;
mod credentials;
mod env_ext;
mod error;
mod inference;
mod machine;
mod mockctl;
mod network;
mod paths;
mod process_ext;
mod runtime;
mod settings;
mod ssh;
mod sync;
mod workspace;

use std::sync::Arc;

use tauri::{ipc::Channel, Manager, State};
use tracing_subscriber::prelude::*;

use crate::activity::types::{Task, TaskKind};
use crate::activity::TaskRegistry;
use crate::agents::types::AuthSessionMap;
use crate::backends::types::{Sandbox, SandboxBackend};
use crate::machine::types::MachineManager;
use crate::error::AppError;
use crate::inference::types::{
    ActiveServer, EngineKind, InferenceStatus, MachineSpecs, MachineSpecsCache, SetupStep,
};
use crate::network::types::{ForwardInfo, NetworkManager, ServiceKind};
use crate::runtime::types::RuntimeManager;
use crate::settings::types::AppSettings;
use crate::sync::types::{SessionInfo, SyncEngine, SyncStatus};
use crate::workspace::types::{AppKind, TerminalPane, WorkspaceRegistry, WorkspaceState};

const INFERENCE_FORWARD: &str = "inference-reverse";

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
    existing_path: Option<String>,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Sandbox, AppError> {
    store.ensure_base_image().await?;
    let sandbox = store.create(&name, existing_path.as_deref()).await?;
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

async fn prepare_terminal_webview(
    name: &str,
    workspace_path: &str,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
) -> Result<TerminalPane, AppError> {
    let connection = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.host)
        .unwrap_or_else(|| runtime.local_connection().to_string());

    crate::backends::ensure_connection_alias(&connection).await?;

    let (token, container_port) = runtime.prepare_terminal(name, &connection, workspace_path).await?;

    let forward_name = format!("terminal-{name}");
    let windows_port = network
        .open_forward(name, ServiceKind::Terminal, &connection, container_port)
        .await
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    let mut url = tauri::Url::parse(&format!("http://127.0.0.1:{windows_port}/"))
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| {
            crate::runtime::types::RuntimeError::TerminalUnavailable("cannot-be-a-base URL".into())
        })?
        .push(name);
    let url = url.to_string();

    let mio_dir = std::path::Path::new(workspace_path).join(".mio");
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

    let token_json = serde_json::to_string(&token)
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    let init_script = format!(
        r#"(async () => {{
    if (sessionStorage.getItem('__mio_terminal_logged_in')) return;
    try {{
        const r = await fetch('/command/login', {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ auth_token: {token_json}, remember_me: true }})
        }});
        if (r.ok) {{
            sessionStorage.setItem('__mio_terminal_logged_in', '1');
            location.replace(location.pathname + location.search);
        }}
    }} catch (_) {{}}
}})();"#
    );
    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    Ok(TerminalPane { url: parsed_url, init_script, forward_name })
}

fn read_paseo_password(workspace_path: &str) -> Option<String> {
    let path = std::path::Path::new(workspace_path).join(".mio").join("paseo.json");
    let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    value
        .get("password")
        .and_then(|p| p.as_str())
        .map(String::from)
        .filter(|p| !p.is_empty())
}

async fn fetch_paseo_server_info(
    port: u16,
    password: &str,
) -> Result<(String, Option<String>), String> {
    let url = format!("http://127.0.0.1:{port}/api/status");
    let client = reqwest::Client::new();
    let mut last_err = String::from("no attempt made");
    for _ in 0..10 {
        let attempt = client
            .get(&url)
            .bearer_auth(password)
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await;
        match attempt {
            Ok(resp) => match resp.json::<serde_json::Value>().await {
                Ok(v) => {
                    if let Some(server_id) = v.get("serverId").and_then(|s| s.as_str()) {
                        let hostname =
                            v.get("hostname").and_then(|h| h.as_str()).map(String::from);
                        return Ok((server_id.to_string(), hostname));
                    }
                    last_err = format!("response has no serverId: {v}");
                }
                Err(e) => last_err = e.to_string(),
            },
            Err(e) => last_err = e.to_string(),
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    Err(format!("{url}: {last_err}"))
}

async fn prepare_agent_web_pane(
    name: &str,
    workspace_path: &str,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
    network: &Arc<dyn NetworkManager>,
) -> Result<TerminalPane, AppError> {
    let unavailable =
        |e: String| crate::runtime::types::RuntimeError::AgentWebUnavailable(e);

    let connection = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.host)
        .unwrap_or_else(|| runtime.local_connection().to_string());
    crate::backends::ensure_connection_alias(&connection).await?;

    let password = read_paseo_password(workspace_path)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

    let container_port = runtime.prepare_agent_web(name, &connection, &password).await?;

    let forward_name = format!("agentweb-{name}");
    let windows_port = network
        .open_forward(name, ServiceKind::AgentWeb, &connection, container_port)
        .await
        .map_err(|e| unavailable(e.to_string()))?;
    let url = format!("http://127.0.0.1:{windows_port}/");

    let mio_dir = std::path::Path::new(workspace_path).join(".mio");
    std::fs::create_dir_all(&mio_dir)
        .and_then(|_| {
            let json = serde_json::json!({ "password": &password });
            std::fs::write(mio_dir.join("paseo.json"), json.to_string())
        })
        .map_err(|e| {
            unavailable(format!(
                "failed to write session to {}: {e}",
                mio_dir.join("paseo.json").to_string_lossy()
            ))
        })?;

    let (server_id, hostname) = fetch_paseo_server_info(windows_port, &password)
        .await
        .map_err(|e| unavailable(format!("paseo /api/status through forward: {e}")))?;
    let endpoint = format!("localhost:{windows_port}");
    let connection_id = format!("direct:{endpoint}");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string();
    let registry_entry = serde_json::json!([{
        "serverId": &server_id,
        "label": hostname.as_deref().unwrap_or(name),
        "lifecycle": {},
        "connections": [{
            "id": &connection_id,
            "type": "directTcp",
            "endpoint": &endpoint,
            "useTls": false,
            "password": &password,
        }],
        "preferredConnectionId": &connection_id,
        "createdAt": &now,
        "updatedAt": &now,
    }]);
    let registry_json = serde_json::to_string(&registry_entry.to_string())
        .map_err(|e| unavailable(e.to_string()))?;
    let init_script = format!("window.localStorage.setItem('@paseo:daemon-registry', {registry_json});");

    let parsed_url = url.parse::<tauri::Url>().map_err(|e| unavailable(e.to_string()))?;
    Ok(TerminalPane { url: parsed_url, init_script, forward_name })
}

/// Open sandbox's embedded Agents pane (lazily on first call, unlike Terminal/Dashboard).
#[tauri::command]
async fn open_agent_web(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    workspaces: State<'_, WorkspaceRegistry>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let exists = workspaces
        .lock()
        .unwrap()
        .apps
        .iter()
        .any(|p| p.sandbox == name && p.kind == AppKind::AgentWeb);
    if exists {
        return workspace::switch_pane(&app, &name, AppKind::AgentWeb).map_err(Into::into);
    }

    let pane = prepare_agent_web_pane(&name, &workspace_path, &runtime, &engine, &network).await?;
    workspace::embed_agent_web(&app, &name, &workspace_path, pane)?;
    Ok(())
}

/// Open standalone web-terminal window (code-level fallback to embedded workspace).
#[tauri::command]
async fn open_terminal(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let label = format!("terminal-{name}");

    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| {
            crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string())
        })?;
        return Ok(());
    }

    let pane =
        prepare_terminal_webview(&name, &workspace_path, &runtime, &engine, &network).await?;
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(pane.url))
        .title(format!("{name} — terminal"))
        .initialization_script(&pane.init_script)
        .build()
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;

    Ok(())
}

/// Start watching ports file. Idempotent; safe to call on every view.
#[tauri::command]
async fn open_workspace(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<(), AppError> {
    let connection = resolve_connection(&name, &runtime, &engine).await?;
    network
        .inner()
        .clone()
        .watch_ports_file(&name, &workspace_path, &connection)
        .await;
    Ok(())
}

/// Open sandbox's embedded Terminal pane (lazily on first call).
#[tauri::command]
async fn open_terminal_pane(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    workspaces: State<'_, WorkspaceRegistry>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let exists = workspaces
        .lock()
        .unwrap()
        .apps
        .iter()
        .any(|p| p.sandbox == name && p.kind == AppKind::Terminal);
    if exists {
        return workspace::switch_pane(&app, &name, AppKind::Terminal).map_err(Into::into);
    }

    let pane =
        prepare_terminal_webview(&name, &workspace_path, &runtime, &engine, &network).await?;
    workspace::embed_terminal(&app, &name, &workspace_path, pane)?;
    Ok(())
}

/// Bring sandbox's last-active Terminal/AgentWeb pane to the front. No-op if none is open.
#[tauri::command]
fn switch_app(name: String, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::switch_app(&app, &name)?)
}

/// Bring a specific pane to the front.
#[tauri::command]
fn switch_pane(name: String, kind: AppKind, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::switch_pane(&app, &name, kind)?)
}

/// Hide current Terminal/AgentWeb pane.
#[tauri::command]
fn hide_active_pane(app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::hide_active_pane(&app)?)
}

/// Collapse or expand the main window's global panel.
#[tauri::command]
fn set_panel_collapsed(collapsed: bool, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::set_collapsed(&app, collapsed)?)
}

/// Close every embedded pane of sandbox and reclaim forwards. No-op if none is open.
#[tauri::command]
async fn close_workspace(name: String, app: tauri::AppHandle) -> Result<(), AppError> {
    workspace::close_app(&app, &name).await;
    Ok(())
}

/// Close exactly one app tab of sandbox. No-op if none is open.
#[tauri::command]
async fn close_pane(name: String, kind: AppKind, app: tauri::AppHandle) -> Result<(), AppError> {
    workspace::close_pane(&app, &name, kind).await;
    Ok(())
}

/// Read main workspace state. Backs chrome and e2e introspection; infallible.
#[tauri::command]
fn workspace_state(workspaces: State<'_, WorkspaceRegistry>) -> WorkspaceState {
    workspaces.lock().unwrap().clone()
}

#[tauri::command]
async fn list_packages(
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Vec<String>, AppError> {
    Ok(runtime.list(&workspace_path).await?)
}

#[tauri::command]
async fn list_registry(
    name: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<Vec<runtime::types::RegistryEntry>, AppError> {
    Ok(runtime.registry(&name).await?)
}

/// List sandboxes, excluding ephemeral auth containers.
#[tauri::command]
async fn list_sandboxes(store: State<'_, Arc<dyn SandboxBackend>>) -> Result<Vec<Sandbox>, AppError> {
    Ok(store
        .list()
        .await?
        .into_iter()
        .filter(|s| !s.name.starts_with(credentials::types::AUTH_CONTAINER_PREFIX))
        .collect())
}

#[tauri::command]
async fn start_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.start(&name).await?)
}

#[tauri::command]
async fn stop_sandbox(name: String, store: State<'_, Arc<dyn SandboxBackend>>) -> Result<(), AppError> {
    Ok(store.stop(&name).await?)
}

/// Deploy a local sandbox to a registered host.
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

/// Reverse a deploy: flush remote edits and tear down remote session.
#[tauri::command]
async fn undeploy_sandbox(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let session = engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .ok_or_else(|| AppError::NotDeployed(name.clone()))?;
    // Close pane and forwards before closing SSH channel to avoid orphans.
    workspace::close_app(&app, &name).await;
    network.close_sandbox(&name).await;
    // Order is load-bearing: flush pending remote writes first, and remove the container before
    // terminating the session — terminating drops Mutagen's SSH connections, which can briefly
    // make the machine unresponsive to a following `podman rm`.
    engine.flush(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    engine.terminate(&name).await?;
    Ok(store.start(&name).await?)
}

/// Remove a sandbox. For deployed sandboxes, guard against discarding unsynced remote work.
#[tauri::command]
async fn destroy_sandbox(
    name: String,
    force: bool,
    store: State<'_, Arc<dyn SandboxBackend>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let session = engine
        .sessions()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|s| s.name == name);
    let Some(session) = session else {
        workspace::close_app(&app, &name).await;
        network.close_sandbox(&name).await;
        return Ok(store.destroy(&name).await?);
    };
    let _ = engine.flush(&name).await;
    if !force && engine.status(&name).await? != SyncStatus::UpToDate {
        return Err(AppError::UnsyncedChanges {
            name: name.clone(),
            host: session.host.clone(),
        });
    }
    workspace::close_app(&app, &name).await;
    network.close_sandbox(&name).await;
    engine.terminate(&name).await?;
    store.remove_remote(&name, &session.host).await?;
    Ok(store.destroy(&name).await?)
}

/// Report active sync sessions as deployment state.
#[tauri::command]
async fn sandbox_deployments(
    engine: State<'_, Arc<dyn SyncEngine>>,
) -> Result<Vec<SessionInfo>, AppError> {
    Ok(engine.sessions().await?)
}

/// Snapshot of every open Mutagen forward (terminal, agent-web, user-exposed sandbox ports).
#[tauri::command]
async fn list_open_forwards(
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<Vec<ForwardInfo>, AppError> {
    Ok(network.list_forwards().await)
}

async fn resolve_connection(
    name: &str,
    runtime: &Arc<dyn RuntimeManager>,
    engine: &Arc<dyn SyncEngine>,
) -> Result<String, AppError> {
    Ok(engine
        .sessions()
        .await?
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.host)
        .unwrap_or_else(|| runtime.local_connection().to_string()))
}

/// Expose a target port to Windows-host loopback via Mutagen. Returns the host port.
#[tauri::command]
async fn expose_port(
    name: String,
    workspace_path: String,
    target_port: u16,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<u16, AppError> {
    let connection = resolve_connection(&name, &runtime, &engine).await?;
    Ok(network
        .expose_port(&name, &connection, &workspace_path, target_port, None, false)
        .await?)
}

/// Stop exposing a target port. Best-effort.
#[tauri::command]
async fn unexpose_port(
    name: String,
    target_port: u16,
    network: State<'_, Arc<dyn NetworkManager>>,
) -> Result<(), AppError> {
    network.unexpose_port(&name, target_port).await;
    Ok(())
}

/// Probe whether default Podman machine is running and responding.
#[tauri::command]
async fn check_podman_ready(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    if !machine.machine_is_running().await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))?
    {
        return Err(AppError::PodmanUnavailable("Podman machine is not running".into()));
    }
    machine.poll_ready(1).await
        .map_err(|_| AppError::PodmanUnavailable("Podman socket is not ready".into()))
}

/// Check if default Podman machine exists (not just running).
#[tauri::command]
async fn machine_needs_init(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<bool, AppError> {
    Ok(!machine.machine_exists().await.map_err(|e| AppError::PodmanUnavailable(e.to_string()))?)
}

/// Full startup sequence for default Podman machine. Every error path calls `activity::fail`.
#[tauri::command]
async fn start_podman_machine(
    app: tauri::AppHandle,
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    activity::open(&app, "podman-startup", TaskKind::PodmanStartup, None);

    activity::progress(&app, "podman-startup", None, Some("Starting machine…".into()));
    if machine.run_machine_start().await.is_err() {
        if machine.check_other_machines_running().await {
            activity::progress(&app, "podman-startup", None, Some("Retrying…".into()));
        } else {
            activity::progress(&app, "podman-startup", None, Some("Retrying after WSL reset…".into()));
            machine.run_wsl_shutdown().await.ok();
        }
        if machine.run_machine_start().await.is_err() {
            let err = "Failed to start Podman machine".to_string();
            activity::fail(&app, "podman-startup", err.clone());
            return Err(AppError::PodmanUnavailable(err));
        }
    }

    activity::progress(&app, "podman-startup", None, Some("Activating Podman socket…".into()));
    machine.activate_podman_socket().await.ok();

    activity::progress(&app, "podman-startup", None, Some("Waiting for socket…".into()));
    if let Err(e) = machine.poll_ready(4).await {
        let err = format!("Podman socket did not become ready: {e}");
        activity::fail(&app, "podman-startup", err.clone());
        return Err(AppError::PodmanUnavailable(err));
    }

    activity::succeed(&app, "podman-startup", None);
    Ok(())
}

/// First-run setup for Podman (ensure WSL2, init machine, start). Returns [`AppError::PodmanRebootRequired`] if restart needed.
#[tauri::command]
async fn provision_podman(
    app: tauri::AppHandle,
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    activity::open(&app, "podman-startup", TaskKind::PodmanStartup, None);

    activity::progress(&app, "podman-startup", None, Some("Checking WSL2…".into()));
    if !machine.wsl_is_ready().await {
        activity::progress(&app, "podman-startup", None, Some("Installing WSL2…".into()));
        match machine.install_wsl().await {
            Ok(true) => {
                let msg = "WSL2 was installed and needs a restart to finish. Restart Windows, \
                           then try again."
                    .to_string();
                activity::fail(&app, "podman-startup", msg.clone());
                return Err(AppError::PodmanRebootRequired(msg));
            }
            Ok(false) => {}
            Err(e) => {
                let err = format!("Failed to install WSL2: {e}");
                activity::fail(&app, "podman-startup", err.clone());
                return Err(AppError::PodmanUnavailable(err));
            }
        }
    }

    activity::progress(&app, "podman-startup", None, Some("Checking Podman machine…".into()));
    match machine.machine_exists().await {
        Ok(true) => {}
        Ok(false) => {
            activity::progress(&app, "podman-startup", None, Some("Creating Podman machine…".into()));
            if let Err(e) = machine.init_machine().await {
                let err = format!("Failed to create Podman machine: {e}");
                activity::fail(&app, "podman-startup", err.clone());
                return Err(AppError::PodmanUnavailable(err));
            }
        }
        Err(e) => {
            let err = format!("Failed to inspect Podman machines: {e}");
            activity::fail(&app, "podman-startup", err.clone());
            return Err(AppError::PodmanUnavailable(err));
        }
    }

    start_podman_machine(app, machine).await
}

/// Advanced-settings debug utility: force-clear a default Podman machine stuck mid-import (the
/// WSL import failure surfaces as `wsl --import ... failed: exit status 0xffffffff`), so the user
/// can retry `provision_podman` without manually running `wsl --unregister` themselves.
#[tauri::command]
async fn reset_stuck_podman_machine(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    machine
        .reset_stuck_machine()
        .await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))
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

/// Install agent packages and run postinstall steps. Auto-injects credentials if available.
#[tauri::command]
async fn install_agent(
    name: String,
    packages: Vec<String>,
    postinstall_npm: Option<(String, String)>,
    credential_service: Option<String>,
    onboarding: Option<credentials::types::OnboardingSeed>,
    on_progress: Channel<serde_json::Value>,
    app: tauri::AppHandle,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    agents::install_packages(&name, &packages, &on_progress, &runtime).await?;
    if let Some((npm_package, node_tool)) = postinstall_npm {
        agents::run_postinstall_npm(&name, &npm_package, &node_tool, &on_progress, &runtime).await?;
    }
    if let Some(service_id) = credential_service {
        if credentials::load_manifest(&app, &service_id).is_ok() {
            credentials::inject(&app, &name, &service_id, &runtime).await?;
            if let Some(seed) = &onboarding {
                credentials::seed_onboarding(&name, seed, &runtime).await?;
            }
        }
    }
    on_progress.send(serde_json::to_value(&agents::types::AgentInstallEvent::Done).unwrap()).ok();
    Ok(())
}

/// Write/merge OpenCode provider config host-side (preserves existing user config).
#[tauri::command]
async fn provision_opencode(
    workspace_path: String,
    relative_path: String,
    provider_id: String,
    provider_json: serde_json::Value,
    default_model: Option<String>,
) -> Result<(), AppError> {
    let path = std::path::Path::new(&workspace_path).join(&relative_path);

    let mut obj = match std::fs::read_to_string(&path) {
        Ok(s) if s.trim().is_empty() => serde_json::Map::new(),
        Ok(s) => serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s)
            .map_err(|e| crate::backends::types::BackendError::CommandFailed(format!(
                "failed to parse existing {}: {e}",
                path.to_string_lossy()
            )))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(e) => {
            return Err(crate::backends::types::BackendError::CommandFailed(format!(
                "failed to read {}: {e}",
                path.to_string_lossy()
            ))
            .into())
        }
    };

    obj.insert(
        "$schema".into(),
        serde_json::Value::String("https://opencode.ai/config.json".into()),
    );
    let provider = obj
        .entry("provider")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = provider.as_object_mut() {
        map.insert(provider_id, provider_json);
    }
    if let Some(model) = default_model {
        obj.entry("model").or_insert(serde_json::Value::String(model));
    }

    let pretty = serde_json::to_string_pretty(&obj)
        .map_err(|e| crate::backends::types::BackendError::CommandFailed(e.to_string()))?;
    std::fs::write(&path, pretty).map_err(|e| {
        crate::backends::types::BackendError::CommandFailed(format!(
            "failed to write {}: {e}",
            path.to_string_lossy()
        ))
    })?;
    Ok(())
}

/// Write pi's local-llm provider config and env redirect host-side (container-state independent).
#[tauri::command]
async fn provision_pi(
    workspace_path: String,
    relative_path: String,            // ".mio/pi-agent/models.json"
    provider_id: String,              // "local-llm"
    provider_json: serde_json::Value, // the provider block from buildPiLocalLlmProvider()
    config_env: String,               // "PI_CODING_AGENT_DIR"
    config_dir_container: String,     // "/root/work/.mio/pi-agent"
) -> Result<(), AppError> {
    use crate::backends::types::BackendError;

    // (1) models.json scoped merge. The config path is nested, so create its parent first.
    let models_path = std::path::Path::new(&workspace_path).join(&relative_path);
    if let Some(parent) = models_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            BackendError::CommandFailed(format!("failed to create {}: {e}", parent.to_string_lossy()))
        })?;
    }
    let mut models = match std::fs::read_to_string(&models_path) {
        Ok(s) if s.trim().is_empty() => serde_json::Map::new(),
        Ok(s) => serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&s).map_err(|e| {
            BackendError::CommandFailed(format!("failed to parse existing {}: {e}", models_path.to_string_lossy()))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(e) => {
            return Err(BackendError::CommandFailed(format!("failed to read {}: {e}", models_path.to_string_lossy())).into())
        }
    };
    let providers = models
        .entry("providers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let Some(map) = providers.as_object_mut() {
        map.insert(provider_id, provider_json);
    }
    let pretty = serde_json::to_string_pretty(&models)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    std::fs::write(&models_path, pretty).map_err(|e| {
        BackendError::CommandFailed(format!("failed to write {}: {e}", models_path.to_string_lossy()))
    })?;

    let manifest_path = std::path::Path::new(&workspace_path).join("mise.toml");
    let manifest_src = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "[tools]\n".to_string(),
        Err(e) => {
            return Err(BackendError::CommandFailed(format!("failed to read {}: {e}", manifest_path.to_string_lossy())).into())
        }
    };
    let mut manifest: toml::Table = manifest_src
        .parse()
        .map_err(|e| BackendError::CommandFailed(format!("failed to parse {}: {e}", manifest_path.to_string_lossy())))?;
    let env = manifest
        .entry("env")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let Some(table) = env.as_table_mut() {
        table.insert(config_env, toml::Value::String(config_dir_container));
    }
    let manifest_out = toml::to_string(&manifest)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    std::fs::write(&manifest_path, manifest_out).map_err(|e| {
        BackendError::CommandFailed(format!("failed to write {}: {e}", manifest_path.to_string_lossy()))
    })?;
    Ok(())
}

/// Spawn agent auth command with piped I/O. Emits `AgentAuthEvent` to `agent-auth-output-<session_id>`.
#[tauri::command]
async fn start_agent_auth(
    name: String,
    auth_command: Vec<String>,
    app: tauri::AppHandle,
    sessions: State<'_, AuthSessionMap>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<String, AppError> {
    Ok(agents::start_auth(&name, &auth_command, &app, &sessions, &runtime).await?)
}

/// Write to auth process stdin. One-shot for PKCE flows.
#[tauri::command]
async fn send_agent_auth_input(
    session_id: String,
    input: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::send_auth_input(&session_id, &input, &sessions).await?)
}

/// Cancel auth session by closing stdin.
#[tauri::command]
async fn cancel_agent_auth(
    session_id: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::cancel_auth(&session_id, &sessions).await?)
}

/// Read agent credential file and return auth status.
#[tauri::command]
async fn get_agent_auth_status(
    name: String,
    credential_path: String,
    expires_at_json_key: String,
    account_source_path: Option<String>,
    account_email_json_key: Option<String>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<agents::types::AgentAuthStatus, AppError> {
    Ok(agents::get_auth_status(
        &name,
        &credential_path,
        &expires_at_json_key,
        account_source_path.as_deref(),
        account_email_json_key.as_deref(),
        &runtime,
    )
    .await?)
}

/// Create ephemeral auth container if absent. Idempotent.
#[tauri::command]
async fn ensure_auth_container(
    name: String,
    store: State<'_, Arc<dyn SandboxBackend>>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    Ok(credentials::ensure_auth_container(&name, &store, &runtime).await?)
}

/// Capture agent credentials from container into host store.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn capture_agent_credentials(
    name: String,
    service_id: String,
    credential_path: String,
    expires_at_json_key: String,
    account_source_path: Option<String>,
    account_email_json_key: Option<String>,
    app: tauri::AppHandle,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<credentials::types::CredentialManifest, AppError> {
    Ok(credentials::capture(
        &app,
        &name,
        &service_id,
        &credential_path,
        &expires_at_json_key,
        account_source_path.as_deref(),
        account_email_json_key.as_deref(),
        &runtime,
    )
    .await?)
}

/// Inject stored credentials into container. Seeds onboarding to skip wizard if given.
#[tauri::command]
async fn inject_agent_credentials(
    name: String,
    service_id: String,
    onboarding: Option<credentials::types::OnboardingSeed>,
    app: tauri::AppHandle,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    credentials::inject(&app, &name, &service_id, &runtime).await?;
    if let Some(seed) = onboarding {
        credentials::seed_onboarding(&name, &seed, &runtime).await?;
    }
    Ok(())
}

/// Mark agent first-run complete to skip onboarding wizard in terminal.
#[tauri::command]
async fn seed_agent_onboarding(
    name: String,
    onboarding: credentials::types::OnboardingSeed,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    Ok(credentials::seed_onboarding(&name, &onboarding, &runtime).await?)
}

/// List every service with persisted credentials.
#[tauri::command]
fn list_credential_bundles(
    app: tauri::AppHandle,
) -> Result<Vec<(String, credentials::types::CredentialManifest)>, AppError> {
    Ok(credentials::list(&app)?)
}

/// Delete host credential bundle (container copies are untouched).
#[tauri::command]
fn delete_credential_bundle(service_id: String, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(credentials::delete(&app, &service_id)?)
}

fn ephemeral_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| inference::types::InferenceError::Sidecar(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| inference::types::InferenceError::Sidecar(e.to_string()))?
        .port();
    Ok(port)
}

/// Probe host CPU/RAM/GPU/NPU and Foundry availability (runs on host, uses cache after first call).
#[tauri::command]
async fn detect_machine_specs(cache: State<'_, MachineSpecsCache>) -> Result<MachineSpecs, AppError> {
    let cache = cache.inner().clone();
    let specs = tauri::async_runtime::spawn_blocking(move || inference::detect_specs(&cache))
        .await
        .map_err(|e| inference::types::InferenceError::CommandFailed(e.to_string()))?;
    Ok(specs)
}

/// Force full re-probe (clears cache).
#[tauri::command]
async fn redetect_machine_specs(cache: State<'_, MachineSpecsCache>) -> Result<MachineSpecs, AppError> {
    let cache = cache.inner().clone();
    let specs = tauri::async_runtime::spawn_blocking(move || inference::redetect_specs(&cache))
        .await
        .map_err(|e| inference::types::InferenceError::CommandFailed(e.to_string()))?;
    Ok(specs)
}

/// Snapshot activity registry (infallible).
#[tauri::command]
fn list_tasks(activity: State<'_, TaskRegistry>) -> Vec<Task> {
    activity.snapshot()
}

/// Download a GGUF model (no-op if present). Enrolled in activity center; failure recorded before propagating.
#[tauri::command]
async fn download_model(
    model_id: String,
    repo: String,
    file: String,
    sha256: String,
    on_progress: Channel<serde_json::Value>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let id = format!("model-download:{model_id}");
    activity::open(&app, &id, TaskKind::ModelDownload, Some(model_id.clone()));
    match inference::download_gguf(&app, &model_id, &repo, &file, &sha256, &on_progress).await {
        Ok(()) => {
            activity::succeed(&app, &id, None);
            Ok(())
        }
        Err(e) => {
            activity::fail(&app, &id, e.to_string());
            Err(e.into())
        }
    }
}

/// List downloaded models.
#[tauri::command]
fn downloaded_models(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    Ok(inference::downloaded(&app)?)
}

/// Return engine's ordered setup checklist (no-op).
#[tauri::command]
fn runtime_setup_steps(engine: EngineKind, app: tauri::AppHandle) -> Result<Vec<SetupStep>, AppError> {
    Ok(inference::engine_for(engine, &app).setup_steps())
}

/// Start app-wide inference server (stops prior server first). Streams setup progress.
#[tauri::command]
async fn start_inference(
    engine: EngineKind,
    model_id: String,
    on_progress: Channel<serde_json::Value>,
    active: State<'_, ActiveServer>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    app: tauri::AppHandle,
) -> Result<InferenceStatus, AppError> {
    tracing::info!(target: "inference", ?engine, %model_id, "start_inference requested");
    let mut guard = active.lock().await;
    if let Some(existing) = guard.take() {
        tracing::info!(target: "inference", model_id = %existing.model_id, "stopping prior server");
        let prev = inference::engine_for(existing.engine, &app);
        let _ = prev.stop(existing).await;
    }
    let port = ephemeral_port()?;
    let engine_impl = inference::engine_for(engine, &app);

    activity::open(&app, "inference-setup", TaskKind::InferenceSetup, Some(model_id.clone()));
    let total_steps = engine_impl.setup_steps().len();

    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let app_chan = app.clone();
    let real = on_progress.clone();
    let completed_chan = completed.clone();
    let activity_channel: Channel<serde_json::Value> = Channel::new(move |body| {
        if let Ok(val) = body.deserialize::<serde_json::Value>() {
            real.send(val.clone()).ok();
            let kind = val.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if matches!(kind, "stepDone" | "stepSkipped") {
                completed_chan.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if kind != "ready" {
                let done = completed_chan.load(std::sync::atomic::Ordering::Relaxed);
                let fraction = (total_steps > 0).then(|| done as f32 / total_steps as f32);
                let detail = val
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("id").and_then(|v| v.as_str()))
                    .map(String::from);
                activity::progress(&app_chan, "inference-setup", fraction, detail);
            }
        }
        Ok(())
    });

    let handle = match engine_impl.start(&model_id, port, &activity_channel).await {
        Ok(handle) => handle,
        Err(e) => {
            tracing::error!(target: "inference", ?engine, %model_id, error = %e, "start failed");
            activity::fail(&app, "inference-setup", e.to_string());
            return Err(e.into());
        }
    };
    tracing::info!(target: "inference", base_url = %handle.base_url, model_id = %handle.model_id, "server ready");
    activity::succeed(&app, "inference-setup", Some(handle.base_url.clone()));

    let connection = runtime.local_connection().to_string();
    match crate::backends::ensure_connection_alias(&connection).await {
        Ok(()) => {
            if let Err(e) = sync::forward_create_reverse(
                &app,
                INFERENCE_FORWARD,
                &connection,
                sync::INFERENCE_VM_PORT,
                handle.port,
            )
            .await
            {
                tracing::warn!(target: "inference", error = %e, "inference reverse forward failed");
            }
        }
        Err(e) => {
            tracing::warn!(target: "inference", error = %e, "could not resolve connection alias for inference forward");
        }
    }

    let status = handle.status();
    *guard = Some(handle);
    Ok(status)
}

#[tauri::command]
async fn stop_inference(active: State<'_, ActiveServer>, app: tauri::AppHandle) -> Result<(), AppError> {
    let handle = active.lock().await.take();
    if let Some(handle) = handle {
        let engine = inference::engine_for(handle.engine, &app);
        engine.stop(handle).await?;
    }
    // Spawned off the critical path: `forward terminate` can take seconds and the frontend
    // awaits this command; a briefly dangling forward just gets connection-refused.
    let app_for_forward = app.clone();
    tauri::async_runtime::spawn(async move {
        sync::forward_terminate(&app_for_forward, INFERENCE_FORWARD).await;
    });
    Ok(())
}

#[tauri::command]
async fn inference_status(active: State<'_, ActiveServer>) -> Result<InferenceStatus, AppError> {
    let guard = active.lock().await;
    Ok(match guard.as_ref() {
        Some(handle) => handle.status(),
        None => InferenceStatus { running: false, engine: None, model_id: None, base_url: None },
    })
}

/// Install Foundry Local via winget. Streams CLI output.
#[tauri::command]
async fn install_foundry_local(on_progress: Channel<serde_json::Value>) -> Result<(), AppError> {
    inference::install_foundry(&on_progress).await?;
    Ok(())
}

/// Read `MIO_TEST_MODEL` env var (test-only GGUF descriptor).
#[tauri::command]
fn test_model() -> Option<String> {
    std::env::var("MIO_TEST_MODEL").ok().filter(|s| !s.trim().is_empty())
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> Result<AppSettings, AppError> {
    Ok(settings::load(&app)?)
}

#[tauri::command]
fn set_settings(settings: AppSettings, app: tauri::AppHandle) -> Result<(), AppError> {
    settings::save(&app, &settings)?;
    Ok(())
}

/// Returns the `WorkerGuard` for the caller to `app.manage()`. Its `Drop` is the only code path
/// that signals the non-blocking writer's worker thread to shut down and blocks briefly for it to
/// drain — dropping it silently (or leaking it, as this used to) loses whatever was queued but not
/// yet written when the process exits.
fn init_logging(app: &tauri::AppHandle) -> tracing_appender::non_blocking::WorkerGuard {
    // MIO_APP_DATA_DIR moves the log with the rest of a run's app-data (Phase C); unset, this must
    // resolve exactly as before — app_log_dir() is Local AppData, not a subdirectory of
    // app_data_dir() (Roaming), so folding the unset case into the override would relocate every
    // user's log as a side effect (docs/features/parallel-worktree-e2e/02_dev.md, Resolved Decision 9).
    let log_dir = match env_ext::env_dir("MIO_APP_DATA_DIR") {
        Some(root) => root.join("logs"),
        None => app.path().app_log_dir().unwrap_or_else(|_| std::env::temp_dir()),
    };
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "mio.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(non_blocking))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stdout))
        .try_init();

    let panic_log_path = log_dir.join("mio-panic.log");
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&panic_log_path) {
            use std::io::Write;
            let _ = writeln!(f, "{:?}: {info}", std::time::SystemTime::now());
        }
        tracing::error!("panic: {info}");
    }));

    tracing::info!(log_dir = %log_dir.display(), "logging initialized");
    guard
}

/// Install one scripted mock behaviour (see `src-tauri/src/mockctl/mod.rs`). Test-only: hard-fails
/// outside the light e2e tier so a stray call against a real backend is loud, not a silent no-op.
#[tauri::command]
fn mock_configure(
    key: String,
    behaviour: mockctl::types::Behaviour,
    director: State<'_, Arc<mockctl::types::MockDirector>>,
) -> Result<(), AppError> {
    if !backends::mock_backend_enabled() {
        return Err(AppError::MockControl(
            "mock_configure is only available under MIO_MOCK_BACKEND=1".to_string(),
        ));
    }
    director.configure(&key, behaviour).map_err(AppError::MockControl)
}

/// Clear every installed mock override. See [`mock_configure`].
#[tauri::command]
fn mock_reset(director: State<'_, Arc<mockctl::types::MockDirector>>) -> Result<(), AppError> {
    if !backends::mock_backend_enabled() {
        return Err(AppError::MockControl(
            "mock_reset is only available under MIO_MOCK_BACKEND=1".to_string(),
        ));
    }
    director.reset();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(all(debug_assertions, feature = "devtools"))]
    let devtools = tauri_plugin_devtools::Builder::default().init();

    let mock_director = Arc::new(mockctl::types::MockDirector::new());
    let podman_runner: Arc<dyn mockctl::types::ProcessRunner> = if backends::mock_backend_enabled() {
        Arc::new(mockctl::types::ScriptedPodmanRunner::new(mock_director.clone()))
    } else {
        Arc::new(mockctl::types::SystemPodmanRunner)
    };
    backends::set_podman_runner(podman_runner.clone());
    let backend = backends::default_backend(podman_runner.clone());
    let runtime = runtime::default_runtime();
    let machine = machine::default_machine_manager(podman_runner.clone());

    let mut builder = tauri::Builder::default();

    #[cfg(all(debug_assertions, feature = "devtools"))]
    {
        builder = builder.plugin(devtools);
    }

    let app = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(backend)
        .manage(runtime)
        .manage(machine)
        .manage(mock_director.clone())
        .setup(move |app| {
            app.manage(init_logging(app.handle()));
            if !backends::mock_backend_enabled() {
                if let Ok(resource_dir) = app.path().resource_dir() {
                    backends::set_podman_resource_dir(resource_dir.join("podman"));
                }
            }
            let mutagen_runner: Arc<dyn mockctl::types::ProcessRunner> =
                if backends::mock_backend_enabled() {
                    Arc::new(mockctl::types::ScriptedMutagenRunner::new(mock_director.clone()))
                } else {
                    Arc::new(mockctl::types::SystemMutagenRunner::new(app.handle().clone()))
                };
            app.manage(sync::default_sync_engine(mutagen_runner));
            app.manage(network::default_network_manager(app.handle().clone()));
            app.manage(WorkspaceRegistry::default());
            app.manage(AuthSessionMap::default());
            app.manage(ActiveServer::default());
            app.manage(TaskRegistry::default());
            app.manage(MachineSpecsCache::default());
            workspace::create_main_window(app.handle())?;
            if !backends::mock_backend_enabled() {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let live = match handle.state::<Arc<dyn SandboxBackend>>().list().await {
                        Ok(sandboxes) => sandboxes.into_iter().map(|s| s.name).collect::<Vec<_>>(),
                        Err(_) => return,
                    };
                    handle.state::<Arc<dyn NetworkManager>>().sweep(&live).await;
                });
            }
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
            open_workspace,
            open_terminal_pane,
            open_agent_web,
            switch_app,
            switch_pane,
            hide_active_pane,
            set_panel_collapsed,
            close_workspace,
            close_pane,
            workspace_state,
            list_packages,
            list_sandboxes,
            start_sandbox,
            stop_sandbox,
            deploy_sandbox,
            undeploy_sandbox,
            destroy_sandbox,
            sandbox_deployments,
            list_open_forwards,
            expose_port,
            unexpose_port,
            check_podman_ready,
            machine_needs_init,
            start_podman_machine,
            provision_podman,
            reset_stuck_podman_machine,
            listen_events,
            list_sandbox_hosts,
            add_sandbox_host,
            prepare_sandbox_host,
            provision_sandbox_host,
            sync_status,
            enable_sync,
            disable_sync,
            list_registry,
            install_agent,
            provision_opencode,
            provision_pi,
            start_agent_auth,
            send_agent_auth_input,
            cancel_agent_auth,
            get_agent_auth_status,
            ensure_auth_container,
            capture_agent_credentials,
            inject_agent_credentials,
            seed_agent_onboarding,
            list_credential_bundles,
            delete_credential_bundle,
            detect_machine_specs,
            redetect_machine_specs,
            download_model,
            list_tasks,
            downloaded_models,
            runtime_setup_steps,
            start_inference,
            stop_inference,
            inference_status,
            install_foundry_local,
            test_model,
            get_settings,
            set_settings,
            mock_configure,
            mock_reset
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let active = app_handle.state::<ActiveServer>();
            let app_for_stop = app_handle.clone();
            tauri::async_runtime::block_on(async move {
                if let Some(handle) = active.lock().await.take() {
                    let engine = inference::engine_for(handle.engine, &app_for_stop);
                    let _ = engine.stop(handle).await;
                }
            });
            let engine = app_handle.state::<Arc<dyn SyncEngine>>();
            tauri::async_runtime::block_on(engine.shutdown());
        }
    });
}

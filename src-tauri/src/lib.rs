mod activity;
mod agents;
mod backends;
mod error;
mod inference;
mod runtime;
mod settings;
mod ssh;
mod sync;

use std::sync::{Arc, Mutex};

use tauri::{ipc::Channel, Manager, State};

use crate::activity::types::{Task, TaskKind};
use crate::activity::TaskRegistry;
use crate::agents::types::AuthSessionMap;
use crate::backends::types::{Sandbox, SandboxBackend};
use crate::error::AppError;
use crate::inference::types::{ActiveServer, EngineKind, InferenceStatus, MachineSpecs, SetupStep};
use crate::runtime::types::RuntimeManager;
use crate::settings::types::AppSettings;
use crate::sync::types::{SessionInfo, SyncEngine, SyncStatus};

/// Maps sandbox name → the Windows-host loopback port of its active Mutagen forwarding session.
/// Held in Tauri state so undeploy/destroy can terminate the session and reclaim the port.
type ForwardPortMap = Arc<Mutex<std::collections::HashMap<String, u16>>>;

/// Name of the single app-wide reverse forward bridging container netns to the host inference
/// server. Created in `start_inference`, torn down in `stop_inference` and on app exit.
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
/// published zellij port, and opens a dedicated WebviewWindow with an initialization script that
/// auto-logs in via POST /command/login (sets a 30-day cookie) so no manual paste is needed.
#[tauri::command]
async fn open_terminal(
    name: String,
    workspace_path: String,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
    engine: State<'_, Arc<dyn SyncEngine>>,
    forward_ports: State<'_, ForwardPortMap>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let label = format!("terminal-{name}");

    // Second click: focus the existing window without re-preparing the terminal.
    if let Some(window) = app.get_webview_window(&label) {
        window.set_focus().map_err(|e| {
            crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string())
        })?;
        return Ok(());
    }

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

    // WindowConfig (used by the create_webview_window IPC command) has deny_unknown_fields, so
    // initialization_script cannot be passed from the JS side. Create the window from Rust instead,
    // where WebviewWindowBuilder::initialization_script() is available as a typed builder method.
    // The script POSTs /command/login to set a 30-day session cookie, eliminating manual paste.
    // See: https://github.com/zellij-org/zellij/issues/5194#issuecomment-4516799423
    let token_json = serde_json::to_string(&token)
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    let init_script = format!(
        r#"(async () => {{
    try {{
        const r = await fetch('/command/login', {{
            method: 'POST',
            headers: {{ 'Content-Type': 'application/json' }},
            body: JSON.stringify({{ auth_token: {token_json}, remember_me: true }})
        }});
        if (r.ok && location.pathname.startsWith('/auth')) location.replace('/');
    }} catch (_) {{}}
}})();"#
    );
    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;
    tauri::WebviewWindowBuilder::new(&app, &label, tauri::WebviewUrl::External(parsed_url))
        .title(format!("{name} — terminal"))
        .initialization_script(&init_script)
        .build()
        .map_err(|e| crate::runtime::types::RuntimeError::TerminalUnavailable(e.to_string()))?;

    Ok(())
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

/// Read-only probe for the projects page startup gate. Reports whether the default Podman machine
/// is running *and* its rootless socket answers. Starts nothing — `start_podman_machine` does that.
/// Takes no state: it drives the global Podman CLI, not the `SandboxBackend` trait.
#[tauri::command]
async fn check_podman_ready() -> Result<(), AppError> {
    if !backends::machine_is_running().await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))?
    {
        return Err(AppError::PodmanUnavailable("Podman machine is not running".into()));
    }
    backends::poll_ready(1).await
        .map_err(|_| AppError::PodmanUnavailable("Podman socket is not ready".into()))
}

/// Full startup sequence for the default Podman machine, driven from the "Start Podman" button.
/// Drives the activity center task `"podman-startup"` so the global panel shows live progress.
/// Every `Err` return path first calls `activity::fail` — the registry has no view of the command's
/// `Result`, so a missing `fail` would leave the task stuck `running` until the app restarts.
#[tauri::command]
async fn start_podman_machine(app: tauri::AppHandle) -> Result<(), AppError> {
    activity::open(&app, "podman-startup", TaskKind::PodmanStartup, None);

    activity::progress(&app, "podman-startup", None, Some("Starting machine…".into()));
    if backends::run_machine_start().await.is_err() {
        // A bare retry first; only reset WSL if no other machine is running (a `wsl --shutdown`
        // would terminate mio-tests/mio-tests-remote mid-run).
        if backends::check_other_machines_running().await {
            activity::progress(&app, "podman-startup", None, Some("Retrying…".into()));
        } else {
            activity::progress(&app, "podman-startup", None, Some("Retrying after WSL reset…".into()));
            backends::run_wsl_shutdown().await.ok();
        }
        if backends::run_machine_start().await.is_err() {
            let err = "Failed to start Podman machine".to_string();
            activity::fail(&app, "podman-startup", err.clone());
            return Err(AppError::PodmanUnavailable(err));
        }
    }

    activity::progress(&app, "podman-startup", None, Some("Activating Podman socket…".into()));
    backends::activate_podman_socket().await.ok();

    activity::progress(&app, "podman-startup", None, Some("Waiting for socket…".into()));
    if let Err(e) = backends::poll_ready(4).await {
        let err = format!("Podman socket did not become ready: {e}");
        activity::fail(&app, "podman-startup", err.clone());
        return Err(AppError::PodmanUnavailable(err));
    }

    activity::succeed(&app, "podman-startup", None);
    Ok(())
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

/// Install all mise packages for an agent sequentially, then run its postinstall steps.
/// Streams `AgentInstallEvent` JSON objects to `on_progress`.
/// `postinstall_npm` is `(npm_package, node_tool)` — present only when the agent needs the
/// npm postinstall workaround for mise's `--ignore-scripts=true` default.
#[tauri::command]
async fn install_agent(
    name: String,
    packages: Vec<String>,
    postinstall_npm: Option<(String, String)>,
    on_progress: Channel<serde_json::Value>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
) -> Result<(), AppError> {
    agents::install_packages(&name, &packages, &on_progress, &runtime).await?;
    if let Some((npm_package, node_tool)) = postinstall_npm {
        agents::run_postinstall_npm(&name, &npm_package, &node_tool, &on_progress, &runtime).await?;
    }
    on_progress.send(serde_json::to_value(&agents::types::AgentInstallEvent::Done).unwrap()).ok();
    Ok(())
}

/// Write/merge the OpenCode provider config into `<workspace_path>/<relative_path>` host-side.
/// Mirrors `open_terminal`'s host-side JSON write into the bind-mounted workspace, so it needs no
/// `podman exec` and works whether the container is running or stopped. Preserves any existing
/// `opencode.json`: sets `provider[provider_id]` and ensures `$schema`, and sets the top-level
/// `model` only when absent, so a user's own config is never clobbered.
#[tauri::command]
async fn provision_opencode(
    workspace_path: String,
    relative_path: String,
    provider_id: String,
    provider_json: serde_json::Value,
    default_model: Option<String>,
) -> Result<(), AppError> {
    let path = std::path::Path::new(&workspace_path).join(&relative_path);

    // Read+parse any existing config. An absent or empty file starts from `{}`; a genuine parse
    // error of non-empty content is surfaced rather than silently overwriting the user's file.
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

/// Provision pi's `local-llm` provider host-side into the bind-mounted workspace, container-state
/// independent (mirrors `provision_opencode`). Two writes: (1) merge `<workspace>/<relative_path>`
/// (`models.json`) setting only `providers[provider_id]`, preserving other providers; (2) set
/// `mise.toml [env] <config_env> = <config_dir_container>`, preserving `[tools]` and other `[env]`
/// keys, so pi reads the redirected `models.json` from the bind-mounted workspace.
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

    // (2) mise.toml [env] redirect. `init` always writes the manifest before any agent install, so
    // it normally exists; fall back to a bare `[tools]` manifest if not.
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

/// Spawn the agent auth command inside sandbox `name` with piped stdin/stdout.
/// Returns a session ID that identifies the auth session for subsequent input/cancel calls.
/// Emits `AgentAuthEvent` objects to the Tauri app-wide event `agent-auth-output-<session_id>`.
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

/// Write a line to the auth process stdin identified by `session_id`. One-shot for PKCE flows.
#[tauri::command]
async fn send_agent_auth_input(
    session_id: String,
    input: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::send_auth_input(&session_id, &input, &sessions).await?)
}

/// Cancel a running auth session by closing its stdin pipe (process receives EOF and exits).
#[tauri::command]
async fn cancel_agent_auth(
    session_id: String,
    sessions: State<'_, AuthSessionMap>,
) -> Result<(), AppError> {
    Ok(agents::cancel_auth(&session_id, &sessions).await?)
}

/// Read the agent credential file from the container and return auth status.
/// `expires_at_json_key` and `account_email_json_key` are dot-separated JSON paths.
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

/// Allocate an ephemeral loopback port by binding and immediately releasing it. Same trick as
/// `open_terminal`; the race window before the inference server rebinds it is negligible on loopback.
fn ephemeral_port() -> Result<u16, AppError> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| inference::types::InferenceError::Sidecar(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| inference::types::InferenceError::Sidecar(e.to_string()))?
        .port();
    Ok(port)
}

/// Probe host CPU/RAM/GPU/NPU and Foundry availability. Runs on the host, never the container VM.
#[tauri::command]
async fn detect_machine_specs(app: tauri::AppHandle) -> Result<MachineSpecs, AppError> {
    // `detect_specs` shells out (PowerShell/winget on Windows); run it off the async runtime.
    let specs = tauri::async_runtime::spawn_blocking(move || inference::detect_specs(&app))
        .await
        .map_err(|e| inference::types::InferenceError::CommandFailed(e.to_string()))?;
    Ok(specs)
}

/// Snapshot the activity registry for the global activity center. Infallible: a snapshot read
/// cannot meaningfully fail (precedent: `test_model` returns `Option` without `Result`). The store
/// subscribes to `activity://task-updated` *before* calling this, then folds both sources by version.
#[tauri::command]
fn list_tasks(activity: State<'_, TaskRegistry>) -> Vec<Task> {
    activity.snapshot()
}

/// Download a GGUF for the llama engine. No-op if already present. Enrolled in the activity center:
/// opens a `model-download:<model_id>` task, succeeds/fails it (the registry has no view of the
/// command's `Result`, so the failure path must be recorded here before propagating).
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

/// Catalogue model_ids that have a verified local GGUF.
#[tauri::command]
fn downloaded_models(app: tauri::AppHandle) -> Result<Vec<String>, AppError> {
    Ok(inference::downloaded(&app)?)
}

/// Return an engine's ordered setup checklist without starting anything, so the UI can render the
/// full plan up front (also used by the `/settings` static explainer).
#[tauri::command]
fn runtime_setup_steps(engine: EngineKind, app: tauri::AppHandle) -> Result<Vec<SetupStep>, AppError> {
    Ok(inference::engine_for(engine, &app).setup_steps())
}

/// Start the app-wide server for `engine` + `model_id`, stopping any prior server first.
/// Streams `RuntimeSetupEvent` to `on_progress` so the UI shows each setup step live.
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
    // Enforce one-model-at-a-time: stop the existing server before starting the next.
    if let Some(existing) = guard.take() {
        tracing::info!(target: "inference", model_id = %existing.model_id, "stopping prior server");
        let prev = inference::engine_for(existing.engine, &app);
        let _ = prev.stop(existing).await;
    }
    let port = ephemeral_port()?;
    let engine_impl = inference::engine_for(engine, &app);

    // Enroll in the activity center. `inference-setup` is an app-wide singleton (the
    // one-model-at-a-time `ActiveServer` rule), so re-running replaces the prior row.
    activity::open(&app, "inference-setup", TaskKind::InferenceSetup, Some(model_id.clone()));
    let total_steps = engine_impl.setup_steps().len();

    // Wrap the JS `on_progress` channel: each `RuntimeSetupEvent` is forwarded verbatim to the
    // in-context `/models` checklist *and* mapped to a registry `progress` update. The mapping
    // lives here (not in the engine trait) so `InferenceEngine` impls stay unaware of activity.
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

    // App-wide reverse forward so in-sandbox agents (OpenCode) reach the host inference server at
    // a stable address. Local connection only (v1): a remote sandbox runs on a different machine
    // and cannot reach the host server. Failure here is logged, not fatal — the host-side `/models`
    // chat surface still works; only the in-container OpenCode path depends on the forward.
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
    // Tear down the reverse forward off the command's critical path: `forward terminate` can take
    // many seconds against the VM listener, and the frontend Stop awaits this command before
    // refreshing status. A brief window where the forward dials a dead port is harmless (containers
    // get connection-refused); the app-exit daemon shutdown is the backstop if this is still
    // running at exit.
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

/// Install Foundry Local via winget (advanced-settings action). Streams CLI output.
#[tauri::command]
async fn install_foundry_local(on_progress: Channel<serde_json::Value>) -> Result<(), AppError> {
    inference::install_foundry(&on_progress).await?;
    Ok(())
}

/// Raw `MIO_TEST_MODEL` JSON (a tiny test-only GGUF descriptor) for the e2e harness, or `None` in
/// normal runs. The webview cannot read process env directly, so the `/models` route asks Rust.
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
            app.manage(AuthSessionMap::default());
            app.manage(ActiveServer::default());
            app.manage(TaskRegistry::default());
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
            check_podman_ready,
            start_podman_machine,
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
            detect_machine_specs,
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
            set_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        // The app-private Mutagen daemon auto-starts on the first session command; stop it on
        // exit so we never orphan it. Best-effort: a failed stop must not block shutdown.
        if let tauri::RunEvent::Exit = event {
            // Stop the active inference server first so a llama-server sidecar holding gigabytes
            // resident is never orphaned; then stop the app-private Mutagen daemon.
            let active = app_handle.state::<ActiveServer>();
            let app_for_stop = app_handle.clone();
            tauri::async_runtime::block_on(async move {
                if let Some(handle) = active.lock().await.take() {
                    let engine = inference::engine_for(handle.engine, &app_for_stop);
                    let _ = engine.stop(handle).await;
                }
            });
            // The daemon stop below tears down every Mutagen session, including the inference
            // reverse forward, so no explicit `forward terminate` is needed here (and avoids a
            // multi-second teardown blocking app exit).
            let engine = app_handle.state::<Arc<dyn SyncEngine>>();
            tauri::async_runtime::block_on(engine.shutdown());
        }
    });
}

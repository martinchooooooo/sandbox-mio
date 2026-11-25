//! Preparation of the embedded panes: the native terminal's SSH session, and for the agent web UI a
//! route, the service inside the sandbox, a Mutagen forward to it and the webview descriptor.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Runtime};

use crate::agent_host::types::AgentHost;
use crate::error::AppError;
use crate::mio_files;
use crate::network::types::{NetworkManager, ServiceKind};
use crate::runtime::types::{RuntimeError, RuntimeManager};
use crate::runtime::PASEO_WEB_PORT;
use crate::services::routing::SandboxRouter;
use crate::terminal::types::{AttachRequest, TerminalEvent, TerminalHost};
use crate::workspace::types::{AppKind, AppPane, WebviewPane, WorkspaceRegistry};

/// Tears down every embedded pane of sandbox `name` and what backs them. Idempotent and
/// best-effort, so it is safe from the command, from undeploy/destroy and from the Destroyed event.
pub async fn close_app<R: Runtime>(app: &AppHandle<R>, name: &str) {
    let panes = crate::workspace::remove_panes(app, name);
    if panes.is_empty() {
        return;
    }
    teardown_backing(app, name, &panes).await;
    crate::workspace::emit_state(app);
}

/// Close one pane of sandbox `name`, leaving its siblings open; no-op on an already-closed pane.
pub async fn close_pane<R: Runtime>(app: &AppHandle<R>, name: &str, kind: AppKind) {
    let Some(pane) = crate::workspace::remove_pane(app, name, kind) else {
        return;
    };
    teardown_backing(app, name, std::slice::from_ref(&pane)).await;
    crate::workspace::emit_state(app);
}

/// Best-effort teardown of every embedded pane once the main window is destroyed: the app is
/// exiting, but the Mutagen daemon and its forwards would survive us.
pub fn close_all_apps_in_background<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let names: Vec<String> = {
            let registry = app.state::<WorkspaceRegistry>();
            let state = registry.lock().unwrap();
            state.apps.iter().map(|p| p.sandbox.clone()).collect()
        };
        for name in names {
            close_app(&app, &name).await;
        }
    });
}

/// Ends what backs each closed pane: a forward, the native chat's attach stream (pi keeps running
/// inside the sandbox), or the terminal's SSH session (the Herdr session keeps running).
async fn teardown_backing<R: Runtime>(app: &AppHandle<R>, name: &str, panes: &[AppPane]) {
    let network = app.state::<Arc<dyn NetworkManager>>();
    for pane in panes {
        let kind = match pane.kind {
            AppKind::Terminal => {
                app.state::<Arc<dyn TerminalHost>>().detach(name).await;
                continue;
            }
            AppKind::AgentWeb => ServiceKind::AgentWeb,
            AppKind::AgentChat => {
                app.state::<Arc<dyn AgentHost>>().detach(name).await;
                continue;
            }
        };
        network.close_forward(name, kind).await;
    }
}

/// Opens the native terminal's SSH session for `name`. Uses `locate`, not `resolve`: the session
/// spawns the engine exec itself and needs no `~/.ssh` alias
/// (`docs/features/native-terminal/02_dev.md`, Resolved Decision 8).
#[allow(clippy::too_many_arguments)]
pub async fn attach_terminal_session(
    name: &str,
    cols: u32,
    rows: u32,
    on_output: tauri::ipc::Channel<tauri::ipc::InvokeResponseBody>,
    on_event: tauri::ipc::Channel<TerminalEvent>,
    router: &SandboxRouter,
    terminal: &Arc<dyn TerminalHost>,
    app: &tauri::AppHandle,
) -> Result<(), AppError> {
    // Before any key is created: the name reaches a remote shell and names files on the host.
    crate::domain::validate_name(name)?;
    let route = router.locate(name).await?;
    let identity_path = crate::ssh::identity::ensure_sandbox_identity(app, name)?;
    let known_hosts_path = crate::ssh::identity::sandbox_known_hosts_path(&identity_path);
    let req = AttachRequest {
        name: name.to_string(),
        machine: route.machine,
        start: !route.running,
        identity_path,
        known_hosts_path,
        cols,
        rows,
    };
    Ok(terminal.attach(req, on_output, on_event).await?)
}

fn paseo_password_path(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, tauri::Error> {
    Ok(crate::paths::sandbox_pane_dir(app, name)?.join("paseo.json"))
}

fn password_in(path: &Path) -> Option<String> {
    mio_files::read_value(path)?
        .get("password")
        .and_then(|p| p.as_str())
        .map(String::from)
        .filter(|p| !p.is_empty())
}

fn write_password(path: &Path, password: &str) -> Result<(), mio_files::FileError> {
    mio_files::write_compact(path, &serde_json::json!({ "password": password }))
}

/// The Paseo password minted for `name` on its first agents-UI open, if any.
fn read_paseo_password(app: &tauri::AppHandle, name: &str) -> Option<String> {
    password_in(&paseo_password_path(app, name).ok()?)
}

/// Serializes [`paseo_password`]'s read-or-mint, so a pre-warm racing a pane open shares one password.
static PASEO_PASSWORD_LOCK: Mutex<()> = Mutex::new(());

/// The sandbox's Paseo password, minted and saved on first use. The daemon's launch hash includes
/// it, so every start path must reuse the saved one or the daemon restarts.
fn paseo_password(app: &tauri::AppHandle, name: &str) -> Result<String, AppError> {
    let _guard = PASEO_PASSWORD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(password) = read_paseo_password(app, name) {
        return Ok(password);
    }
    let password = uuid::Uuid::new_v4().simple().to_string();
    let path = paseo_password_path(app, name).map_err(RuntimeError::agent_web_unavailable)?;
    write_password(&path, &password).map_err(RuntimeError::agent_web_unavailable)?;
    Ok(password)
}

/// Sandboxes with a Paseo pre-warm running, so a second trigger does not pile up execs.
static PREWARMING: std::sync::LazyLock<Mutex<HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));

struct PrewarmSlot(String);

impl PrewarmSlot {
    fn claim(name: &str) -> Option<Self> {
        PREWARMING.lock().unwrap().insert(name.to_string()).then(|| Self(name.to_string()))
    }
}

impl Drop for PrewarmSlot {
    fn drop(&mut self) {
        PREWARMING.lock().unwrap().remove(&self.0);
    }
}

/// [`spawn_prewarm_agent_web`] for a sandbox the caller knows has an installed agent, the condition
/// that makes the Agents app available: mints the password when none is saved yet.
pub fn spawn_prewarm_installed(app: &tauri::AppHandle, name: &str) {
    let Some(slot) = PrewarmSlot::claim(name) else {
        return;
    };
    let app = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn(async move {
        let _slot = slot;
        if let Err(e) = prewarm_agent_web(&app, &name).await {
            tracing::warn!(target: "panes", sandbox = %name, error = %e, "paseo pre-warm failed");
        }
    });
}

/// Brings `name`'s Paseo daemon up off the caller's path, so a later Agents tab open finds it
/// running. Only for sandboxes that opened the agents UI before, which left a saved password; the
/// frontend covers the rest once it sees an installed agent.
pub fn spawn_prewarm_agent_web(app: &tauri::AppHandle, name: &str) {
    if read_paseo_password(app, name).is_some() {
        spawn_prewarm_installed(app, name);
    }
}

/// Never starts a stopped sandbox, and skips deployed ones, whose route is never `running`.
async fn prewarm_agent_web(app: &tauri::AppHandle, name: &str) -> Result<(), AppError> {
    use tauri::Manager;
    let opened = app
        .state::<WorkspaceRegistry>()
        .lock()
        .unwrap()
        .apps
        .iter()
        .any(|p| p.sandbox == name && p.kind == AppKind::AgentWeb);
    if opened {
        return Ok(());
    }
    let route = app.state::<SandboxRouter>().locate(name).await?;
    if !route.running {
        return Ok(());
    }
    let password = paseo_password(app, name)?;
    let began = std::time::Instant::now();
    let runtime = app.state::<Arc<dyn RuntimeManager>>();
    runtime.prepare_agent_web(name, &route.machine, &password, false).await?;
    tracing::info!(target: "panes", sandbox = %name, ms = began.elapsed().as_millis() as u64, "paseo pre-warmed");
    Ok(())
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

pub async fn prepare_agent_web_pane(
    name: &str,
    router: &SandboxRouter,
    runtime: &Arc<dyn RuntimeManager>,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) -> Result<WebviewPane, AppError> {
    let total = std::time::Instant::now();
    let route = router.resolve(name, app).await?;
    let route_ms = total.elapsed().as_millis() as u64;

    let password = paseo_password(app, name)?;

    // The route says whether a start is needed; a stopped container must run before the forward
    // dials it through `podman exec`.
    let began = std::time::Instant::now();
    if !route.running {
        runtime.ensure_running(name, &route.machine).await?;
    }
    let start_ms = began.elapsed().as_millis() as u64;

    // The forward only dials the container; the daemon's port is dialled per connection, so
    // creating it overlaps the daemon's start.
    let began = std::time::Instant::now();
    let forward = async {
        let began = std::time::Instant::now();
        let port = network
            .open_forward(name, ServiceKind::AgentWeb, &route.forward_alias, PASEO_WEB_PORT)
            .await;
        (port, began.elapsed().as_millis() as u64)
    };
    let daemon = async {
        let began = std::time::Instant::now();
        let up = runtime.prepare_agent_web(name, &route.machine, &password, true).await;
        (up, began.elapsed().as_millis() as u64)
    };
    let ((forward, forward_ms), (daemon, paseo_up_ms)) = tokio::join!(forward, daemon);
    let parallel_ms = began.elapsed().as_millis() as u64;
    if let Err(e) = daemon {
        // A forward without a daemon behind it is only a leak.
        if forward.is_ok() {
            network.close_forward(name, ServiceKind::AgentWeb).await;
        }
        return Err(e.into());
    }
    // A stale `route.running` sends the forward at a stopped container, which paseo-up has since
    // started.
    let forward = match forward {
        Err(e) => {
            tracing::info!(target: "panes", sandbox = name, error = %e, "agent web forward failed beside paseo-up, retrying");
            network
                .open_forward(name, ServiceKind::AgentWeb, &route.forward_alias, PASEO_WEB_PORT)
                .await
        }
        ok => ok,
    };
    let forward_name = format!("agentweb-{name}");
    let windows_port = forward.map_err(RuntimeError::agent_web_unavailable)?;
    let url = format!("http://127.0.0.1:{windows_port}/");

    let began = std::time::Instant::now();
    let (server_id, hostname) = fetch_paseo_server_info(windows_port, &password)
        .await
        .map_err(|e| RuntimeError::agent_web_unavailable(format!("paseo /api/status through forward: {e}")))?;
    let status_ms = began.elapsed().as_millis() as u64;
    tracing::info!(
        target: "panes",
        sandbox = name,
        route_ms,
        start_ms,
        paseo_up_ms,
        forward_ms,
        parallel_ms,
        status_ms,
        total_ms = total.elapsed().as_millis() as u64,
        "agent web pane prepared"
    );
    let endpoint = format!("localhost:{windows_port}");
    let connection_id = format!("direct:{endpoint}");
    // Only the fields the client requires: it validates strictly and deletes the whole registry on
    // any malformed one, so optional fields (timestamps, lifecycle) stay out. Since 0.16 the
    // password sits on the profile, and the daemon's connection hint reuses this entry by endpoint.
    let registry_entry = serde_json::json!([{
        "serverId": &server_id,
        "password": &password,
        "label": hostname.as_deref().unwrap_or(name),
        "connections": [{
            "id": &connection_id,
            "type": "directTcp",
            "endpoint": &endpoint,
        }],
    }]);
    let registry_json = serde_json::to_string(&registry_entry.to_string())
        .map_err(RuntimeError::agent_web_unavailable)?;
    let init_script = format!("window.localStorage.setItem('@paseo:daemon-registry', {registry_json});");

    let parsed_url = url.parse::<tauri::Url>().map_err(RuntimeError::agent_web_unavailable)?;
    Ok(WebviewPane { url: parsed_url, init_script, forward_name })
}

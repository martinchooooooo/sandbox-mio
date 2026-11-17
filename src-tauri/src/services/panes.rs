//! Preparation of the embedded panes: the native terminal's SSH session, and for the agent web UI a
//! route, the service inside the sandbox, a Mutagen forward to it and the webview descriptor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::mio_files;
use crate::network::types::{NetworkManager, ServiceKind};
use crate::runtime::types::{RuntimeError, RuntimeManager};
use crate::services::routing::SandboxRouter;
use crate::terminal::types::{AttachRequest, TerminalEvent, TerminalHost};
use crate::workspace::types::WebviewPane;

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
    crate::backends::validate_name(name)?;
    let route = router.locate(name).await?;
    let identity_path = crate::backends::ensure_sandbox_identity(app, name)?;
    let known_hosts_path = crate::backends::sandbox_known_hosts_path(&identity_path);
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

/// Brings `name`'s Paseo daemon up off the caller's path, so a later Agents tab open finds it
/// running. Only for sandboxes that opened the agents UI before, which left a saved password.
pub fn spawn_prewarm_agent_web(app: &tauri::AppHandle, name: &str) {
    let Some(password) = read_paseo_password(app, name) else {
        return;
    };
    let app = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = prewarm_agent_web(&app, &name, &password).await {
            tracing::warn!(target: "panes", sandbox = %name, error = %e, "paseo pre-warm failed");
        }
    });
}

async fn prewarm_agent_web(app: &tauri::AppHandle, name: &str, password: &str) -> Result<(), AppError> {
    use tauri::Manager;
    let route = app.state::<SandboxRouter>().locate(name).await?;
    let runtime = app.state::<Arc<dyn RuntimeManager>>();
    runtime.prepare_agent_web(name, &route.machine, password).await?;
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
    let route = router.resolve(name, app).await?;

    let password = read_paseo_password(app, name)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

    let container_port = runtime
        .prepare_agent_web(name, &route.machine, &password)
        .await?;

    let forward_name = format!("agentweb-{name}");
    let windows_port = network
        .open_forward(name, ServiceKind::AgentWeb, &route.forward_alias, container_port)
        .await
        .map_err(RuntimeError::agent_web_unavailable)?;
    let url = format!("http://127.0.0.1:{windows_port}/");

    let password_path = paseo_password_path(app, name).map_err(RuntimeError::agent_web_unavailable)?;
    write_password(&password_path, &password).map_err(RuntimeError::agent_web_unavailable)?;

    let (server_id, hostname) = fetch_paseo_server_info(windows_port, &password)
        .await
        .map_err(|e| RuntimeError::agent_web_unavailable(format!("paseo /api/status through forward: {e}")))?;
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

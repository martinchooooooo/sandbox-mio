//! Preparation of the embedded panes: resolve a route, start the service inside the sandbox, open a
//! Mutagen forward to it, and build the webview descriptor the pane is mounted from.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::AppError;
use crate::mio_files;
use crate::network::types::{NetworkManager, ServiceKind};
use crate::runtime::types::{RuntimeError, RuntimeManager};
use crate::services::routing::SandboxRouter;
use crate::workspace::types::TerminalPane;

pub async fn prepare_terminal_webview(
    name: &str,
    workspace_path: &str,
    router: &SandboxRouter,
    runtime: &Arc<dyn RuntimeManager>,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) -> Result<TerminalPane, AppError> {
    let route = router.resolve(name, app).await?;

    let (token, container_port) = runtime
        .prepare_terminal(name, &route.machine, workspace_path)
        .await?;

    let forward_name = format!("terminal-{name}");
    let windows_port = network
        .open_forward(name, ServiceKind::Terminal, &route.forward_alias, container_port)
        .await
        .map_err(RuntimeError::terminal_unavailable)?;
    let mut url = tauri::Url::parse(&format!("http://127.0.0.1:{windows_port}/"))
        .map_err(RuntimeError::terminal_unavailable)?;
    url.path_segments_mut()
        .map_err(|_| RuntimeError::terminal_unavailable("cannot-be-a-base URL"))?
        .push(name);
    let url = url.to_string();

    let token_json = serde_json::to_string(&token)
        .map_err(RuntimeError::terminal_unavailable)?;
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
        .map_err(RuntimeError::terminal_unavailable)?;
    Ok(TerminalPane { url: parsed_url, init_script, forward_name })
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

/// Reads the app-data password, adopting a pre-existing workspace copy once: the workspace file is
/// deleted only after the app-data copy is written, so a failed write loses nothing.
fn read_or_migrate_paseo_password(app_copy: &Path, workspace_copy: &Path) -> Option<String> {
    if let Some(password) = password_in(app_copy) {
        return Some(password);
    }
    let password = password_in(workspace_copy)?;
    match write_password(app_copy, &password) {
        Ok(()) => {
            if let Err(e) = std::fs::remove_file(workspace_copy) {
                tracing::warn!(target: "panes", path = %workspace_copy.display(), error = %e, "could not delete the workspace paseo password copy");
            }
        }
        Err(e) => tracing::warn!(target: "panes", error = %e, "could not move the paseo password to app data"),
    }
    Some(password)
}

pub fn read_paseo_password(app: &tauri::AppHandle, name: &str, workspace_path: &str) -> Option<String> {
    let app_copy = paseo_password_path(app, name).ok()?;
    read_or_migrate_paseo_password(&app_copy, &mio_files::mio_path(workspace_path, "paseo.json"))
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
    workspace_path: &str,
    router: &SandboxRouter,
    runtime: &Arc<dyn RuntimeManager>,
    network: &Arc<dyn NetworkManager>,
    app: &tauri::AppHandle,
) -> Result<TerminalPane, AppError> {
    let route = router.resolve(name, app).await?;

    let password = read_paseo_password(app, name, workspace_path)
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
        .map_err(RuntimeError::agent_web_unavailable)?;
    let init_script = format!("window.localStorage.setItem('@paseo:daemon-registry', {registry_json});");

    let parsed_url = url.parse::<tauri::Url>().map_err(RuntimeError::agent_web_unavailable)?;
    Ok(TerminalPane { url: parsed_url, init_script, forward_name })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mio-panes-{tag}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn workspace_copy_is_moved_to_app_data_once() {
        let root = scratch("migrate");
        let app_copy = root.join("app").join("paseo.json");
        let workspace_copy = root.join("ws").join(".mio").join("paseo.json");
        write_password(&workspace_copy, "hunter2").unwrap();

        assert_eq!(read_or_migrate_paseo_password(&app_copy, &workspace_copy).as_deref(), Some("hunter2"));
        assert!(!workspace_copy.exists());
        assert_eq!(password_in(&app_copy).as_deref(), Some("hunter2"));
        assert_eq!(read_or_migrate_paseo_password(&app_copy, &workspace_copy).as_deref(), Some("hunter2"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn app_data_copy_wins_and_absence_reads_none() {
        let root = scratch("wins");
        let app_copy = root.join("app").join("paseo.json");
        let workspace_copy = root.join("ws").join(".mio").join("paseo.json");
        assert_eq!(read_or_migrate_paseo_password(&app_copy, &workspace_copy), None);

        write_password(&app_copy, "new").unwrap();
        write_password(&workspace_copy, "old").unwrap();
        assert_eq!(read_or_migrate_paseo_password(&app_copy, &workspace_copy).as_deref(), Some("new"));
        std::fs::remove_dir_all(root).ok();
    }
}

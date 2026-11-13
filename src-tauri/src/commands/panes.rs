//! Terminal, agent-web, and workspace pane lifecycle.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::network::types::NetworkManager;
use crate::runtime::types::RuntimeManager;
use crate::services::panes::{prepare_agent_web_pane, prepare_terminal_webview};
use crate::services::routing::SandboxRouter;
use crate::workspace;
use crate::workspace::types::{AppKind, WorkspaceRegistry, WorkspaceState};

/// Start watching ports file. Idempotent; safe to call on every view.
#[tauri::command]
pub async fn watch_workspace_ports(
    name: String,
    workspace_path: String,
    router: State<'_, SandboxRouter>,
    network: State<'_, Arc<dyn NetworkManager>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    // The poll loop auto-exposes ports over this alias without going through `expose_port`'s
    // command handler, so it has to exist before the loop's first tick.
    let route = router.resolve(&name, &app).await?;
    network
        .inner()
        .clone()
        .watch_ports_file(&name, &workspace_path, &route.machine)
        .await;
    crate::services::inference_reach::spawn_ensure_for_sandbox(&app, &name);
    Ok(())
}

/// Open sandbox's embedded Terminal pane (lazily on first call).
#[tauri::command]
pub async fn open_terminal_pane(
    name: String,
    workspace_path: String,
    router: State<'_, SandboxRouter>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
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
        prepare_terminal_webview(&name, &workspace_path, &router, &runtime, &network, &app).await?;
    workspace::embed_terminal(&app, &name, &workspace_path, pane)?;
    Ok(())
}

/// Open sandbox's embedded Agents pane (lazily on first call, unlike Terminal/Dashboard).
#[tauri::command]
pub async fn open_agent_web(
    name: String,
    workspace_path: String,
    router: State<'_, SandboxRouter>,
    runtime: State<'_, Arc<dyn RuntimeManager>>,
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

    let pane =
        prepare_agent_web_pane(&name, &workspace_path, &router, &runtime, &network, &app).await?;
    workspace::embed_agent_web(&app, &name, &workspace_path, pane)?;
    Ok(())
}

/// Open sandbox's native agent chat pane, or switch to it when it is already open. The pane has no
/// webview: the main webview renders the chat while it is active.
#[tauri::command]
pub fn open_agent_chat(
    name: String,
    workspace_path: String,
    workspaces: State<'_, WorkspaceRegistry>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    let exists = workspaces
        .lock()
        .unwrap()
        .apps
        .iter()
        .any(|p| p.sandbox == name && p.kind == AppKind::AgentChat);
    if exists {
        return workspace::switch_pane(&app, &name, AppKind::AgentChat).map_err(Into::into);
    }
    Ok(workspace::open_native_pane(&app, &name, &workspace_path, AppKind::AgentChat)?)
}

/// Bring sandbox's last-active Terminal/AgentWeb pane to the front. No-op if none is open.
#[tauri::command]
pub fn switch_app(name: String, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::switch_app(&app, &name)?)
}

/// Bring a specific pane to the front.
#[tauri::command]
pub fn switch_pane(name: String, kind: AppKind, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::switch_pane(&app, &name, kind)?)
}

/// Hide current Terminal/AgentWeb pane.
#[tauri::command]
pub fn hide_active_pane(app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::hide_active_pane(&app)?)
}

/// Collapse or expand the main window's global panel.
#[tauri::command]
pub fn set_panel_collapsed(collapsed: bool, app: tauri::AppHandle) -> Result<(), AppError> {
    Ok(workspace::set_collapsed(&app, collapsed)?)
}

/// Close every embedded pane of sandbox and reclaim forwards. No-op if none is open.
#[tauri::command]
pub async fn close_workspace(name: String, app: tauri::AppHandle) -> Result<(), AppError> {
    workspace::close_app(&app, &name).await;
    Ok(())
}

/// Close exactly one app tab of sandbox. No-op if none is open.
#[tauri::command]
pub async fn close_pane(name: String, kind: AppKind, app: tauri::AppHandle) -> Result<(), AppError> {
    workspace::close_pane(&app, &name, kind).await;
    Ok(())
}

/// Read main workspace state. Backs chrome and e2e introspection; infallible.
#[tauri::command]
pub fn workspace_state(workspaces: State<'_, WorkspaceRegistry>) -> WorkspaceState {
    workspaces.lock().unwrap().clone()
}

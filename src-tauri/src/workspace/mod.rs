//! Embedded AppContainer: main window with merged NavigationPanel/Dashboard and overlay panes.

pub mod types;

use std::sync::mpsc;

use tauri::{AppHandle, Emitter, Manager, Runtime, WindowEvent};

use types::{AppKind, AppPane, Rect, TerminalPane, WorkspaceError, WorkspaceRegistry, WorkspaceState};

pub const MAIN_WINDOW: &str = "main";
pub const NAVIGATION_PANEL_LABEL: &str = "main-navigation-panel";

pub const NAVIGATION_PANEL_EXPANDED_W: f64 = 380.0;
pub const NAVIGATION_PANEL_RAIL_W: f64 = 48.0;
pub const SWITCHER_H: f64 = 64.0;

pub fn workspace_layout(width: f64, height: f64, collapsed: bool) -> Rect {
    let navigation_panel_w =
        if collapsed { NAVIGATION_PANEL_RAIL_W } else { NAVIGATION_PANEL_EXPANDED_W }.min(width);
    let switcher_h = SWITCHER_H.min(height);
    Rect {
        x: navigation_panel_w,
        y: switcher_h,
        width: (width - navigation_panel_w).max(0.0),
        height: (height - switcher_h).max(0.0),
    }
}

pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), WorkspaceError> {
    let win = |e: tauri::Error| WorkspaceError::Window(e.to_string());

    let window = tauri::window::WindowBuilder::new(app, MAIN_WINDOW)
        .title("sandbox-mio")
        .inner_size(1024.0, 720.0)
        .build()
        .map_err(win)?;

    let size = logical_size(&window)?;

    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                NAVIGATION_PANEL_LABEL,
                tauri::WebviewUrl::App(Default::default()),
            ),
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(size.width, size.height),
        )
        .map_err(win)?;

    let overlay = workspace_layout(size.width, size.height, false);
    *app.state::<WorkspaceRegistry>().lock().unwrap() =
        WorkspaceState { overlay, ..Default::default() };

    let handler_app = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(size) => relayout(&handler_app, *size),
        WindowEvent::Destroyed => {
            let app = handler_app.clone();
            tauri::async_runtime::spawn(async move { close_all_apps(&app).await });
        }
        _ => {}
    });

    Ok(())
}

/// Embed sandbox `name`'s terminal as a new content pane and make it the active tab. Caller
/// guarantees no Terminal pane exists yet for `name` (the switch-or-embed guard in `open_terminal_pane`).
pub fn embed_terminal<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
    terminal: TerminalPane,
) -> Result<(), WorkspaceError> {
    embed_external(app, name, workspace_path, terminal, AppKind::Terminal)
}

pub fn embed_agent_web<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
    pane: TerminalPane,
) -> Result<(), WorkspaceError> {
    embed_external(app, name, workspace_path, pane, AppKind::AgentWeb)
}

/// Shared embedder for the externally-served panes (Terminal, AgentWeb): a content child webview
/// over a Mutagen-forwarded URL that becomes the active tab on creation.
fn embed_external<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
    pane: TerminalPane,
    kind: AppKind,
) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    let name = name.to_string();
    let workspace_path = workspace_path.to_string();
    run_on_main(app, move || {
        embed_external_on_main(&app_in, &name, &workspace_path, pane, kind)
    })?
}

fn embed_external_on_main<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
    pane: TerminalPane,
    kind: AppKind,
) -> Result<(), WorkspaceError> {
    let win = |e: tauri::Error| WorkspaceError::Window(e.to_string());
    let window = main_window(app)?;
    let label = pane_label(name, kind);

    let registry = app.state::<WorkspaceRegistry>();
    let collapsed = registry.lock().unwrap().collapsed;
    let size = logical_size(&window)?;
    let overlay = workspace_layout(size.width, size.height, collapsed);

    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                &label,
                tauri::WebviewUrl::External(pane.url.clone()),
            )
            .initialization_script(&pane.init_script),
            tauri::LogicalPosition::new(overlay.x, overlay.y),
            tauri::LogicalSize::new(overlay.width, overlay.height),
        )
        .map_err(win)?;

    activate_new_pane(
        &window,
        &registry,
        AppPane {
            label,
            sandbox: name.to_string(),
            workspace_path: workspace_path.to_string(),
            forward_name: Some(pane.forward_name),
            kind,
            url: Some(pane.url.to_string()),
        },
        overlay,
    );

    let state = registry.lock().unwrap().clone();
    apply_layout(&window, &state, overlay)?;
    emit_state(app);
    Ok(())
}

fn pane_label(name: &str, kind: AppKind) -> String {
    let suffix = match kind {
        AppKind::Terminal => "terminal",
        AppKind::AgentWeb => "agentweb",
        AppKind::AgentChat => "agentchat",
    };
    format!("workspace-{name}-{suffix}")
}

/// Opens a pane the main webview renders itself (no child webview, no forward) and makes it the
/// active tab. Showing, hiding and bounding its label are no-ops, so the frontend draws it in the
/// content area whenever it is active.
pub fn open_native_pane<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
    kind: AppKind,
) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    let name = name.to_string();
    let workspace_path = workspace_path.to_string();
    run_on_main(app, move || {
        let window = main_window(&app_in)?;
        let registry = app_in.state::<WorkspaceRegistry>();
        let collapsed = registry.lock().unwrap().collapsed;
        let size = logical_size(&window)?;
        let overlay = workspace_layout(size.width, size.height, collapsed);
        let pane = AppPane {
            label: pane_label(&name, kind),
            sandbox: name.clone(),
            workspace_path,
            forward_name: None,
            kind,
            url: None,
        };
        activate_new_pane(&window, &registry, pane, overlay);
        emit_state(&app_in);
        Ok(())
    })?
}

/// Push a newly created pane into the registry, make it the sandbox's active tab, and hide
/// whichever pane was active before. Every pane becomes active on creation.
fn activate_new_pane<R: Runtime>(
    window: &tauri::window::Window<R>,
    registry: &WorkspaceRegistry,
    pane: AppPane,
    overlay: Rect,
) {
    let name = pane.sandbox.clone();
    let label = pane.label.clone();
    let previous = {
        let mut state = registry.lock().unwrap();
        let previous = state.active_app.take();
        state.apps.push(pane);
        state.active_app = Some(label.clone());
        state.sandbox_last_pane.insert(name, label);
        state.overlay = overlay;
        previous
    };
    if let Some(previous) = previous {
        hide_webview(window, &previous);
    }
}

/// Bring sandbox `name`'s last-active Terminal/AgentWeb pane to the front. No-op if the sandbox
/// has no such pane open — the client-side router is what puts the user on that sandbox's
/// Dashboard view in that case, not this command (Resolved Decision 2,
/// docs/features/consolidate-app-container/02_dev.md).
pub fn switch_app<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    let name = name.to_string();
    run_on_main(app, move || switch_app_on_main(&app_in, &name))?
}

fn switch_app_on_main<R: Runtime>(app: &AppHandle<R>, name: &str) -> Result<(), WorkspaceError> {
    let window = main_window(app)?;
    let registry = app.state::<WorkspaceRegistry>();
    let target = {
        let state = registry.lock().unwrap();
        state
            .sandbox_last_pane
            .get(name)
            .filter(|label| state.apps.iter().any(|p| &p.label == *label))
            .cloned()
    };
    let Some(target) = target else { return Ok(()) };
    activate_pane(&window, &registry, name, target)?;
    emit_state(app);
    Ok(())
}

/// Hide whichever Terminal/AgentWeb overlay is currently shown and clear `active_app`, with no
/// replacement pane activated. Called when the client-side view navigates to a sandbox's
/// Dashboard (or away from any sandbox) while an overlay is still visible — the merged webview's
/// own DOM change is not enough by itself, since the overlay is a native sibling surface stacked on
/// top of it (docs/features/consolidate-app-container/02_dev.md).
pub fn hide_active_pane<R: Runtime>(app: &AppHandle<R>) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    run_on_main(app, move || hide_active_pane_on_main(&app_in))?
}

fn hide_active_pane_on_main<R: Runtime>(app: &AppHandle<R>) -> Result<(), WorkspaceError> {
    let window = main_window(app)?;
    let registry = app.state::<WorkspaceRegistry>();
    let previous = registry.lock().unwrap().active_app.take();
    if let Some(previous) = previous {
        hide_webview(&window, &previous);
        emit_state(app);
    }
    Ok(())
}

/// Bring a specific pane of sandbox `name` to the front (switcher tab click), recording it as
/// that sandbox's last-active pane so a later cross-sandbox `switch_app` restores it (Resolved
/// Decision 1, `docs/features/responsive-panel/02_dev.md`).
pub fn switch_pane<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    kind: AppKind,
) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    let name = name.to_string();
    run_on_main(app, move || switch_pane_on_main(&app_in, &name, kind))?
}

fn switch_pane_on_main<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    kind: AppKind,
) -> Result<(), WorkspaceError> {
    let window = main_window(app)?;
    let registry = app.state::<WorkspaceRegistry>();
    let target = {
        let state = registry.lock().unwrap();
        state
            .apps
            .iter()
            .find(|p| p.sandbox == name && p.kind == kind)
            .map(|p| p.label.clone())
            .ok_or_else(|| WorkspaceError::NotOpen(name.to_string()))?
    };
    activate_pane(&window, &registry, name, target)?;
    emit_state(app);
    Ok(())
}

/// Show `target`, hide whichever pane was previously active, and record `target` as sandbox
/// `name`'s last-active pane. Shared by `switch_app` and `switch_pane`.
fn activate_pane<R: Runtime>(
    window: &tauri::window::Window<R>,
    registry: &WorkspaceRegistry,
    name: &str,
    target: String,
) -> Result<(), WorkspaceError> {
    let previous = {
        let mut state = registry.lock().unwrap();
        let previous = state.active_app.replace(target.clone());
        state.sandbox_last_pane.insert(name.to_string(), target.clone());
        previous
    };
    if let Some(previous) = previous.filter(|p| *p != target) {
        hide_webview(window, &previous);
    }
    show_webview(window, &target)
}

/// Flip the NavigationPanel between expanded and the thin rail. The transition snaps: one
/// `set_bounds` pass, no animation (Resolved Decision 4). With no embedded app this only records
/// the flag: the layout stays full-bleed.
pub fn set_collapsed<R: Runtime>(app: &AppHandle<R>, collapsed: bool) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    run_on_main(app, move || set_collapsed_on_main(&app_in, collapsed))?
}

fn set_collapsed_on_main<R: Runtime>(
    app: &AppHandle<R>,
    collapsed: bool,
) -> Result<(), WorkspaceError> {
    let window = main_window(app)?;
    let registry = app.state::<WorkspaceRegistry>();
    let size = logical_size(&window)?;
    let (state, overlay) = {
        let mut state = registry.lock().unwrap();
        state.collapsed = collapsed;
        let overlay = workspace_layout(size.width, size.height, collapsed);
        state.overlay = overlay;
        (state.clone(), overlay)
    };
    apply_layout(&window, &state, overlay)?;
    emit_state(app);
    Ok(())
}

/// Tear down every one of sandbox `name`'s embedded panes (Terminal and, if open, AgentWeb): close
/// their webviews, activate the neighbouring tab, terminate their forwards, drop the forward-port
/// entry, clear `sandbox_last_pane`. Idempotent and best-effort, so it is safe from the command,
/// from undeploy/destroy, and from the Destroyed event alike.
pub async fn close_app<R: Runtime>(app: &AppHandle<R>, name: &str) {
    let registry = app.state::<WorkspaceRegistry>();
    let removed = {
        let mut state = registry.lock().unwrap();
        let mut panes = Vec::new();
        state.apps.retain(|p| {
            if p.sandbox == name {
                panes.push(p.clone());
                false
            } else {
                true
            }
        });
        // Clear unconditionally, before the early return below: a sandbox's last pane can already
        // be gone from `apps` (closed individually via `close_pane`, which doesn't touch this map)
        // by the time `close_app` runs, and this entry must not outlive the sandbox regardless.
        state.sandbox_last_pane.remove(name);
        if panes.is_empty() {
            return;
        }
        if state.active_app.as_deref().is_some_and(|active| panes.iter().any(|p| p.label == active)) {
            state.active_app = state.apps.last().map(|p| p.label.clone());
        }
        (panes, state.active_app.clone(), state.collapsed)
    };
    let (panes, next_active, collapsed) = removed;

    let app_in = app.clone();
    let pane_labels: Vec<String> = panes.iter().map(|p| p.label.clone()).collect();
    let _ = run_on_main(app, move || {
        close_app_on_main(&app_in, &pane_labels, next_active, collapsed)
    });

    teardown_forwards(app, name, &panes).await;
    emit_state(app);
}

/// Close one pane of sandbox `name`, leaving its siblings open; no-op on an already-closed pane.
/// Clears `active_app`/hides the overlay if the removed pane was active, letting the client-side
/// view fall back to Dashboard — there is no Dashboard pane label to re-activate any more
/// (docs/features/consolidate-app-container/02_dev.md Resolved Decision 4).
pub async fn close_pane<R: Runtime>(app: &AppHandle<R>, name: &str, kind: AppKind) {
    let registry = app.state::<WorkspaceRegistry>();
    let removed = {
        let mut state = registry.lock().unwrap();
        let Some(index) = state.apps.iter().position(|p| p.sandbox == name && p.kind == kind)
        else {
            return;
        };
        let pane = state.apps.remove(index);
        if state.active_app.as_deref() == Some(pane.label.as_str()) {
            state.active_app = None;
        }
        (pane, state.active_app.clone(), state.collapsed)
    };
    let (pane, next_active, collapsed) = removed;

    let app_in = app.clone();
    let pane_labels = vec![pane.label.clone()];
    let _ = run_on_main(app, move || {
        close_app_on_main(&app_in, &pane_labels, next_active, collapsed)
    });

    teardown_forwards(app, name, std::slice::from_ref(&pane)).await;
    emit_state(app);
}

/// Ends what backs each closed pane: a forward, or the native chat's attach stream (pi keeps
/// running inside the sandbox).
async fn teardown_forwards<R: Runtime>(app: &AppHandle<R>, name: &str, panes: &[AppPane]) {
    let network = app.state::<std::sync::Arc<dyn crate::network::types::NetworkManager>>();
    for pane in panes {
        let kind = match pane.kind {
            AppKind::Terminal => crate::network::types::ServiceKind::Terminal,
            AppKind::AgentWeb => crate::network::types::ServiceKind::AgentWeb,
            AppKind::AgentChat => {
                app.state::<std::sync::Arc<dyn crate::agent_host::types::AgentHost>>().detach(name).await;
                continue;
            }
        };
        network.close_forward(name, kind).await;
    }
}

/// The window/webview-mutating half of [`close_app`], run on the main thread: close every pane's
/// webview, show the neighbouring tab, re-layout the survivors.
fn close_app_on_main<R: Runtime>(
    app: &AppHandle<R>,
    pane_labels: &[String],
    next_active: Option<String>,
    collapsed: bool,
) {
    let Some(window) = app.get_window(MAIN_WINDOW) else { return };
    for label in pane_labels {
        if let Some(webview) = window.webviews().into_iter().find(|w| w.label() == label) {
            let _ = webview.close();
        }
    }
    if let Some(next) = &next_active {
        let _ = show_webview(&window, next);
    }
    if let Ok(size) = logical_size(&window) {
        let overlay = workspace_layout(size.width, size.height, collapsed);
        let registry = app.state::<WorkspaceRegistry>();
        let state = registry.lock().unwrap().clone();
        if apply_layout(&window, &state, overlay).is_ok() {
            registry.lock().unwrap().overlay = overlay;
        }
    }
}

/// Best-effort teardown of every embedded pane (main-window Destroyed → app is exiting, but the
/// Mutagen daemon and its forwards would survive us).
pub async fn close_all_apps<R: Runtime>(app: &AppHandle<R>) {
    let registry = app.state::<WorkspaceRegistry>();
    let names: Vec<String> = {
        let state = registry.lock().unwrap();
        state.apps.iter().map(|p| p.sandbox.clone()).collect()
    };
    for name in names {
        close_app(app, &name).await;
    }
}

/// Recompute and apply the layout after an OS-window resize, preserving collapse and tab state.
/// The merged NavigationPanel webview always fills the window, so it alone is re-bounded here even
/// when no overlay is active.
fn relayout<R: Runtime>(app: &AppHandle<R>, size: tauri::PhysicalSize<u32>) {
    let Some(window) = app.get_window(MAIN_WINDOW) else { return };
    let Ok(scale) = window.scale_factor() else { return };
    let logical = size.to_logical::<f64>(scale);
    let registry = app.state::<WorkspaceRegistry>();
    let state = registry.lock().unwrap().clone();
    let overlay = workspace_layout(logical.width, logical.height, state.collapsed);
    // Coalesce resize bursts: an unchanged overlay rect (minimise/restore, sub-pixel moves) implies
    // the window's logical size didn't change either, so there is nothing to apply.
    if overlay == state.overlay {
        return;
    }
    let full = Rect { x: 0.0, y: 0.0, width: logical.width, height: logical.height };
    let _ = set_webview_bounds(&window, NAVIGATION_PANEL_LABEL, full);
    if apply_layout(&window, &state, overlay).is_ok() {
        registry.lock().unwrap().overlay = overlay;
    }
}

/// Broadcast the fresh state so the switcher tab bar and the NavigationPanel's collapse chrome
/// track what Rust applied. Best-effort: a dropped event never fails the underlying operation.
fn emit_state<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<WorkspaceRegistry>().lock().unwrap().clone();
    let _ = app.emit("workspace://state", state);
}

/// Run `f` on the main/event-loop thread and block the caller until it finishes, returning its
/// result. Window/webview mutation (`add_child`, `set_bounds`, `show`/`hide`) must happen on the
/// main thread on Windows: calling it from an async command's tokio thread crashes the process
/// (tauri-apps/tauri#10010). Every entry point below is reachable from an async command or a
/// spawned task, so each routes its window-touching body through here.
fn run_on_main<R, F, T>(app: &AppHandle<R>, f: F) -> Result<T, WorkspaceError>
where
    R: Runtime,
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| WorkspaceError::Window(e.to_string()))?;
    rx.recv().map_err(|_| WorkspaceError::Window("main-thread task panicked".into()))
}

fn main_window<R: Runtime>(app: &AppHandle<R>) -> Result<tauri::window::Window<R>, WorkspaceError> {
    app.get_window(MAIN_WINDOW).ok_or(WorkspaceError::MainWindowMissing)
}

fn logical_size<R: Runtime>(
    window: &tauri::window::Window<R>,
) -> Result<tauri::LogicalSize<f64>, WorkspaceError> {
    let win = |e: tauri::Error| WorkspaceError::Window(e.to_string());
    let scale = window.scale_factor().map_err(win)?;
    Ok(window.inner_size().map_err(win)?.to_logical::<f64>(scale))
}

/// `set_bounds` every Terminal/AgentWeb overlay pane to `overlay`. Hidden panes are re-bounded too
/// so they reappear at the right geometry when a later `switch_app`/`switch_pane` shows them.
fn apply_layout<R: Runtime>(
    window: &tauri::window::Window<R>,
    state: &WorkspaceState,
    overlay: Rect,
) -> Result<(), WorkspaceError> {
    for pane in &state.apps {
        set_webview_bounds(window, &pane.label, overlay)?;
    }
    Ok(())
}

fn set_webview_bounds<R: Runtime>(
    window: &tauri::window::Window<R>,
    label: &str,
    rect: Rect,
) -> Result<(), WorkspaceError> {
    if let Some(webview) = window.webviews().into_iter().find(|w| w.label() == label) {
        webview
            .set_bounds(tauri::Rect {
                position: tauri::LogicalPosition::new(rect.x, rect.y).into(),
                size: tauri::LogicalSize::new(rect.width, rect.height).into(),
            })
            .map_err(|e| WorkspaceError::Window(e.to_string()))?;
    }
    Ok(())
}

fn hide_webview<R: Runtime>(window: &tauri::window::Window<R>, label: &str) {
    if let Some(webview) = window.webviews().into_iter().find(|w| w.label() == label) {
        let _ = webview.hide();
    }
}

fn show_webview<R: Runtime>(
    window: &tauri::window::Window<R>,
    label: &str,
) -> Result<(), WorkspaceError> {
    if let Some(webview) = window.webviews().into_iter().find(|w| w.label() == label) {
        webview.show().map_err(|e| WorkspaceError::Window(e.to_string()))?;
        // `.show()` alone makes the pane visible but leaves OS input focus wherever it was before
        // (often the previously-active pane, now hidden) — clicks/keystrokes sent to this pane
        // then land without ever firing its elements' focus handlers. `set_focus` here is what
        // makes a freshly-activated pane actually receive input.
        webview.set_focus().map_err(|e| WorkspaceError::Window(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_layout_places_overlay_beside_nav_and_below_switcher() {
        let r = workspace_layout(1024.0, 720.0, false);
        assert_eq!(r.x, NAVIGATION_PANEL_EXPANDED_W);
        assert_eq!(r.y, SWITCHER_H);
        assert_eq!(r.width, 1024.0 - NAVIGATION_PANEL_EXPANDED_W);
        assert_eq!(r.height, 720.0 - SWITCHER_H);
    }

    #[test]
    fn collapsed_layout_shrinks_nav_width_and_grows_overlay() {
        let expanded = workspace_layout(1024.0, 720.0, false);
        let collapsed = workspace_layout(1024.0, 720.0, true);
        assert_eq!(collapsed.x, NAVIGATION_PANEL_RAIL_W);
        assert!(collapsed.x < expanded.x);
        assert!(collapsed.width > expanded.width);
        assert_eq!(collapsed.x + collapsed.width, 1024.0);
    }

    #[test]
    fn degenerate_container_sizes_never_go_negative() {
        let r = workspace_layout(0.0, 0.0, false);
        assert!(r.width >= 0.0 && r.height >= 0.0);
        let r = workspace_layout(30.0, 20.0, true);
        assert!(r.x <= 30.0);
        assert!(r.width >= 0.0 && r.height >= 0.0);
    }
}

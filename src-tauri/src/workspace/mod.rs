//! The embedded multi-webview workspace IS the app's main window: a bare container `Window`
//! (label `main`, created in setup instead of tauri.conf.json) hosting the full SvelteKit app as
//! the left collapsible panel (`main-global`), an app-switcher tab bar (`main-switcher`), and
//! content child webviews per sandbox whose workspace is open.

pub mod types;

use std::sync::mpsc;

use tauri::{AppHandle, Emitter, Manager, Runtime, WindowEvent};

use types::{
    AppKind, AppPane, Rect, TerminalPane, WorkspaceError, WorkspaceRects, WorkspaceRegistry,
    WorkspaceState,
};

/// Label of the main container window (bare `Window`, no webview of its own).
pub const MAIN_WINDOW: &str = "main";
/// Label of the global-panel child webview hosting the full app UI.
pub const GLOBAL_LABEL: &str = "main-global";
/// Label of the app-switcher child webview (tab bar above the content area).
pub const SWITCHER_LABEL: &str = "main-switcher";

/// Global panel width when expanded and an app is embedded, logical px. Cross-referenced by the
/// `max-[420px]:` CSS breakpoint in `src/routes/projects/+page.svelte` and
/// `src/routes/projects/SandboxItem.svelte` (docs/features/responsive-panel/02_dev.md, Resolved
/// Decision 4) — that breakpoint must stay above this value if it ever changes.
pub const GLOBAL_EXPANDED_W: f64 = 380.0;
/// Global panel width when collapsed to the thin rail (never zero: the rail keeps the expand
/// button reachable), logical px.
pub const GLOBAL_RAIL_W: f64 = 48.0;
/// Switcher bar height across the top of the content area, logical px.
pub const SWITCHER_H: f64 = 40.0;

/// Compute the three child-webview rectangles from the container's logical size, the collapse
/// flag, and whether any app is embedded. With no content the global panel takes the whole window
/// (the collapse flag is moot: there is nothing to yield space to). Pure so it is unit-testable
/// without a window; every geometry change (embed, close, collapse toggle, OS resize) flows
/// through here.
pub fn workspace_layout(
    width: f64,
    height: f64,
    collapsed: bool,
    has_content: bool,
) -> WorkspaceRects {
    if !has_content {
        return WorkspaceRects {
            global: Rect { x: 0.0, y: 0.0, width, height },
            switcher: Rect { x: width, y: 0.0, width: 0.0, height: 0.0 },
            content: Rect { x: width, y: 0.0, width: 0.0, height: 0.0 },
        };
    }
    let global_w = if collapsed { GLOBAL_RAIL_W } else { GLOBAL_EXPANDED_W }.min(width);
    let content_w = (width - global_w).max(0.0);
    let switcher_h = SWITCHER_H.min(height);
    WorkspaceRects {
        global: Rect { x: 0.0, y: 0.0, width: global_w, height },
        switcher: Rect { x: global_w, y: 0.0, width: content_w, height: switcher_h },
        content: Rect {
            x: global_w,
            y: switcher_h,
            width: content_w,
            height: (height - switcher_h).max(0.0),
        },
    }
}

/// Create the main workspace window with its two chrome children. Called once from setup; the
/// window replaces the `windows` entry that used to live in tauri.conf.json (a config window
/// always gets its own webview, which a multi-webview container must not have).
pub fn create_main_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), WorkspaceError> {
    let win = |e: tauri::Error| WorkspaceError::Window(e.to_string());

    let window = tauri::window::WindowBuilder::new(app, MAIN_WINDOW)
        .title("sandbox-mio")
        .inner_size(1024.0, 720.0)
        .build()
        .map_err(win)?;

    let size = logical_size(&window)?;
    let rects = workspace_layout(size.width, size.height, false, false);

    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                GLOBAL_LABEL,
                tauri::WebviewUrl::App(Default::default()),
            ),
            tauri::LogicalPosition::new(rects.global.x, rects.global.y),
            tauri::LogicalSize::new(rects.global.width, rects.global.height),
        )
        .map_err(win)?;
    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                SWITCHER_LABEL,
                tauri::WebviewUrl::App("workspace/switcher".into()),
            ),
            tauri::LogicalPosition::new(rects.switcher.x, rects.switcher.y),
            tauri::LogicalSize::new(rects.switcher.width, rects.switcher.height),
        )
        .map_err(win)?;

    *app.state::<WorkspaceRegistry>().lock().unwrap() =
        WorkspaceState { rects, ..Default::default() };

    // Child bounds are absolute and do not reflow: re-layout on every OS resize. Closing the main
    // window exits the app, but the Mutagen forwards live in the daemon and would outlive it, so
    // Destroyed still routes into best-effort teardown of every embedded pane.
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
/// guarantees no pane exists for `name` (the switch-or-embed guard in the `open_workspace`
/// command). The first pane shrinks the global panel from full-bleed to `GLOBAL_EXPANDED_W`.
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
    let suffix = match kind {
        AppKind::Terminal => "terminal",
        AppKind::AgentWeb => "agentweb",
        AppKind::Dashboard => "dashboard",
    };
    let label = format!("workspace-{name}-{suffix}");

    let registry = app.state::<WorkspaceRegistry>();
    let collapsed = registry.lock().unwrap().collapsed;
    let size = logical_size(&window)?;
    let rects = workspace_layout(size.width, size.height, collapsed, true);

    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                &label,
                tauri::WebviewUrl::External(pane.url.clone()),
            )
            .initialization_script(&pane.init_script),
            tauri::LogicalPosition::new(rects.content.x, rects.content.y),
            tauri::LogicalSize::new(rects.content.width, rects.content.height),
        )
        .map_err(win)?;

    let previous = {
        let mut state = registry.lock().unwrap();
        let previous = state.active_app.take();
        state.apps.push(AppPane {
            label: label.clone(),
            sandbox: name.to_string(),
            workspace_path: workspace_path.to_string(),
            forward_name: pane.forward_name,
            kind,
            url: pane.url.to_string(),
        });
        state.active_app = Some(label.clone());
        state.sandbox_last_pane.insert(name.to_string(), label);
        state.rects = rects;
        previous
    };
    if let Some(previous) = previous {
        hide_webview(&window, &previous);
    }

    let state = registry.lock().unwrap().clone();
    apply_layout(&window, &state, &rects)?;
    emit_state(app);
    Ok(())
}

/// Embed sandbox `name`'s package/agent management dashboard as a second content pane alongside
/// its terminal (`docs/features/responsive-panel/02_dev.md` Phase C, eager creation). Unlike
/// `embed_terminal`, this never becomes the active pane on creation: `open_workspace` always
/// embeds Terminal first, so Dashboard starts hidden until `switch_pane` brings it forward. No
/// Mutagen forward backs it, since the dashboard is pure Tauri IPC, not an external process.
///
/// Must only ever be called from an `async` Tauri command: `add_child` deadlocks the whole app
/// when reached from a synchronous command, because `run_on_main`'s blocking wait never returns
/// (confirmed by the de-risking spike, `docs/features/responsive-panel/03_e2e.md`, 2026-07-02).
/// `open_workspace` is already `async fn`, so this is satisfied today; do not add a sync wrapper.
pub fn embed_dashboard<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
) -> Result<(), WorkspaceError> {
    let app_in = app.clone();
    let name = name.to_string();
    let workspace_path = workspace_path.to_string();
    run_on_main(app, move || embed_dashboard_on_main(&app_in, &name, &workspace_path))?
}

fn embed_dashboard_on_main<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    workspace_path: &str,
) -> Result<(), WorkspaceError> {
    let win = |e: tauri::Error| WorkspaceError::Window(e.to_string());
    let window = main_window(app)?;
    let label = format!("workspace-{name}-dashboard");
    let route = format!("workspace/dashboard?sandbox={name}");

    let registry = app.state::<WorkspaceRegistry>();
    let collapsed = registry.lock().unwrap().collapsed;
    let size = logical_size(&window)?;
    let rects = workspace_layout(size.width, size.height, collapsed, true);

    window
        .add_child(
            tauri::webview::WebviewBuilder::new(&label, tauri::WebviewUrl::App(route.clone().into())),
            tauri::LogicalPosition::new(rects.content.x, rects.content.y),
            tauri::LogicalSize::new(rects.content.width, rects.content.height),
        )
        .map_err(win)?;

    {
        let mut state = registry.lock().unwrap();
        state.apps.push(AppPane {
            label: label.clone(),
            sandbox: name.to_string(),
            workspace_path: workspace_path.to_string(),
            forward_name: String::new(),
            kind: AppKind::Dashboard,
            url: format!("/{route}"),
        });
        state.rects = rects;
    }
    hide_webview(&window, &label);

    let state = registry.lock().unwrap().clone();
    apply_layout(&window, &state, &rects)?;
    emit_state(app);
    Ok(())
}

/// Bring sandbox `name`'s last-active pane to the front, defaulting to its Terminal pane if none
/// is recorded yet. Used by the projects-list "Open workspace" button when panes already exist.
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
            .or_else(|| {
                state
                    .apps
                    .iter()
                    .find(|p| p.sandbox == name && p.kind == AppKind::Terminal)
                    .map(|p| p.label.clone())
            })
            .ok_or_else(|| WorkspaceError::NotOpen(name.to_string()))?
    };
    activate_pane(&window, &registry, name, target)?;
    emit_state(app);
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

/// Flip the global panel between expanded and the thin rail. The transition snaps: one
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
    let (state, rects) = {
        let mut state = registry.lock().unwrap();
        state.collapsed = collapsed;
        let rects =
            workspace_layout(size.width, size.height, collapsed, !state.apps.is_empty());
        state.rects = rects;
        (state.clone(), rects)
    };
    apply_layout(&window, &state, &rects)?;
    emit_state(app);
    Ok(())
}

/// Tear down every one of sandbox `name`'s embedded panes (Terminal and, if open, Dashboard and
/// AgentWeb): close their webviews, activate the neighbouring tab, terminate the Terminal and
/// AgentWeb panes' forwards (Dashboard has none), drop the forward-port entry, clear
/// `sandbox_last_pane`, re-layout
/// (closing the last pane returns the global panel to full-bleed). Idempotent and best-effort, so
/// it is safe from the command, from undeploy/destroy, and from the Destroyed event alike.
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
        if panes.is_empty() {
            return;
        }
        if state.active_app.as_deref().is_some_and(|active| panes.iter().any(|p| p.label == active)) {
            state.active_app = state.apps.last().map(|p| p.label.clone());
        }
        state.sandbox_last_pane.remove(name);
        (panes, state.active_app.clone(), state.collapsed, state.apps.is_empty())
    };
    let (panes, next_active, collapsed, now_empty) = removed;

    let app_in = app.clone();
    let pane_labels: Vec<String> = panes.iter().map(|p| p.label.clone()).collect();
    let _ = run_on_main(app, move || {
        close_app_on_main(&app_in, &pane_labels, next_active, collapsed, now_empty)
    });

    let network = app.state::<std::sync::Arc<dyn crate::network::types::NetworkManager>>();
    for pane in &panes {
        let kind = match pane.kind {
            AppKind::Terminal => Some(crate::network::types::ServiceKind::Terminal),
            AppKind::AgentWeb => Some(crate::network::types::ServiceKind::AgentWeb),
            AppKind::Dashboard => None,
        };
        if let Some(kind) = kind {
            network.close_forward(name, kind).await;
        }
    }
    emit_state(app);
}

/// The window/webview-mutating half of [`close_app`], run on the main thread: close every pane's
/// webview, show the neighbouring tab, re-layout the survivors.
fn close_app_on_main<R: Runtime>(
    app: &AppHandle<R>,
    pane_labels: &[String],
    next_active: Option<String>,
    collapsed: bool,
    now_empty: bool,
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
        let rects = workspace_layout(size.width, size.height, collapsed, !now_empty);
        let registry = app.state::<WorkspaceRegistry>();
        let state = registry.lock().unwrap().clone();
        if apply_layout(&window, &state, &rects).is_ok() {
            registry.lock().unwrap().rects = rects;
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
fn relayout<R: Runtime>(app: &AppHandle<R>, size: tauri::PhysicalSize<u32>) {
    let Some(window) = app.get_window(MAIN_WINDOW) else { return };
    let Ok(scale) = window.scale_factor() else { return };
    let logical = size.to_logical::<f64>(scale);
    let registry = app.state::<WorkspaceRegistry>();
    let state = registry.lock().unwrap().clone();
    let rects =
        workspace_layout(logical.width, logical.height, state.collapsed, !state.apps.is_empty());
    // Coalesce resize bursts: identical rects (minimise/restore, sub-pixel moves) apply nothing.
    if rects == state.rects {
        return;
    }
    if apply_layout(&window, &state, &rects).is_ok() {
        registry.lock().unwrap().rects = rects;
    }
}

/// Broadcast the fresh state so the switcher tab bar and the global panel's collapse chrome
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

/// `set_bounds` every child of the workspace. Hidden app panes are re-bounded too so they
/// reappear at the right geometry when a later `switch_app` shows them.
fn apply_layout<R: Runtime>(
    window: &tauri::window::Window<R>,
    state: &WorkspaceState,
    rects: &WorkspaceRects,
) -> Result<(), WorkspaceError> {
    set_webview_bounds(window, GLOBAL_LABEL, rects.global)?;
    set_webview_bounds(window, SWITCHER_LABEL, rects.switcher)?;
    for pane in &state.apps {
        set_webview_bounds(window, &pane.label, rects.content)?;
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_content_the_global_panel_fills_the_window() {
        let r = workspace_layout(1024.0, 720.0, false, false);
        assert_eq!(r.global, Rect { x: 0.0, y: 0.0, width: 1024.0, height: 720.0 });
        assert_eq!(r.switcher.width, 0.0);
        assert_eq!(r.content.width, 0.0);
        // The collapse flag is moot with nothing to yield space to.
        let r = workspace_layout(1024.0, 720.0, true, false);
        assert_eq!(r.global.width, 1024.0);
    }

    #[test]
    fn expanded_layout_partitions_the_container() {
        let r = workspace_layout(1024.0, 720.0, false, true);
        assert_eq!(r.global.width, GLOBAL_EXPANDED_W);
        assert_eq!(r.global.height, 720.0);
        assert_eq!(r.switcher.x, GLOBAL_EXPANDED_W);
        assert_eq!(r.switcher.width, 1024.0 - GLOBAL_EXPANDED_W);
        assert_eq!(r.switcher.height, SWITCHER_H);
        assert_eq!(r.content.x, GLOBAL_EXPANDED_W);
        assert_eq!(r.content.y, SWITCHER_H);
        assert_eq!(r.content.width, 1024.0 - GLOBAL_EXPANDED_W);
        assert_eq!(r.content.height, 720.0 - SWITCHER_H);
    }

    #[test]
    fn collapsed_layout_shrinks_global_to_the_rail_and_grows_content() {
        let expanded = workspace_layout(1024.0, 720.0, false, true);
        let collapsed = workspace_layout(1024.0, 720.0, true, true);
        assert_eq!(collapsed.global.width, GLOBAL_RAIL_W);
        assert!(collapsed.content.x < expanded.content.x);
        assert!(collapsed.content.width > expanded.content.width);
        assert_eq!(collapsed.content.x + collapsed.content.width, 1024.0);
    }

    #[test]
    fn regions_do_not_overlap() {
        let r = workspace_layout(800.0, 600.0, false, true);
        assert_eq!(r.global.x + r.global.width, r.switcher.x);
        assert_eq!(r.global.x + r.global.width, r.content.x);
        assert_eq!(r.switcher.y + r.switcher.height, r.content.y);
    }

    #[test]
    fn degenerate_container_sizes_never_go_negative() {
        let r = workspace_layout(0.0, 0.0, false, true);
        assert!(r.global.width >= 0.0 && r.switcher.width >= 0.0 && r.content.width >= 0.0);
        assert!(r.switcher.height >= 0.0 && r.content.height >= 0.0);
        let r = workspace_layout(30.0, 20.0, true, true);
        assert!(r.global.width <= 30.0);
        assert!(r.content.width >= 0.0 && r.content.height >= 0.0);
    }
}

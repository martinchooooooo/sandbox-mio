use std::sync::{Arc, Mutex};

use serde::Serialize;

/// The live state of the single main workspace window, held in Tauri state (mirroring
/// `ForwardPortMap`) so the switcher chrome can target the right webviews and undeploy/destroy
/// can close a sandbox's embedded pane.
pub type WorkspaceRegistry = Arc<Mutex<WorkspaceState>>;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("the main workspace window is not available")]
    MainWindowMissing,
    #[error("no embedded app is open for sandbox {0:?}")]
    NotOpen(String),
    #[error("workspace window error: {0}")]
    Window(String),
}

/// Which app a content pane embeds. v1 embeds only the terminal; further kinds arrive with the
/// multi-app phase (Phase F in docs/features/embedded-windows/02_dev.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Terminal,
}

/// One embedded app surface: a content child webview plus the Mutagen forward backing its URL.
#[derive(Debug, Clone, Serialize)]
pub struct AppPane {
    pub label: String,
    /// The sandbox this pane belongs to; switcher tabs and teardown are keyed by it.
    pub sandbox: String,
    pub forward_name: String,
    pub kind: AppKind,
    pub url: String,
}

/// Inputs for embedding the terminal as a content child webview. Produced by
/// `prepare_terminal_webview` in `lib.rs`, shared with the standalone `open_terminal` path so the
/// auto-login init script is authored once.
pub struct TerminalPane {
    pub url: tauri::Url,
    pub init_script: String,
    pub forward_name: String,
}

/// An axis-aligned rectangle in logical (DPI-independent) units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The three child-webview rectangles computed by [`super::workspace_layout`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct WorkspaceRects {
    pub global: Rect,
    pub switcher: Rect,
    pub content: Rect,
}

/// Main-window workspace bookkeeping. `rects` holds the last-*applied* logical rectangles, not
/// live physical bounds, so `workspace_state` reports deterministic DPI-independent geometry to
/// the e2e read-back seam (docs/features/embedded-windows/03_e2e.md). Broadcast to the chrome
/// webviews as the `workspace://state` event after every mutation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkspaceState {
    pub apps: Vec<AppPane>,
    pub active_app: Option<String>,
    pub collapsed: bool,
    pub rects: WorkspaceRects,
}

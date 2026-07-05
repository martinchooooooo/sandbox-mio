use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// The live state of the single AppContainer window, held in Tauri state (mirroring
/// `ForwardPortMap`) so the switcher chrome can target the right webviews and undeploy/destroy
/// can close a sandbox's embedded pane.
pub type WorkspaceRegistry = Arc<Mutex<WorkspaceState>>;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("the AppContainer window is not available")]
    MainWindowMissing,
    #[error("no embedded app is open for sandbox {0:?}")]
    NotOpen(String),
    #[error("AppContainer window error: {0}")]
    Window(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    Terminal,
    AgentWeb,
}

/// One embedded app surface: a content child webview plus the Mutagen forward backing its URL.
#[derive(Debug, Clone, Serialize)]
pub struct AppPane {
    pub label: String,
    pub sandbox: String,
    pub workspace_path: String,
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

/// AppContainer state bookkeeping. `overlay` holds the last-*applied* logical rectangle for the
/// Terminal/AgentWeb overlay webview, not live physical bounds, so `workspace_state` reports
/// deterministic DPI-independent geometry to the e2e read-back seam
/// (docs/features/embedded-windows/03_e2e.md). Broadcast to the merged webview as the
/// `workspace://state` event after every mutation.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkspaceState {
    pub apps: Vec<AppPane>,
    pub active_app: Option<String>,
    pub collapsed: bool,
    pub overlay: Rect,
    /// Sandbox name -> label of that sandbox's own last-active pane (Terminal or AgentWeb), so
    /// switching back to a sandbox restores whichever pane the user had open instead of always
    /// defaulting to Terminal.
    pub sandbox_last_pane: HashMap<String, String>,
}

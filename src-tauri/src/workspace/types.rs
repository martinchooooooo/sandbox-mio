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
    /// The native agent chat, rendered by the main webview: no child webview and no forward.
    AgentChat,
}

/// One app surface. Terminal and AgentWeb are a content child webview plus the Mutagen forward
/// backing its URL; a native pane (AgentChat) has neither, so `forward_name` and `url` are `None`
/// (`docs/features/native-agent-ui/02_dev.md`, Resolved Decision 14).
#[derive(Debug, Clone, Serialize)]
pub struct AppPane {
    pub label: String,
    pub sandbox: String,
    pub workspace_path: String,
    pub forward_name: Option<String>,
    pub kind: AppKind,
    pub url: Option<String>,
}

/// Inputs for embedding the terminal as a content child webview. Produced by
/// `prepare_terminal_webview` in `src-tauri/src/services/panes.rs`, shared with the standalone `open_terminal` path so the
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

/// AppContainer window state. `overlay` is the last-applied logical rectangle for Terminal/AgentWeb
/// overlay (deterministic for e2e tests). Broadcast to merged webview as `workspace://state` event.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkspaceState {
    pub apps: Vec<AppPane>,
    pub active_app: Option<String>,
    pub collapsed: bool,
    /// Logical rectangle for overlay webview (deterministic, DPI-independent).
    pub overlay: Rect,
    /// Per-sandbox last-active pane (Terminal or AgentWeb) for restoring on switch.
    pub sandbox_last_pane: HashMap<String, String>,
}

//! The native terminal: attach to, write to, resize and detach from a sandbox's zellij session.

use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

use crate::error::AppError;
use crate::services::panes::attach_terminal_session;
use crate::services::routing::SandboxRouter;
use crate::terminal::types::{TerminalEvent, TerminalHost};

/// Opens the sandbox's terminal session at `cols` by `rows`, replacing any earlier one. Raw output
/// bytes stream on `on_output`; the session's end arrives on `on_event`.
#[tauri::command]
pub async fn terminal_attach(
    name: String,
    cols: u32,
    rows: u32,
    on_output: Channel<InvokeResponseBody>,
    on_event: Channel<TerminalEvent>,
    router: State<'_, SandboxRouter>,
    terminal: State<'_, Arc<dyn TerminalHost>>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    attach_terminal_session(&name, cols, rows, on_output, on_event, &router, &terminal, &app).await
}

/// Writes typed or pasted input to the live session.
#[tauri::command]
pub async fn terminal_input(
    name: String,
    data: String,
    terminal: State<'_, Arc<dyn TerminalHost>>,
) -> Result<(), AppError> {
    Ok(terminal.input(&name, data.into_bytes()).await?)
}

/// Tells the live session the view's new size.
#[tauri::command]
pub async fn terminal_resize(
    name: String,
    cols: u32,
    rows: u32,
    terminal: State<'_, Arc<dyn TerminalHost>>,
) -> Result<(), AppError> {
    Ok(terminal.resize(&name, cols, rows).await?)
}

/// Ends this app's session; the zellij session keeps running inside the sandbox.
#[tauri::command]
pub async fn terminal_detach(name: String, terminal: State<'_, Arc<dyn TerminalHost>>) -> Result<(), AppError> {
    terminal.detach(&name).await;
    Ok(())
}

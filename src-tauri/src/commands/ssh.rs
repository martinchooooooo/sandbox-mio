//! Host SSH client access to a sandbox over its managed `~/.ssh` alias.

use tauri::State;

use crate::error::AppError;
use crate::services::routing::SandboxRouter;
use crate::ssh::types::SshAccess;

/// Refreshes the sandbox's SSH alias and returns what a host client needs to connect through it.
#[tauri::command]
pub async fn sandbox_ssh_access(
    name: String,
    router: State<'_, SandboxRouter>,
    app: tauri::AppHandle,
) -> Result<SshAccess, AppError> {
    crate::services::ssh_access::access(&name, &router, &app).await
}

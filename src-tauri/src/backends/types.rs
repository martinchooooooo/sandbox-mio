use serde::Serialize;
use tauri::ipc::Channel;

/// A sandbox instance as seen by the frontend. Returned by all [`SandboxBackend`] operations.
/// Field `status` is backend-defined (e.g. `"created"`, `"running"`, `"exited"`).
#[derive(Debug, Serialize, Clone)]
pub struct Sandbox {
    pub id: String,
    pub name: String,
    pub status: String,
}

/// Primary interface for sandbox lifecycle management.
///
/// Tauri state holds an `Arc<dyn SandboxBackend>` — commands receive it via [`tauri::State`]
/// and delegate directly to these methods.
///
/// Implementations live in `backends/mod.rs`:
/// - [`PodmanBollardBackend`] — Docker-compatible API over a Windows named pipe (default)
/// - [`PodmanShellBackend`] — SSH into the Podman machine and run CLI commands
///
/// To swap backends at startup, change the assignment in `lib.rs::run()`.
#[async_trait::async_trait]
pub trait SandboxBackend: Send + Sync {
    /// Pull the base image if absent, then create a stopped container named `name`.
    async fn create(&self, name: &str) -> Result<Sandbox, String>;
    /// Return all containers (running and stopped).
    async fn list(&self) -> Result<Vec<Sandbox>, String>;
    /// Start a stopped container.
    async fn start(&self, name: &str) -> Result<(), String>;
    /// Stop a running container.
    async fn stop(&self, name: &str) -> Result<(), String>;
    /// Stream container events to the frontend channel until the connection drops.
    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), String>;
}

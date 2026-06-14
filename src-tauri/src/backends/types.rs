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
/// To swap backends at startup, change `default_backend()` in `backends/mod.rs`.
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

/// Internal helper trait shared by Podman backends. Not part of the public API.
///
/// New Podman backends should implement this too.
///
/// Env vars that control behaviour:
/// - `PODMAN_MACHINE` — Podman machine name (default: `podman-machine-default`; E2E tests set this to `mio-tests`)
/// - `PODMAN_PIPE` — overrides the full named-pipe path; skips `podman_pipe()` derivation entirely
pub(crate) trait PodmanBackend {
    /// Returns the Windows named-pipe path for the Podman socket.
    /// Override entirely by setting `PODMAN_PIPE`; otherwise derived from `podman_machine()`.
    fn podman_pipe(&self) -> String {
        if let Ok(pipe) = std::env::var("PODMAN_PIPE") {
            return pipe;
        }
        let mut machine = self.podman_machine();
        // Only podman-machine-default seems to carry the extra prefix
        if !machine.starts_with("podman-") {
            machine = format!("podman-{machine}")
        }
        format!("//./pipe/{}", machine)
    }

    /// Returns the Podman machine name, defaulting to `podman-machine-default`.
    /// Override by setting `PODMAN_MACHINE` (the E2E harness sets this to `mio-tests`).
    fn podman_machine(&self) -> String {
        std::env::var("PODMAN_MACHINE")
            .unwrap_or_else(|_| "podman-machine-default".to_string())
    }

    /// Base image used when creating sandboxes. Override in a subtype to change the image.
    fn base_image() -> &'static str { "alpine" }
    /// Tag for `base_image`. Override in a subtype to pin a specific version.
    fn base_image_version() -> &'static str { "latest" }
}

/// Talks to Podman via the Docker-compatible API over a Windows named pipe.
pub(super) struct PodmanBollardBackend;

/// Drives Podman by SSHing into the Podman machine and running CLI commands.
/// Use this when the named pipe is not accessible. Each operation spawns a subprocess.
pub(super) struct PodmanShellBackend;

impl PodmanBackend for PodmanBollardBackend {}
impl PodmanBackend for PodmanShellBackend {}

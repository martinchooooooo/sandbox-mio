use crate::backends::types::BackendError;

/// Global Podman machine lifecycle (distinct from per-connection backends).
/// Methods operate on the default machine's CLI, WSL kernel, and rootless API socket.
#[async_trait::async_trait]
pub trait MachineManager: Send + Sync {
    /// True if the default machine's `Running` field is true. Call `poll_ready` for socket readiness.
    async fn machine_is_running(&self) -> Result<bool, BackendError>;
    /// True if any machine other than the default is running. Conservative: errors return false.
    async fn check_other_machines_running(&self) -> bool;
    /// Run `podman machine start` for the default machine.
    async fn run_machine_start(&self) -> Result<(), BackendError>;
    /// Run `wsl --shutdown` to reset a stuck WSL kernel service.
    async fn run_wsl_shutdown(&self) -> Result<(), BackendError>;
    /// Launch the rootless Podman API service inside the default machine. Call `poll_ready` next.
    async fn activate_podman_socket(&self) -> Result<(), BackendError>;
    /// Poll the socket up to `attempts` times (500ms apart). The authoritative readiness check.
    async fn poll_ready(&self, attempts: usize) -> Result<(), BackendError>;
    /// True if WSL2 is installed and responsive. Gate for first-run `podman machine init`.
    async fn wsl_is_ready(&self) -> bool;
    /// Install WSL2. Returns `Ok(true)` if reboot required, `Ok(false)` if already usable.
    async fn install_wsl(&self) -> Result<bool, BackendError>;
    /// True if the default machine exists in `podman machine list`, regardless of running state.
    async fn machine_exists(&self) -> Result<bool, BackendError>;
    /// Create the default machine. Downloads machine image on first call (takes minutes, needs network).
    async fn init_machine(&self) -> Result<(), BackendError>;
    /// Force-clear a stuck machine mid-import (e.g., failed WSL import). Idempotent, always `Ok`.
    async fn reset_stuck_machine(&self) -> Result<(), BackendError>;
}

/// The real [`MachineManager`], driving `podman machine` CLI, `wsl`, and the rootless API socket.
pub(super) struct PodmanMachineManager {
    /// The process seam every `podman` call routes through — real in production, scripted under
    /// `MIO_MOCK_BACKEND=1`. Shares one instance with `PodmanCliBackend` (see
    /// `docs/features/scriptable-mock-backend/02_dev.md`, "shared runner").
    pub(super) runner: std::sync::Arc<dyn crate::mockctl::types::ProcessRunner>,
}

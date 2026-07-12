use crate::backends::types::BackendError;

/// Global Podman machine-lifecycle operations, independent of any single system connection.
///
/// Distinct from [`crate::backends::types::SandboxBackend`], whose methods each act on a named
/// connection: these drive the `podman machine` CLI, the WSL kernel, and the rootless API socket
/// for the *default* machine as a whole. Tauri state holds an `Arc<dyn MachineManager>`; the
/// `check_podman_ready` and `start_podman_machine` commands receive it via [`tauri::State`].
///
/// Two implementations: [`PodmanMachineManager`] (drives the real CLI) and the in-memory mock
/// selected when `MIO_MOCK_BACKEND=1`. Choose between them in
/// [`crate::machine::default_machine_manager`] — the single place the mock flag steers
/// machine-lifecycle behavior.
#[async_trait::async_trait]
pub trait MachineManager: Send + Sync {
    /// True if the default machine reports `Running: true` in `podman machine list`. A `true`
    /// here does not mean the rootless socket is bound — [`MachineManager::poll_ready`] is the
    /// authoritative gate.
    async fn machine_is_running(&self) -> Result<bool, BackendError>;
    /// True if any machine *other than* the default reports `Running: true`. Conservative: any
    /// error returns `false` so the WSL shutdown guard never disrupts a running test machine.
    async fn check_other_machines_running(&self) -> bool;
    /// Run `podman machine start` for the default machine. `Ok` on exit 0, captured stderr otherwise.
    async fn run_machine_start(&self) -> Result<(), BackendError>;
    /// `wsl --shutdown` to reset a stuck WSL kernel service, then wait for WSL to settle. Always `Ok`.
    async fn run_wsl_shutdown(&self) -> Result<(), BackendError>;
    /// Launch the rootless Podman API service inside the default machine. Fire-and-forget: always
    /// follow with [`MachineManager::poll_ready`], which is the real readiness signal. Always `Ok`.
    async fn activate_podman_socket(&self) -> Result<(), BackendError>;
    /// Poll the default machine's socket up to `attempts` times, 500 ms apart. The authoritative
    /// readiness gate — `machine_is_running` and `activate_podman_socket` are insufficient alone.
    async fn poll_ready(&self, attempts: usize) -> Result<(), BackendError>;
    /// True if WSL2 is installed and its kernel service answers (`wsl --status`). First-run
    /// provisioning gate: `podman machine init` cannot succeed without it.
    async fn wsl_is_ready(&self) -> bool;
    /// Run `wsl --install --no-distribution`. Returns `Ok(true)` when the install requires a
    /// reboot before Podman can proceed (the common case for a fresh install), `Ok(false)` if WSL
    /// was already usable.
    async fn install_wsl(&self) -> Result<bool, BackendError>;
    /// True if `podman machine list` already has an entry named
    /// [`crate::backends::DEFAULT_CONNECTION`], regardless of its running state.
    async fn machine_exists(&self) -> Result<bool, BackendError>;
    /// Run `podman machine init` for the default machine. Downloads the machine image on first
    /// call, so this can take minutes and needs network access.
    async fn init_machine(&self) -> Result<(), BackendError>;
    /// Force-clear a default machine stuck mid-import (e.g. a WSL import that failed with exit
    /// `0xffffffff`): removes podman's own bookkeeping for it, then unregisters its WSL distro.
    /// Best-effort and idempotent — always `Ok`, since "no machine, no distro" is the goal state
    /// whether or not either resource existed. The next `init_machine` recreates it cleanly.
    async fn reset_stuck_machine(&self) -> Result<(), BackendError>;
}

/// The real [`MachineManager`], driving the global `podman machine` CLI, `wsl`, and the rootless
/// socket for the default machine. Holds no state: every operation reads fresh CLI output, since a
/// machine's boot state and SSH port change on each start.
pub(super) struct PodmanMachineManager;

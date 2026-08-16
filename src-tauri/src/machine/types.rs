use tauri::ipc::Channel;

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
    pub(super) runner: std::sync::Arc<dyn crate::process::types::ProcessRunner>,
}

/// One stage of bringing a machine online, provider-neutral. Rust owns only the stable step `id`;
/// English copy lives in the frontend (`src/lib/machine/MachineSteps.svelte`) as a hardcoded literal
/// until the feature's i18n pass, mirroring `inference::types::SetupStep`.
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SetupStep {
    pub id: String,
}

impl SetupStep {
    pub fn new(id: &str) -> Self {
        SetupStep { id: id.to_string() }
    }
}

/// Streamed per-step progress from [`MachineReadinessProvider::prepare`], mirroring
/// `inference::types::RuntimeSetupEvent`. `RebootRequired` is Podman-only (a fresh WSL2 install
/// needs a restart); no microsandbox setup step ever emits it.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MachineSetupEvent {
    StepStart { id: String },
    StepDone { id: String, detail: Option<String> },
    StepSkipped { id: String, reason: String },
    StepFailed { id: String, error: String },
    RebootRequired { message: String },
    Ready,
}

impl MachineSetupEvent {
    /// Best-effort emit; a dropped progress event must never fail `prepare`.
    pub fn send(&self, channel: &Channel<serde_json::Value>) {
        channel.send(serde_json::to_value(self).unwrap()).ok();
    }
}

/// Provider-neutral snapshot of whether a machine is usable right now, and whether bringing it
/// online means first-run setup or just a restart.
#[derive(serde::Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct MachineReadiness {
    pub ready: bool,
    /// True when the machine has never been set up. Drives "Create"/"Set up" vs. "Start" copy in
    /// `CreateVmStep.svelte`. Microsandbox has no distinct "exists but stopped" state, so
    /// `MsbMachineManager` reports this as `true` whenever `ready` is `false`.
    pub needs_setup: bool,
}

/// Provider-neutral welcome-flow readiness contract (`docs/features/microsandbox-provider/01_ideas.md`,
/// "Provider readiness replacing `MachineManager`'s Podman shape"). Additive alongside
/// [`MachineManager`] rather than a narrowing of it: `MachineManager` remains Podman's own
/// recovery-gate contract backing `src/routes/projects/+page.svelte`, verified end-to-end by
/// `e2e/test/specs/standard/podman-startup-recovery.spec.ts` against a real machine, which this
/// phase has no way to re-verify. This trait backs only the welcome flow's first-run checklist
/// (`src/lib/welcome/CreateVmStep.svelte`), a surface with no existing e2e coverage to break
/// (`docs/features/microsandbox-provider/02_dev.md`, Phase I).
#[async_trait::async_trait]
pub trait MachineReadinessProvider: Send + Sync {
    /// Ordered plan for `prepare`. Pure metadata (no side effects) so the UI can render the full
    /// checklist before `prepare` runs the first step.
    fn setup_steps(&self) -> Vec<SetupStep>;
    /// Probe whether the machine is usable right now, without changing anything.
    async fn readiness(&self) -> MachineReadiness;
    /// Bring the machine online, emitting a [`MachineSetupEvent`] per `setup_steps` stage.
    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError>;
}

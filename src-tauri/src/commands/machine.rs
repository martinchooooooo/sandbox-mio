//! Podman machine readiness and provisioning, plus the provider-neutral setup checklist.

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::activity;
use crate::activity::types::TaskKind;
use crate::error::AppError;
use crate::machine;
use crate::machine::types::{
    BringUpCause, BringUpEntry, BringUpEvent, BringUpFailure, BringUpNote, MachineManager,
    STEP_MACHINE_INIT, STEP_MACHINE_START, STEP_SOCKET_ACTIVATE, STEP_WSL_INSTALL,
};
use crate::machine::MachineRunners;
use crate::provider::types::ProviderId;

/// Probe whether default Podman machine is running and responding.
#[tauri::command]
pub async fn check_podman_ready(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    if !machine.machine_is_running().await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))?
    {
        return Err(AppError::PodmanUnavailable("Podman machine is not running".into()));
    }
    machine.poll_ready(1).await
        .map_err(|_| AppError::PodmanUnavailable("Podman socket is not ready".into()))
}

/// Check if default Podman machine exists (not just running).
#[tauri::command]
pub async fn machine_needs_init(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<bool, AppError> {
    Ok(!machine.machine_exists().await.map_err(|e| AppError::PodmanUnavailable(e.to_string()))?)
}

/// Progress line for a bring-up event on the activity task; `None` for events that only matter to
/// the welcome flow's checklist.
fn progress_detail(event: BringUpEvent) -> Option<&'static str> {
    match event {
        BringUpEvent::StepStart(STEP_WSL_INSTALL) => Some("Checking WSL2…"),
        BringUpEvent::StepStart(STEP_MACHINE_INIT) => Some("Checking Podman machine…"),
        BringUpEvent::StepStart(STEP_MACHINE_START) => Some("Starting machine…"),
        BringUpEvent::StepStart(STEP_SOCKET_ACTIVATE) => Some("Activating Podman socket…"),
        BringUpEvent::StepStart(_) | BringUpEvent::StepDone(_) | BringUpEvent::StepSkipped { .. } => {
            None
        }
        BringUpEvent::Note(BringUpNote::InstallingWsl) => Some("Installing WSL2…"),
        BringUpEvent::Note(BringUpNote::CreatingMachine) => Some("Creating Podman machine…"),
        BringUpEvent::Note(BringUpNote::RetryingStart) => Some("Retrying…"),
        BringUpEvent::Note(BringUpNote::RetryingAfterWslReset) => Some("Retrying after WSL reset…"),
        BringUpEvent::Note(BringUpNote::WaitingForSocket) => Some("Waiting for socket…"),
    }
}

/// Failure text and IPC error for a stopped bring-up. A reboot requirement keeps its own
/// [`AppError`] kind so the frontend can tell the user to restart rather than retry.
fn bring_up_failure(failure: BringUpFailure) -> activity::TaskFailure<AppError> {
    let (message, error): (String, fn(String) -> AppError) = match failure.cause {
        BringUpCause::RebootRequired(message) => (message, AppError::PodmanRebootRequired),
        BringUpCause::Failed(e) => {
            let prefix = match failure.step {
                STEP_WSL_INSTALL => "Failed to install WSL2",
                STEP_MACHINE_INIT => "Failed to set up Podman machine",
                STEP_MACHINE_START => "Failed to start Podman machine",
                _ => "Podman socket did not become ready",
            };
            (format!("{prefix}: {e}"), AppError::PodmanUnavailable)
        }
    };
    activity::TaskFailure::new(message.clone(), error(message))
}

/// Runs the shared bring-up sequence, reporting each step on an already-open activity task.
/// Shared by [`start_podman_machine`] and [`provision_podman`] so the task is opened and resolved
/// exactly once per command — the caller of a failing `start_podman_machine` must not record a
/// second failure (`docs/features/podman-startup-recovery/02_dev.md`, Concrete Model).
async fn run_bring_up(
    task: &activity::TaskScope,
    machine: &Arc<dyn MachineManager>,
    entry: BringUpEntry,
) -> Result<(), activity::TaskFailure<AppError>> {
    machine::bring_up(machine.as_ref(), entry, |event| {
        if let Some(detail) = progress_detail(event) {
            task.progress(None, detail);
        }
    })
    .await
    .map_err(bring_up_failure)
}

/// Startup sequence for the default Podman machine from the start step on. Every error path fails
/// the activity task.
#[tauri::command]
pub async fn start_podman_machine(
    app: tauri::AppHandle,
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    activity::scope(&app, "podman-startup", TaskKind::PodmanStartup, None, |task| async move {
        run_bring_up(&task, &machine, BringUpEntry::FromStart).await
    })
    .await
}

/// First-run setup for Podman (ensure WSL2, init machine, start). Returns [`AppError::PodmanRebootRequired`] if restart needed.
#[tauri::command]
pub async fn provision_podman(
    app: tauri::AppHandle,
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    activity::scope(&app, "podman-startup", TaskKind::PodmanStartup, None, |task| async move {
        run_bring_up(&task, &machine, BringUpEntry::Full).await
    })
    .await
}

/// Advanced-settings debug utility: force-clear a default Podman machine stuck mid-import (the
/// WSL import failure surfaces as `wsl --import ... failed: exit status 0xffffffff`), so the user
/// can retry `provision_podman` without manually running `wsl --unregister` themselves.
#[tauri::command]
pub async fn reset_stuck_podman_machine(
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    machine
        .reset_stuck_machine()
        .await
        .map_err(|e| AppError::PodmanUnavailable(e.to_string()))
}

/// Provider-neutral welcome-flow checklist plan (`CreateVmStep.svelte`), distinct from the Podman-only
/// recovery gate above (`docs/features/microsandbox-provider/02_dev.md`, Phase I).
#[tauri::command]
pub fn machine_setup_steps(
    provider: ProviderId,
    runners: State<'_, MachineRunners>,
) -> Vec<machine::types::SetupStep> {
    machine::readiness_provider_for(provider, &runners).setup_steps()
}

/// Probe whether `provider`'s machine is usable right now, without changing anything.
#[tauri::command]
pub async fn machine_readiness(
    provider: ProviderId,
    runners: State<'_, MachineRunners>,
) -> Result<machine::types::MachineReadiness, AppError> {
    Ok(machine::readiness_provider_for(provider, &runners).readiness().await)
}

/// Bring `provider`'s machine online, streaming a `MachineSetupEvent` per setup-checklist row.
/// Also an activity task, so a run started in the background stays visible outside its own card.
#[tauri::command]
pub async fn prepare_machine(
    provider: ProviderId,
    on_progress: Channel<serde_json::Value>,
    runners: State<'_, MachineRunners>,
    app: tauri::AppHandle,
) -> Result<(), AppError> {
    const TASK: &str = "machine-setup";
    let readiness = machine::readiness_provider_for(provider, &runners);
    let total_steps = readiness.setup_steps().len();
    let app_for_steps = app.clone();
    activity::scope(&app, TASK, TaskKind::MachineSetup, None, |_task| async move {
        let channel = activity::step_progress_channel(&app_for_steps, TASK, total_steps, on_progress);
        readiness.prepare(&channel).await.map_err(|e| {
            let message = e.to_string();
            activity::TaskFailure::new(message, AppError::from(e))
        })
    })
    .await
}

//! Podman machine readiness and provisioning, plus the provider-neutral setup checklist.

use std::sync::Arc;

use tauri::{ipc::Channel, State};

use crate::activity;
use crate::activity::types::TaskKind;
use crate::error::AppError;
use crate::machine;
use crate::machine::types::MachineManager;
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

/// Brings the default Podman machine up, reporting each step on an already-open activity task.
/// Shared by [`start_podman_machine`] and [`provision_podman`] so the task is opened and resolved
/// exactly once per command — the caller of a failing `start_podman_machine` must not record a
/// second failure (`docs/features/podman-startup-recovery/02_dev.md`, Concrete Model).
async fn start_machine(
    task: &activity::TaskScope,
    machine: &Arc<dyn MachineManager>,
) -> Result<(), activity::TaskFailure<AppError>> {
    task.progress(None, "Starting machine…");
    // `podman machine start` on an already-running machine exits non-zero ("already running"),
    // indistinguishable from a real failure below — checking first avoids a needless
    // `wsl --shutdown` of a machine a stale frontend gate or double click re-triggered against.
    if !matches!(machine.machine_is_running().await, Ok(true)) && machine.run_machine_start().await.is_err() {
        if machine.check_other_machines_running().await {
            task.progress(None, "Retrying…");
        } else {
            task.progress(None, "Retrying after WSL reset…");
            machine.run_wsl_shutdown().await.ok();
        }
        if let Err(e) = machine.run_machine_start().await {
            let err = format!("Failed to start Podman machine: {e}");
            return Err(activity::TaskFailure::new(err.clone(), AppError::PodmanUnavailable(err)));
        }
    }

    task.progress(None, "Activating Podman socket…");
    machine.activate_podman_socket().await.ok();

    task.progress(None, "Waiting for socket…");
    if let Err(e) = machine.poll_ready(4).await {
        let err = format!("Podman socket did not become ready: {e}");
        return Err(activity::TaskFailure::new(err.clone(), AppError::PodmanUnavailable(err)));
    }
    Ok(())
}

/// Full startup sequence for default Podman machine. Every error path fails the activity task.
#[tauri::command]
pub async fn start_podman_machine(
    app: tauri::AppHandle,
    machine: State<'_, Arc<dyn MachineManager>>,
) -> Result<(), AppError> {
    activity::scope(&app, "podman-startup", TaskKind::PodmanStartup, None, |task| async move {
        start_machine(&task, &machine).await
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
        task.progress(None, "Checking WSL2…");
        if !machine.wsl_is_ready().await {
            task.progress(None, "Installing WSL2…");
            match machine.install_wsl().await {
                Ok(true) => {
                    let msg = "WSL2 was installed and needs a restart to finish. Restart Windows,                                then try again."
                        .to_string();
                    return Err(activity::TaskFailure::new(
                        msg.clone(),
                        AppError::PodmanRebootRequired(msg),
                    ));
                }
                Ok(false) => {}
                Err(e) => {
                    let err = format!("Failed to install WSL2: {e}");
                    return Err(activity::TaskFailure::new(
                        err.clone(),
                        AppError::PodmanUnavailable(err),
                    ));
                }
            }
        }

        task.progress(None, "Checking Podman machine…");
        match machine.machine_exists().await {
            Ok(true) => {}
            Ok(false) => {
                task.progress(None, "Creating Podman machine…");
                if let Err(e) = machine.init_machine().await {
                    let err = format!("Failed to create Podman machine: {e}");
                    return Err(activity::TaskFailure::new(
                        err.clone(),
                        AppError::PodmanUnavailable(err),
                    ));
                }
            }
            Err(e) => {
                let err = format!("Failed to inspect Podman machines: {e}");
                return Err(activity::TaskFailure::new(
                    err.clone(),
                    AppError::PodmanUnavailable(err),
                ));
            }
        }

        start_machine(&task, &machine).await
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
#[tauri::command]
pub async fn prepare_machine(
    provider: ProviderId,
    on_progress: Channel<serde_json::Value>,
    runners: State<'_, MachineRunners>,
) -> Result<(), AppError> {
    Ok(machine::readiness_provider_for(provider, &runners).prepare(&on_progress).await?)
}

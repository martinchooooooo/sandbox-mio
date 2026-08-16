//! Microsandbox readiness: `msb` binary present, Windows `HypervisorPlatform` optional feature
//! enabled, `msb doctor` clean. `docs/features/microsandbox-provider/01_ideas.md`, "Provider
//! readiness replacing `MachineManager`'s Podman shape".

use std::sync::Arc;

use tauri::ipc::Channel;

use crate::backends::types::BackendError;
use crate::process::types::ProcessRunner;

use super::types::{MachineReadiness, MachineReadinessProvider, MachineSetupEvent, SetupStep};

/// Report `step` as the failed row and carry `error` out as the command's failure, so a checklist
/// row is never left showing progress for work that stopped here.
fn fail(on_step: &Channel<serde_json::Value>, step: &str, error: String) -> BackendError {
    MachineSetupEvent::StepFailed { id: step.into(), error: error.clone() }.send(on_step);
    BackendError::CommandFailed(error)
}

/// `msb` writes its diagnostics to stdout as often as to stderr, so an empty stderr is not an
/// empty explanation — falling back keeps the user from seeing a blank failure message.
fn command_error(out: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if stdout.is_empty() {
        format!("`msb doctor --fix` failed with {}", out.status)
    } else {
        stdout
    }
}

/// Real [`MachineReadinessProvider`] driving `msb doctor`/`msb doctor --fix`. Unlike
/// `PodmanMachineManager`, microsandbox has no separate "exists but stopped" machine state, so
/// there is nothing else to hold beyond the process seam.
pub struct MsbMachineManager {
    runner: Arc<dyn ProcessRunner>,
}

impl MsbMachineManager {
    pub fn new(runner: Arc<dyn ProcessRunner>) -> Self {
        MsbMachineManager { runner }
    }
}

#[async_trait::async_trait]
impl MachineReadinessProvider for MsbMachineManager {
    fn setup_steps(&self) -> Vec<SetupStep> {
        ["msb-binary", "hypervisor-platform", "msb-doctor"].into_iter().map(SetupStep::new).collect()
    }

    async fn readiness(&self) -> MachineReadiness {
        let Ok(out) = self.runner.run(&["doctor"]).await else {
            // The runner call itself failing (not just a non-zero exit) means `msb` isn't
            // reachable at all, e.g. not on `PATH`.
            return MachineReadiness { ready: false, needs_setup: true };
        };
        let ready = out.status.success()
            && String::from_utf8_lossy(&out.stdout).to_lowercase().contains("ready");
        MachineReadiness { ready, needs_setup: !ready }
    }

    /// `msb doctor --fix` is one call covering all three checklist rows (HANDOVER.md, "the
    /// enable-and-no-reboot path the spike confirmed") — there is no per-row microsandbox command
    /// to drive individually, unlike Podman's four distinct steps. Each row therefore reports on
    /// what that one call actually proves: spawning it at all proves the binary is installed, its
    /// exit status reports on the only thing `--fix` changes (the Windows Hypervisor Platform
    /// feature), and the last row re-probes rather than rubber-stamping itself — `--fix` exiting 0
    /// is not on its own evidence the host ended up ready.
    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError> {
        MachineSetupEvent::StepStart { id: "msb-binary".into() }.send(on_step);
        let out = match self.runner.run(&["doctor", "--fix"]).await {
            Ok(out) => out,
            // The call never ran, so the binary itself is what is missing.
            Err(e) => return Err(fail(on_step, "msb-binary", e.to_string())),
        };
        // Spawning the process at all is what proves the binary is installed; its exit status
        // belongs to the row below, not this one.
        MachineSetupEvent::StepDone { id: "msb-binary".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "hypervisor-platform".into() }.send(on_step);
        if !out.status.success() {
            return Err(fail(on_step, "hypervisor-platform", command_error(&out)));
        }
        MachineSetupEvent::StepDone { id: "hypervisor-platform".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "msb-doctor".into() }.send(on_step);
        if !self.readiness().await.ready {
            return Err(fail(
                on_step,
                "msb-doctor",
                "microsandbox still reports the host as not ready after `msb doctor --fix`"
                    .to_string(),
            ));
        }
        MachineSetupEvent::StepDone { id: "msb-doctor".into(), detail: None }.send(on_step);

        MachineSetupEvent::Ready.send(on_step);
        Ok(())
    }
}

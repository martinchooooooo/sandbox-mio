//! Microsandbox readiness: `msb` binary present, Windows `HypervisorPlatform` optional feature
//! enabled, `msb doctor` clean. `docs/features/microsandbox-provider/01_ideas.md`, "Provider
//! readiness replacing `MachineManager`'s Podman shape".

use std::sync::Arc;

use tauri::ipc::Channel;

use crate::backends::types::BackendError;
use crate::mockctl::types::ProcessRunner;

use super::types::{MachineReadiness, MachineReadinessProvider, MachineSetupEvent, SetupStep};

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
    /// to drive individually, unlike Podman's four distinct steps.
    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError> {
        MachineSetupEvent::StepStart { id: "msb-binary".into() }.send(on_step);
        let out = self.runner.run(&["doctor", "--fix"]).await.map_err(|e| {
            let msg = e.to_string();
            MachineSetupEvent::StepFailed { id: "msb-binary".into(), error: msg.clone() }
                .send(on_step);
            BackendError::CommandFailed(msg)
        })?;
        MachineSetupEvent::StepDone { id: "msb-binary".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "hypervisor-platform".into() }.send(on_step);
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            MachineSetupEvent::StepFailed { id: "hypervisor-platform".into(), error: err.clone() }
                .send(on_step);
            return Err(BackendError::CommandFailed(err));
        }
        MachineSetupEvent::StepDone { id: "hypervisor-platform".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "msb-doctor".into() }.send(on_step);
        MachineSetupEvent::StepDone { id: "msb-doctor".into(), detail: None }.send(on_step);

        MachineSetupEvent::Ready.send(on_step);
        Ok(())
    }
}

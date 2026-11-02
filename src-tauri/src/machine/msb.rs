//! Microsandbox readiness: `msb` binary present at or above [`MIN_MSB_VERSION`], Windows
//! `HypervisorPlatform` optional feature enabled, `msb doctor` clean. `docs/features/microsandbox-provider/01_ideas.md`, "Provider
//! readiness replacing `MachineManager`'s Podman shape".

use std::sync::Arc;

use tauri::ipc::Channel;

use crate::backends::types::BackendError;
use crate::process::types::ProcessRunner;

use super::types::{MachineReadiness, MachineReadinessProvider, MachineSetupEvent, SetupStep};

/// The only `msb` release the network-policy spikes validated
/// (`docs/features/microsandbox-network-policy/02_dev.md`, Resolved Decision 1).
pub const MIN_MSB_VERSION: (u32, u32, u32) = (0, 7, 12);

/// The first `x.y.z` token in `msb --version` output (`msb 0.7.12`), or `None` if there is none.
pub fn parse_msb_version(stdout: &str) -> Option<(u32, u32, u32)> {
    stdout.split_whitespace().find_map(|token| {
        let token = token.trim_start_matches('v');
        let mut parts = token.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch: String = parts.next()?.chars().take_while(|c| c.is_ascii_digit()).collect();
        Some((major, minor, patch.parse().ok()?))
    })
}

fn version_supported(stdout: &str) -> bool {
    parse_msb_version(stdout).is_some_and(|v| v >= MIN_MSB_VERSION)
}

fn version_instruction(stdout: &str) -> String {
    let (a, b, c) = MIN_MSB_VERSION;
    let found = stdout.trim();
    let found = if found.is_empty() { "an unknown version" } else { found };
    format!(
        "microsandbox {a}.{b}.{c} or newer is required (found {found}). Copy msb.exe to a temporary          folder and run `<copy> self update` there — the running msb.exe cannot replace itself."
    )
}

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

    /// `msb --version` stdout, or `None` when the binary could not be spawned at all.
    async fn version_output(&self) -> Option<String> {
        let out = self.runner.run(&["--version"]).await.ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

#[async_trait::async_trait]
impl MachineReadinessProvider for MsbMachineManager {
    fn setup_steps(&self) -> Vec<SetupStep> {
        ["msb-binary", "msb-version", "hypervisor-platform", "msb-doctor"].into_iter().map(SetupStep::new).collect()
    }

    async fn readiness(&self) -> MachineReadiness {
        match self.version_output().await {
            Some(v) if version_supported(&v) => {}
            _ => return MachineReadiness { ready: false, needs_setup: true },
        }
        let Ok(out) = self.runner.run(&["doctor"]).await else {
            // The runner call itself failing (not just a non-zero exit) means `msb` isn't
            // reachable at all, e.g. not on `PATH`.
            return MachineReadiness { ready: false, needs_setup: true };
        };
        let ready = out.status.success()
            && String::from_utf8_lossy(&out.stdout).to_lowercase().contains("ready");
        MachineReadiness { ready, needs_setup: !ready }
    }

    /// `msb --version` covers the first two rows: spawning it proves the binary is installed and
    /// its output is the version check. `msb doctor --fix` then covers the rest — its exit status
    /// reports on the only thing `--fix` changes (the Windows Hypervisor Platform feature), and the
    /// last row re-probes rather than rubber-stamping itself, since `--fix` exiting 0 is not on its
    /// own evidence the host ended up ready.
    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError> {
        MachineSetupEvent::StepStart { id: "msb-binary".into() }.send(on_step);
        let Some(version) = self.version_output().await else {
            return Err(fail(on_step, "msb-binary", "the msb CLI could not be started".to_string()));
        };
        MachineSetupEvent::StepDone { id: "msb-binary".into(), detail: None }.send(on_step);

        // The app never runs `msb self update` itself
        // (docs/features/microsandbox-network-policy/02_dev.md, Resolved Decision 14).
        MachineSetupEvent::StepStart { id: "msb-version".into() }.send(on_step);
        if !version_supported(&version) {
            return Err(fail(on_step, "msb-version", version_instruction(&version)));
        }
        MachineSetupEvent::StepDone { id: "msb-version".into(), detail: Some(version) }.send(on_step);

        let out = match self.runner.run(&["doctor", "--fix"]).await {
            Ok(out) => out,
            Err(e) => return Err(fail(on_step, "hypervisor-platform", e.to_string())),
        };

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_msb_version_line() {
        assert_eq!(parse_msb_version("msb 0.7.12"), Some((0, 7, 12)));
        assert_eq!(parse_msb_version("msb v0.8.0-rc1
"), Some((0, 8, 0)));
        assert_eq!(parse_msb_version("command not found"), None);
        assert_eq!(parse_msb_version(""), None);
    }

    #[test]
    fn minimum_version_gates_readiness() {
        assert!(!version_supported("msb 0.6.6"));
        assert!(!version_supported("msb 0.7.11"));
        assert!(version_supported("msb 0.7.12"));
        assert!(version_supported("msb 0.8.0"));
        assert!(!version_supported("garbage"));
    }
}

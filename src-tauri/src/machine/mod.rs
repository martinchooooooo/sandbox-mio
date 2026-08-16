//! Podman machine-lifecycle management: start default machine, reset WSL, activate rootless socket.
//! Separate from backends since it drives the whole machine rather than single connections.

pub mod msb;
pub mod types;

use std::process::Output;
use std::sync::Arc;

use tauri::ipc::Channel;

use crate::backends::types::BackendError;
use crate::backends::{podman_command, DEFAULT_MACHINE_NAME};
use crate::process::types::ProcessRunner;
use crate::process::detached_command_std;
use crate::provider::types::ProviderId;

use types::{MachineManager, MachineReadinessProvider, MachineReadiness, MachineSetupEvent, PodmanMachineManager, SetupStep};

/// Rootless Podman API socket inside the default machine (UID 1000). Must be activated manually
/// via `podman system service` because the machine's user systemd session does not start on boot.
const PODMAN_SOCKET: &str = "/run/user/1000/podman/podman.sock";

pub fn default_machine_manager(runner: Arc<dyn ProcessRunner>) -> std::sync::Arc<dyn MachineManager> {
    std::sync::Arc::new(PodmanMachineManager { runner })
}

/// Runs a host-mutating `wsl` call on the blocking pool, or no-ops under `MIO_MOCK_BACKEND=1` —
/// without this gate, e2e's light tier would run `wsl --unregister podman-machine-default` for
/// real, deleting the dev's machine and its disk. Read-only probes (`wsl --status`) skip this gate.
///
/// `None` means only "skipped, mock backend"; a panicked blocking task surfaces as `Some(Err(..))`
/// so callers cannot mistake a failure for the skip.
async fn run_host_wsl(args: &[&str]) -> Option<std::io::Result<Output>> {
    if crate::backends::mock_backend_enabled() {
        return None;
    }
    let owned: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let joined = tokio::task::spawn_blocking(move || {
        detached_command_std("wsl").args(&owned).output()
    })
    .await;
    Some(joined.unwrap_or_else(|e| Err(std::io::Error::other(e))))
}

/// Returns stdout on success, or the captured stderr as `Err` on a non-zero exit. Local to this
/// module, mirroring the per-module `check` in `backends`/`runtime`.
fn check(out: Output) -> Result<String, BackendError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(BackendError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

/// Probe a connection's live socket with `podman --connection <name> info`. Success means the
/// rootless API answers — the readiness signal `poll_ready` polls for.
async fn verify_host(runner: &Arc<dyn ProcessRunner>, name: &str) -> Result<(), BackendError> {
    let out = runner
        .run(&["--connection", name, "info", "--format", "json"])
        .await
        .map_err(BackendError::from)?;
    check(out).map(|_| ())
}

impl PodmanMachineManager {
    async fn podman(&self, args: &[&str]) -> Result<Output, BackendError> {
        self.runner.run(args).await.map_err(BackendError::from)
    }
}

#[async_trait::async_trait]
impl MachineManager for PodmanMachineManager {
    async fn machine_is_running(&self) -> Result<bool, BackendError> {
        let out = self.podman(&["machine", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let machines: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        Ok(machines.iter().any(|m| {
            m["Name"].as_str() == Some(DEFAULT_MACHINE_NAME) && m["Running"].as_bool() == Some(true)
        }))
    }

    async fn check_other_machines_running(&self) -> bool {
        let Ok(out) = self.podman(&["machine", "list", "--format", "json"]).await else {
            return false;
        };
        let Ok(stdout) = check(out) else {
            return false;
        };
        let Ok(machines) = serde_json::from_str::<Vec<serde_json::Value>>(&stdout) else {
            return false;
        };
        machines.iter().any(|m| {
            m["Name"].as_str() != Some(DEFAULT_MACHINE_NAME) && m["Running"].as_bool() == Some(true)
        })
    }

    async fn run_machine_start(&self) -> Result<(), BackendError> {
        check(self.podman(&["machine", "start", DEFAULT_MACHINE_NAME]).await?).map(|_| ())
    }

    async fn run_wsl_shutdown(&self) -> Result<(), BackendError> {
        // A shutdown failure is non-fatal — the caller retries the machine start regardless.
        let _ = run_host_wsl(&["--shutdown"]).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        Ok(())
    }

    async fn activate_podman_socket(&self) -> Result<(), BackendError> {
        // A spawned child, so the scripted `ProcessRunner` seam every other podman call here routes
        // through cannot stand in for it: reaching it starts a service in the dev's real machine.
        if crate::backends::mock_backend_enabled() {
            return Ok(());
        }

        // Activates socket manually; setsid survives SSH SIGHUP; script as single argv for Windows.
        let script = format!(
            "test -S {sock} || {{ mkdir -p $(dirname {sock}) && setsid env XDG_RUNTIME_DIR=/run/user/1000 podman system service --time=0 </dev/null >/tmp/podman-svc.log 2>&1 & }}",
            sock = PODMAN_SOCKET,
        );
        let Ok(mut child) = podman_command()
            .args(["machine", "ssh", DEFAULT_MACHINE_NAME])
            .arg(&script)
            .spawn()
        else {
            return Ok(());
        };
        // Send SIGTERM, not SIGKILL, to avoid tearing down the backgrounded service.
        if tokio::time::timeout(std::time::Duration::from_secs(7), child.wait()).await.is_err() {
            let _ = child.start_kill();
        }
        Ok(())
    }

    async fn poll_ready(&self, attempts: usize) -> Result<(), BackendError> {
        let mut last = BackendError::CommandFailed("Podman socket is not ready".to_string());
        for attempt in 0..attempts {
            match verify_host(&self.runner, DEFAULT_MACHINE_NAME).await {
                Ok(()) => return Ok(()),
                Err(e) => last = e,
            }
            if attempt + 1 < attempts {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
        Err(last)
    }

    async fn wsl_is_ready(&self) -> bool {
        // `wsl --status` exits non-zero when WSL is not installed, and its stdout reports the
        // default version even when no distribution is installed yet (which is all Podman needs:
        // it brings its own Fedora CoreOS distribution via `machine init`).
        tokio::task::spawn_blocking(|| {
            detached_command_std("wsl").arg("--status").output()
        })
        .await
        .map(|r| r.map(|o| o.status.success()).unwrap_or(false))
        .unwrap_or(false)
    }

    async fn install_wsl(&self) -> Result<bool, BackendError> {
        // `--no-distribution` installs just the WSL2 platform (kernel + virtualization stack),
        // leaving distribution provisioning to `podman machine init`. A fresh install typically
        // needs a reboot before the kernel component is usable, so the reboot requirement is
        // assumed rather than probed — Windows gives no reliable signal short of trying and
        // failing, which would surface as a confusing `machine init` error instead.
        let Some(result) = run_host_wsl(&["--install", "--no-distribution"]).await else {
            // Mock backend: `false` is "installed, no reboot needed" — `true` would fail
            // `provision_podman` with `PodmanRebootRequired` (`src-tauri/src/lib.rs`).
            return Ok(false);
        };
        let out = result.map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(BackendError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(true)
    }

    async fn machine_exists(&self) -> Result<bool, BackendError> {
        let out = self.podman(&["machine", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let machines: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        Ok(machines.iter().any(|m| m["Name"].as_str() == Some(DEFAULT_MACHINE_NAME)))
    }

    async fn init_machine(&self) -> Result<(), BackendError> {
        check(self.podman(&["machine", "init", DEFAULT_MACHINE_NAME]).await?).map(|_| ())
    }

    async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
        // Best-effort: a machine stuck mid-import may not appear in `podman machine list` at all,
        // so its absence here is not an error.
        let _ = self.podman(&["machine", "rm", "-f", DEFAULT_MACHINE_NAME]).await;

        // The actual fix: `wsl --import` leaves a partially-registered distro behind on failure,
        // and podman refuses to retry `machine init` while it's still registered. `wsl
        // --unregister` also exits non-zero when the distro is already gone, which is fine.
        let _ = run_host_wsl(&["--unregister", DEFAULT_MACHINE_NAME]).await;
        Ok(())
    }
}

/// The two process seams [`readiness_provider_for`] picks between, managed once as Tauri state
/// (`src-tauri/src/lib.rs`). A plain `Arc<dyn ProcessRunner>` cannot be managed twice under two
/// different meanings, since Tauri state is keyed by type.
pub struct MachineRunners {
    pub podman: Arc<dyn ProcessRunner>,
    pub msb: Arc<dyn ProcessRunner>,
}

/// Construct the [`MachineReadinessProvider`] for `provider`, mirroring `inference::engine_for`'s
/// per-call construction (cheap: both managers are thin wrappers over an already-shared runner).
pub fn readiness_provider_for(
    provider: ProviderId,
    runners: &MachineRunners,
) -> Arc<dyn MachineReadinessProvider> {
    match provider {
        ProviderId::Podman => Arc::new(PodmanMachineManager { runner: runners.podman.clone() }),
        ProviderId::Microsandbox => Arc::new(msb::MsbMachineManager::new(runners.msb.clone())),
    }
}

#[async_trait::async_trait]
impl MachineReadinessProvider for PodmanMachineManager {
    fn setup_steps(&self) -> Vec<SetupStep> {
        ["wsl-install", "machine-init", "machine-start", "socket-activate"]
            .into_iter()
            .map(SetupStep::new)
            .collect()
    }

    async fn readiness(&self) -> MachineReadiness {
        let Ok(true) = self.machine_exists().await else {
            return MachineReadiness { ready: false, needs_setup: true };
        };
        let ready = matches!(self.machine_is_running().await, Ok(true)) && self.poll_ready(1).await.is_ok();
        MachineReadiness { ready, needs_setup: false }
    }

    /// Independent of `provision_podman`/`start_podman_machine` (`src-tauri/src/lib.rs`) by design,
    /// not oversight: those commands back the real-infra-verified recovery gate on
    /// `src/routes/projects/+page.svelte`, and this phase has no way to re-run
    /// `podman-startup-recovery.spec.ts` to prove a shared implementation is byte-identical. The
    /// duplication is recorded as a follow-up in `docs/features/microsandbox-provider/04_improvements.md`.
    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError> {
        MachineSetupEvent::StepStart { id: "wsl-install".into() }.send(on_step);
        if !self.wsl_is_ready().await {
            match self.install_wsl().await {
                Ok(true) => {
                    let msg = "Windows needs to restart to finish installing WSL2. Restart, then \
                               try again."
                        .to_string();
                    MachineSetupEvent::RebootRequired { message: msg.clone() }.send(on_step);
                    return Err(BackendError::CommandFailed(msg));
                }
                Ok(false) => {}
                Err(e) => {
                    MachineSetupEvent::StepFailed { id: "wsl-install".into(), error: e.to_string() }
                        .send(on_step);
                    return Err(e);
                }
            }
        }
        MachineSetupEvent::StepDone { id: "wsl-install".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "machine-init".into() }.send(on_step);
        match self.machine_exists().await {
            Ok(true) => {
                MachineSetupEvent::StepSkipped {
                    id: "machine-init".into(),
                    reason: "already exists".into(),
                }
                .send(on_step);
            }
            Ok(false) => {
                if let Err(e) = self.init_machine().await {
                    MachineSetupEvent::StepFailed { id: "machine-init".into(), error: e.to_string() }
                        .send(on_step);
                    return Err(e);
                }
                MachineSetupEvent::StepDone { id: "machine-init".into(), detail: None }.send(on_step);
            }
            Err(e) => {
                MachineSetupEvent::StepFailed { id: "machine-init".into(), error: e.to_string() }
                    .send(on_step);
                return Err(e);
            }
        }

        MachineSetupEvent::StepStart { id: "machine-start".into() }.send(on_step);
        if self.run_machine_start().await.is_err() {
            self.run_wsl_shutdown().await.ok();
            if let Err(e) = self.run_machine_start().await {
                MachineSetupEvent::StepFailed { id: "machine-start".into(), error: e.to_string() }
                    .send(on_step);
                return Err(e);
            }
        }
        MachineSetupEvent::StepDone { id: "machine-start".into(), detail: None }.send(on_step);

        MachineSetupEvent::StepStart { id: "socket-activate".into() }.send(on_step);
        self.activate_podman_socket().await.ok();
        if let Err(e) = self.poll_ready(4).await {
            MachineSetupEvent::StepFailed { id: "socket-activate".into(), error: e.to_string() }
                .send(on_step);
            return Err(e);
        }
        MachineSetupEvent::StepDone { id: "socket-activate".into(), detail: None }.send(on_step);

        MachineSetupEvent::Ready.send(on_step);
        Ok(())
    }
}

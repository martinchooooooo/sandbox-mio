//! Podman machine-lifecycle management: start default machine, reset WSL, activate rootless socket.
//! Separate from backends since it drives the whole machine rather than single connections.
//!
//! Targets [`machine_name()`], not `connection_name()`'s `PODMAN_CONNECTION`
//! (`src-tauri/src/backends/mod.rs`) — deliberately: a connection may name a remote SSH endpoint,
//! against which `podman machine start`/`wsl --unregister` are meaningless or destructive.

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

use types::{
    BringUpCause, BringUpEntry, BringUpEvent, BringUpFailure, BringUpNote, MachineManager,
    MachineReadiness, MachineReadinessProvider, MachineSetupEvent, PodmanMachineManager, SetupStep,
    STEP_MACHINE_INIT, STEP_MACHINE_START, STEP_SOCKET_ACTIVATE, STEP_WSL_INSTALL,
};

/// Rootless Podman API socket inside the default machine (UID 1000). Must be activated manually
/// via `podman system service` because the machine's user systemd session does not start on boot.
const PODMAN_SOCKET: &str = "/run/user/1000/podman/podman.sock";

/// `MIO_PODMAN_MACHINE` when set, else `DEFAULT_MACHINE_NAME`. Test-only — production never sets it.
fn machine_name() -> String {
    std::env::var("MIO_PODMAN_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE_NAME.to_string())
}

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
        let name = machine_name();
        Ok(machines.iter().any(|m| {
            m["Name"].as_str() == Some(name.as_str()) && m["Running"].as_bool() == Some(true)
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
        let name = machine_name();
        machines.iter().any(|m| {
            m["Name"].as_str() != Some(name.as_str()) && m["Running"].as_bool() == Some(true)
        })
    }

    async fn run_machine_start(&self) -> Result<(), BackendError> {
        let name = machine_name();
        check(self.podman(&["machine", "start", name.as_str()]).await?).map(|_| ())
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
        let name = machine_name();
        let Ok(mut child) = podman_command()
            .args(["machine", "ssh", name.as_str()])
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
        let name = machine_name();
        let mut last = BackendError::CommandFailed("Podman socket is not ready".to_string());
        for attempt in 0..attempts {
            match verify_host(&self.runner, &name).await {
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
            // `bring_up` with `BringUpCause::RebootRequired`.
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
        let name = machine_name();
        Ok(machines.iter().any(|m| m["Name"].as_str() == Some(name.as_str())))
    }

    async fn init_machine(&self) -> Result<(), BackendError> {
        let name = machine_name();
        check(self.podman(&["machine", "init", name.as_str()]).await?).map(|_| ())
    }

    async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
        let name = machine_name();

        // Best-effort: a machine stuck mid-import may not appear in `podman machine list` at all,
        // so its absence here is not an error.
        let _ = self.podman(&["machine", "rm", "-f", name.as_str()]).await;

        // The actual fix: `wsl --import` leaves a partially-registered distro behind on failure,
        // and podman refuses to retry `machine init` while it's still registered. `wsl
        // --unregister` also exits non-zero when the distro is already gone, which is fine.
        let _ = run_host_wsl(&["--unregister", name.as_str()]).await;
        Ok(())
    }
}

/// Shown for [`BringUpCause::RebootRequired`] by both the welcome flow and the recovery gate.
const WSL_REBOOT_MESSAGE: &str =
    "WSL2 was installed and needs a restart to finish. Restart Windows, then try again.";

/// The one Podman bring-up sequence behind the welcome flow's `prepare()` and the projects-page
/// recovery commands (`src-tauri/src/commands/machine.rs`): WSL check and install, machine init,
/// guarded start, socket activation, readiness poll.
///
/// Both start guards are load-bearing (`docs/features/techdebt-sweep/02_dev.md`, Resolved Decision
/// 6): never `podman machine start` a running machine, and never `wsl --shutdown` while another
/// machine runs, since that stops every WSL distribution on the host.
pub async fn bring_up<F>(
    machine: &dyn MachineManager,
    entry: BringUpEntry,
    mut on_event: F,
) -> Result<(), BringUpFailure>
where
    F: FnMut(BringUpEvent) + Send,
{
    if entry == BringUpEntry::Full {
        wsl_step(machine, &mut on_event).await?;
        init_step(machine, &mut on_event).await?;
    }
    start_step(machine, &mut on_event).await?;
    socket_step(machine, &mut on_event).await
}

fn failed(step: &'static str, e: BackendError) -> BringUpFailure {
    BringUpFailure { step, cause: BringUpCause::Failed(e) }
}

async fn wsl_step<F: FnMut(BringUpEvent) + Send>(
    machine: &dyn MachineManager,
    on_event: &mut F,
) -> Result<(), BringUpFailure> {
    on_event(BringUpEvent::StepStart(STEP_WSL_INSTALL));
    if !machine.wsl_is_ready().await {
        on_event(BringUpEvent::Note(BringUpNote::InstallingWsl));
        match machine.install_wsl().await {
            Ok(true) => {
                return Err(BringUpFailure {
                    step: STEP_WSL_INSTALL,
                    cause: BringUpCause::RebootRequired(WSL_REBOOT_MESSAGE.to_string()),
                });
            }
            Ok(false) => {}
            Err(e) => return Err(failed(STEP_WSL_INSTALL, e)),
        }
    }
    on_event(BringUpEvent::StepDone(STEP_WSL_INSTALL));
    Ok(())
}

async fn init_step<F: FnMut(BringUpEvent) + Send>(
    machine: &dyn MachineManager,
    on_event: &mut F,
) -> Result<(), BringUpFailure> {
    on_event(BringUpEvent::StepStart(STEP_MACHINE_INIT));
    if machine.machine_exists().await.map_err(|e| failed(STEP_MACHINE_INIT, e))? {
        on_event(BringUpEvent::StepSkipped { id: STEP_MACHINE_INIT, reason: "already exists" });
        return Ok(());
    }
    on_event(BringUpEvent::Note(BringUpNote::CreatingMachine));
    machine.init_machine().await.map_err(|e| failed(STEP_MACHINE_INIT, e))?;
    on_event(BringUpEvent::StepDone(STEP_MACHINE_INIT));
    Ok(())
}

async fn start_step<F: FnMut(BringUpEvent) + Send>(
    machine: &dyn MachineManager,
    on_event: &mut F,
) -> Result<(), BringUpFailure> {
    on_event(BringUpEvent::StepStart(STEP_MACHINE_START));
    // `podman machine start` on a running machine exits non-zero ("already running"), which the
    // retry ladder below would misread as a real failure and answer with `wsl --shutdown`.
    if matches!(machine.machine_is_running().await, Ok(true)) {
        on_event(BringUpEvent::StepSkipped { id: STEP_MACHINE_START, reason: "already running" });
        return Ok(());
    }
    if machine.run_machine_start().await.is_err() {
        if machine.check_other_machines_running().await {
            on_event(BringUpEvent::Note(BringUpNote::RetryingStart));
        } else {
            on_event(BringUpEvent::Note(BringUpNote::RetryingAfterWslReset));
            machine.run_wsl_shutdown().await.ok();
        }
        machine.run_machine_start().await.map_err(|e| failed(STEP_MACHINE_START, e))?;
    }
    on_event(BringUpEvent::StepDone(STEP_MACHINE_START));
    Ok(())
}

async fn socket_step<F: FnMut(BringUpEvent) + Send>(
    machine: &dyn MachineManager,
    on_event: &mut F,
) -> Result<(), BringUpFailure> {
    on_event(BringUpEvent::StepStart(STEP_SOCKET_ACTIVATE));
    machine.activate_podman_socket().await.ok();
    on_event(BringUpEvent::Note(BringUpNote::WaitingForSocket));
    machine.poll_ready(4).await.map_err(|e| failed(STEP_SOCKET_ACTIVATE, e))?;
    on_event(BringUpEvent::StepDone(STEP_SOCKET_ACTIVATE));
    Ok(())
}

/// The two process seams [`readiness_provider_for`] picks between, managed once as Tauri state
/// (`src-tauri/src/bootstrap.rs`). A plain `Arc<dyn ProcessRunner>` cannot be managed twice under two
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
        [STEP_WSL_INSTALL, STEP_MACHINE_INIT, STEP_MACHINE_START, STEP_SOCKET_ACTIVATE]
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

    async fn prepare(&self, on_step: &Channel<serde_json::Value>) -> Result<(), BackendError> {
        let result = bring_up(self, BringUpEntry::Full, |event| {
            let message = match event {
                BringUpEvent::StepStart(id) => Some(MachineSetupEvent::StepStart { id: id.into() }),
                BringUpEvent::StepDone(id) => {
                    Some(MachineSetupEvent::StepDone { id: id.into(), detail: None })
                }
                BringUpEvent::StepSkipped { id, reason } => {
                    Some(MachineSetupEvent::StepSkipped { id: id.into(), reason: reason.into() })
                }
                BringUpEvent::Note(_) => None,
            };
            if let Some(message) = message {
                message.send(on_step);
            }
        })
        .await;
        match result {
            Ok(()) => {
                MachineSetupEvent::Ready.send(on_step);
                Ok(())
            }
            Err(BringUpFailure { cause: BringUpCause::RebootRequired(message), .. }) => {
                MachineSetupEvent::RebootRequired { message: message.clone() }.send(on_step);
                Err(BackendError::CommandFailed(message))
            }
            Err(BringUpFailure { step, cause: BringUpCause::Failed(e) }) => {
                MachineSetupEvent::StepFailed { id: step.into(), error: e.to_string() }
                    .send(on_step);
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    /// Records every call and answers from fixed fields; `start_results` is consumed one per
    /// `run_machine_start`, `true` meaning success.
    struct FakeMachine {
        calls: Mutex<Vec<&'static str>>,
        wsl_ready: bool,
        install_needs_reboot: bool,
        exists: bool,
        running: bool,
        other_running: bool,
        start_results: Mutex<VecDeque<bool>>,
        socket_ready: bool,
    }

    impl FakeMachine {
        fn new() -> Self {
            FakeMachine {
                calls: Mutex::new(Vec::new()),
                wsl_ready: true,
                install_needs_reboot: false,
                exists: true,
                running: false,
                other_running: false,
                start_results: Mutex::new(VecDeque::from([true])),
                socket_ready: true,
            }
        }

        fn record(&self, call: &'static str) {
            self.calls.lock().unwrap().push(call);
        }

        fn count(&self, call: &str) -> usize {
            self.calls.lock().unwrap().iter().filter(|c| **c == call).count()
        }
    }

    fn err() -> BackendError {
        BackendError::CommandFailed("boom".to_string())
    }

    #[async_trait::async_trait]
    impl MachineManager for FakeMachine {
        async fn machine_is_running(&self) -> Result<bool, BackendError> {
            self.record("machine_is_running");
            Ok(self.running)
        }
        async fn check_other_machines_running(&self) -> bool {
            self.record("check_other_machines_running");
            self.other_running
        }
        async fn run_machine_start(&self) -> Result<(), BackendError> {
            self.record("run_machine_start");
            if self.start_results.lock().unwrap().pop_front().unwrap_or(false) {
                Ok(())
            } else {
                Err(err())
            }
        }
        async fn run_wsl_shutdown(&self) -> Result<(), BackendError> {
            self.record("run_wsl_shutdown");
            Ok(())
        }
        async fn activate_podman_socket(&self) -> Result<(), BackendError> {
            self.record("activate_podman_socket");
            Ok(())
        }
        async fn poll_ready(&self, _attempts: usize) -> Result<(), BackendError> {
            self.record("poll_ready");
            if self.socket_ready { Ok(()) } else { Err(err()) }
        }
        async fn wsl_is_ready(&self) -> bool {
            self.record("wsl_is_ready");
            self.wsl_ready
        }
        async fn install_wsl(&self) -> Result<bool, BackendError> {
            self.record("install_wsl");
            Ok(self.install_needs_reboot)
        }
        async fn machine_exists(&self) -> Result<bool, BackendError> {
            self.record("machine_exists");
            Ok(self.exists)
        }
        async fn init_machine(&self) -> Result<(), BackendError> {
            self.record("init_machine");
            Ok(())
        }
        async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
            self.record("reset_stuck_machine");
            Ok(())
        }
    }

    async fn run(
        machine: &FakeMachine,
        entry: BringUpEntry,
    ) -> (Result<(), BringUpFailure>, Vec<BringUpEvent>) {
        let mut events = Vec::new();
        let result = bring_up(machine, entry, |e| events.push(e)).await;
        (result, events)
    }

    #[tokio::test]
    async fn running_machine_is_not_started_again() {
        let mut machine = FakeMachine::new();
        machine.running = true;
        let (result, events) = run(&machine, BringUpEntry::Full).await;
        assert!(result.is_ok());
        assert_eq!(machine.count("run_machine_start"), 0);
        assert_eq!(machine.count("run_wsl_shutdown"), 0);
        assert!(events.contains(&BringUpEvent::StepSkipped {
            id: STEP_MACHINE_START,
            reason: "already running"
        }));
        assert_eq!(machine.count("activate_podman_socket"), 1);
    }

    #[tokio::test]
    async fn failed_start_with_other_machines_running_skips_wsl_shutdown() {
        let mut machine = FakeMachine::new();
        machine.other_running = true;
        machine.start_results = Mutex::new(VecDeque::from([false, true]));
        let (result, events) = run(&machine, BringUpEntry::FromStart).await;
        assert!(result.is_ok());
        assert_eq!(machine.count("run_machine_start"), 2);
        assert_eq!(machine.count("run_wsl_shutdown"), 0);
        assert!(events.contains(&BringUpEvent::Note(BringUpNote::RetryingStart)));
    }

    #[tokio::test]
    async fn failed_start_with_no_other_machines_resets_wsl_once() {
        let mut machine = FakeMachine::new();
        machine.start_results = Mutex::new(VecDeque::from([false, true]));
        let (result, events) = run(&machine, BringUpEntry::FromStart).await;
        assert!(result.is_ok());
        assert_eq!(machine.count("run_wsl_shutdown"), 1);
        assert!(events.contains(&BringUpEvent::Note(BringUpNote::RetryingAfterWslReset)));
    }

    #[tokio::test]
    async fn second_start_failure_names_the_start_step() {
        let mut machine = FakeMachine::new();
        machine.start_results = Mutex::new(VecDeque::from([false, false]));
        let (result, _) = run(&machine, BringUpEntry::FromStart).await;
        let failure = result.unwrap_err();
        assert_eq!(failure.step, STEP_MACHINE_START);
        assert!(matches!(failure.cause, BringUpCause::Failed(_)));
        assert_eq!(machine.count("poll_ready"), 0);
    }

    #[tokio::test]
    async fn from_start_entry_skips_wsl_and_init() {
        let machine = FakeMachine::new();
        let (result, events) = run(&machine, BringUpEntry::FromStart).await;
        assert!(result.is_ok());
        assert_eq!(machine.count("wsl_is_ready"), 0);
        assert_eq!(machine.count("machine_exists"), 0);
        assert_eq!(events.first(), Some(&BringUpEvent::StepStart(STEP_MACHINE_START)));
    }

    #[tokio::test]
    async fn full_entry_inits_a_missing_machine() {
        let mut machine = FakeMachine::new();
        machine.exists = false;
        let (result, events) = run(&machine, BringUpEntry::Full).await;
        assert!(result.is_ok());
        assert_eq!(machine.count("init_machine"), 1);
        assert!(events.contains(&BringUpEvent::StepDone(STEP_MACHINE_INIT)));
    }

    #[tokio::test]
    async fn wsl_install_needing_reboot_stops_before_init() {
        let mut machine = FakeMachine::new();
        machine.wsl_ready = false;
        machine.install_needs_reboot = true;
        let (result, _) = run(&machine, BringUpEntry::Full).await;
        let failure = result.unwrap_err();
        assert_eq!(failure.step, STEP_WSL_INSTALL);
        assert!(matches!(failure.cause, BringUpCause::RebootRequired(_)));
        assert_eq!(machine.count("machine_exists"), 0);
    }

    #[tokio::test]
    async fn dead_socket_fails_the_socket_step() {
        let mut machine = FakeMachine::new();
        machine.socket_ready = false;
        let (result, _) = run(&machine, BringUpEntry::FromStart).await;
        assert_eq!(result.unwrap_err().step, STEP_SOCKET_ACTIVATE);
    }
}

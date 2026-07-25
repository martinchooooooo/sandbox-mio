//! Podman machine-lifecycle management: start default machine, reset WSL, activate rootless socket.
//! Separate from backends since it drives the whole machine rather than single connections.

pub mod types;

use std::process::Output;
use std::sync::Arc;

use crate::backends::types::BackendError;
use crate::backends::{podman_command, DEFAULT_CONNECTION};
use crate::mockctl::types::ProcessRunner;
use crate::process_ext::NoWindow;

use types::{MachineManager, PodmanMachineManager};

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
        std::process::Command::new("wsl").no_window().args(&owned).output()
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
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    check(out).map(|_| ())
}

impl PodmanMachineManager {
    async fn podman(&self, args: &[&str]) -> Result<Output, BackendError> {
        self.runner.run(args).await.map_err(|e| BackendError::CommandFailed(e.to_string()))
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
            m["Name"].as_str() == Some(DEFAULT_CONNECTION) && m["Running"].as_bool() == Some(true)
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
            m["Name"].as_str() != Some(DEFAULT_CONNECTION) && m["Running"].as_bool() == Some(true)
        })
    }

    async fn run_machine_start(&self) -> Result<(), BackendError> {
        check(self.podman(&["machine", "start", DEFAULT_CONNECTION]).await?).map(|_| ())
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
            .args(["machine", "ssh", DEFAULT_CONNECTION])
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
            match verify_host(&self.runner, DEFAULT_CONNECTION).await {
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
            std::process::Command::new("wsl").no_window().arg("--status").output()
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
        Ok(machines.iter().any(|m| m["Name"].as_str() == Some(DEFAULT_CONNECTION)))
    }

    async fn init_machine(&self) -> Result<(), BackendError> {
        check(self.podman(&["machine", "init", DEFAULT_CONNECTION]).await?).map(|_| ())
    }

    async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
        // Best-effort: a machine stuck mid-import may not appear in `podman machine list` at all,
        // so its absence here is not an error.
        let _ = self.podman(&["machine", "rm", "-f", DEFAULT_CONNECTION]).await;

        // The actual fix: `wsl --import` leaves a partially-registered distro behind on failure,
        // and podman refuses to retry `machine init` while it's still registered. `wsl
        // --unregister` also exits non-zero when the distro is already gone, which is fine.
        let _ = run_host_wsl(&["--unregister", DEFAULT_CONNECTION]).await;
        Ok(())
    }
}

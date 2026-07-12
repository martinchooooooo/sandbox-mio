//! Global Podman machine-lifecycle management (start the default machine, reset WSL, activate the
//! rootless socket, poll for readiness), separate from [`crate::backends`] because it drives the
//! whole default machine rather than a single named connection. The [`MachineManager`] trait has a
//! real CLI-backed impl here and an in-memory [`mock`] selected once by [`default_machine_manager`].
//!
//! Used by the `check_podman_ready` and `start_podman_machine` commands in `lib.rs`, which receive
//! an `Arc<dyn MachineManager>` from Tauri state.

pub mod types;

mod mock;

use std::process::Output;

use crate::backends::types::BackendError;
use crate::backends::{mock_backend_enabled, podman_command, DEFAULT_CONNECTION};
use crate::process_ext::NoWindow;

use mock::MockMachineManager;
use types::{MachineManager, PodmanMachineManager};

/// Rootless Podman API socket inside the default machine (UID 1000). Must be activated manually
/// via `podman system service` because the machine's user systemd session does not start on boot.
const PODMAN_SOCKET: &str = "/run/user/1000/podman/podman.sock";

/// Construct the managed [`MachineManager`] for Tauri state: [`MockMachineManager`] when
/// `MIO_MOCK_BACKEND=1`, otherwise the real [`PodmanMachineManager`]. The single place that flag
/// steers machine-lifecycle behavior, mirroring `default_backend`/`default_runtime`.
pub fn default_machine_manager() -> std::sync::Arc<dyn MachineManager> {
    if mock_backend_enabled() {
        return std::sync::Arc::new(MockMachineManager);
    }
    std::sync::Arc::new(PodmanMachineManager)
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

/// Runs `podman <args>` without `--connection`, for machine-lifecycle and system-registry
/// operations that are not bound to any single connection.
async fn podman(args: &[&str]) -> Result<Output, BackendError> {
    podman_command()
        .args(args)
        .output()
        .await
        .map_err(|e| BackendError::CommandFailed(e.to_string()))
}

/// Probe a connection's live socket with `podman --connection <name> info`. Success means the
/// rootless API answers — the readiness signal `poll_ready` polls for.
async fn verify_host(name: &str) -> Result<(), BackendError> {
    check(podman(&["--connection", name, "info", "--format", "json"]).await?).map(|_| ())
}

#[async_trait::async_trait]
impl MachineManager for PodmanMachineManager {
    async fn machine_is_running(&self) -> Result<bool, BackendError> {
        let out = podman(&["machine", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let machines: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        Ok(machines.iter().any(|m| {
            m["Name"].as_str() == Some(DEFAULT_CONNECTION) && m["Running"].as_bool() == Some(true)
        }))
    }

    async fn check_other_machines_running(&self) -> bool {
        let Ok(out) = podman(&["machine", "list", "--format", "json"]).await else {
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
        check(podman(&["machine", "start", DEFAULT_CONNECTION]).await?).map(|_| ())
    }

    async fn run_wsl_shutdown(&self) -> Result<(), BackendError> {
        // Run on the blocking pool (synchronous `std::process::Command`). A shutdown failure is
        // non-fatal — the caller retries the machine start regardless of the exit code.
        let _ = tokio::task::spawn_blocking(|| {
            std::process::Command::new("wsl").no_window().arg("--shutdown").output()
        })
        .await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        Ok(())
    }

    async fn activate_podman_socket(&self) -> Result<(), BackendError> {
        // The machine's per-user systemd manager does not start on boot, so the socket must be
        // activated by hand; `setsid` detaches the service so it survives the SIGHUP when the SSH
        // channel closes. `podman machine ssh` allocates a pty and blocks until its timeout even
        // after the service backgrounds, so the call is capped at 7 s and the SSH client is then
        // asked to stop.
        //
        // The script is passed as one argv element (shell `false`): routing through `cmd.exe` on
        // Windows would let it shred the `||`, `&`, `>`, and `<` metacharacters before they reach
        // the remote bash (see `e2e/lib/podman-machine.ts:87`).
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
        // SIGTERM (start_kill), not SIGKILL: the latter tears down the pty abruptly and can take
        // the newly backgrounded service with it. `tokio::time::timeout` only cancels the future,
        // so the OS process must be stopped explicitly.
        if tokio::time::timeout(std::time::Duration::from_secs(7), child.wait()).await.is_err() {
            let _ = child.start_kill();
        }
        Ok(())
    }

    async fn poll_ready(&self, attempts: usize) -> Result<(), BackendError> {
        let mut last = BackendError::CommandFailed("Podman socket is not ready".to_string());
        for attempt in 0..attempts {
            match verify_host(DEFAULT_CONNECTION).await {
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
        let out = tokio::task::spawn_blocking(|| {
            std::process::Command::new("wsl")
                .no_window()
                .args(["--install", "--no-distribution"])
                .output()
        })
        .await
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        if !out.status.success() {
            return Err(BackendError::CommandFailed(
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(true)
    }

    async fn machine_exists(&self) -> Result<bool, BackendError> {
        let out = podman(&["machine", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let machines: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        Ok(machines.iter().any(|m| m["Name"].as_str() == Some(DEFAULT_CONNECTION)))
    }

    async fn init_machine(&self) -> Result<(), BackendError> {
        check(podman(&["machine", "init", DEFAULT_CONNECTION]).await?).map(|_| ())
    }

    async fn reset_stuck_machine(&self) -> Result<(), BackendError> {
        // Best-effort: a machine stuck mid-import may not appear in `podman machine list` at all,
        // so its absence here is not an error.
        let _ = podman(&["machine", "rm", "-f", DEFAULT_CONNECTION]).await;

        // The actual fix: `wsl --import` leaves a partially-registered distro behind on failure,
        // and podman refuses to retry `machine init` while it's still registered. `wsl
        // --unregister` also exits non-zero when the distro is already gone, which is fine.
        let _ = tokio::task::spawn_blocking(|| {
            std::process::Command::new("wsl")
                .no_window()
                .args(["--unregister", DEFAULT_CONNECTION])
                .output()
        })
        .await;
        Ok(())
    }
}

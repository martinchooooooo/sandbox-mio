//! Where the bundled Mutagen sidecar is and how it is isolated from any Mutagen the user runs, and
//! [`SystemMutagenRunner`], the real runner over it.

use std::process::Output;
use std::time::Duration;

use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

use super::is_monitor_call;
use super::types::ProcessRunner;

/// Why the sidecar command could not be built.
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    /// Creating or resolving the app-private Mutagen data directory failed.
    #[error("failed to create Mutagen data directory: {0}")]
    DataDir(String),
    /// The bundled Mutagen sidecar could not be resolved.
    #[error("sidecar error: {0}")]
    Sidecar(String),
}

/// Sidecar name as declared in `bundle.externalBin`.
const SIDECAR: &str = "mutagen";

/// `MIO_APP_DATA_DIR` relocates the Mutagen data directory (see [`crate::paths::app_data_root`]);
/// no subsystem-specific override exists for this one.
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, SidecarError> {
    let dir = crate::paths::app_data_root(app)
        .map_err(|e| SidecarError::DataDir(e.to_string()))?
        .join("mutagen");
    std::fs::create_dir_all(&dir)
        .map_err(|e| SidecarError::DataDir(format!("failed to create mutagen data dir: {e}")))?;
    Ok(dir)
}

/// The bundled Mutagen sidecar, resolved through `tauri_plugin_shell` and then re-detached.
///
/// The plugin's own `Command` hard-codes `CREATE_NO_WINDOW` and piped stdio with no hook to override
/// either, so the resolved sidecar is converted back to a `std::process::Command` and put through
/// [`super::detach`] — otherwise this one binary would keep the inherited stdin every other
/// spawn site drops, and would state its console contract somewhere other than
/// `src-tauri/src/process/mod.rs` (`docs/features/podman-process-isolation/02_dev.md`, Phase B).
pub fn command<R: Runtime>(app: &AppHandle<R>) -> Result<tokio::process::Command, SidecarError> {
    let dir = data_dir(app)?;
    let sidecar: Command = app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| SidecarError::Sidecar(e.to_string()))?;
    let mut cmd = super::detach(sidecar.into());
    cmd.env("MUTAGEN_DATA_DIRECTORY", dir.to_string_lossy().to_string())
        // Defense-in-depth against MSYS/Git-Bash path mangling in the `ssh` → `sh -c "exec …"`
        // transport Mutagen spawns for a `Proxied` alias. NOT load-bearing for the exec transport:
        // `ssh::identity::ensure_sandbox_alias` references `mio-sshd` by bare name precisely so it
        // carries no manglable token, because these vars only reach the daemon that spawns the
        // transport when this CLI call auto-starts it (docs/features/podman-exec-transport/02_dev.md,
        // Resolved Decision 5). Kept for any future manglable argument on the sidecar's own command line.
        .env("MSYS_NO_PATHCONV", "1")
        .env("MSYS2_ARG_CONV_EXCL", "*");
    Ok(cmd)
}

/// Real Mutagen sidecar invocations: wraps [`command`], preserving its `MUTAGEN_DATA_DIRECTORY`
/// isolation. Holds the [`tauri::AppHandle`] because the sidecar is resolved through it, which is why
/// this is a separate runner from [`super::podman::SystemPodmanRunner`].
pub struct SystemMutagenRunner {
    app: AppHandle,
}

impl SystemMutagenRunner {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

/// Bound on every Mutagen one-shot call through [`SystemMutagenRunner`].
const MUTAGEN_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Logs one finished Mutagen call in the shape of the podman seam's lines. Only argv is logged;
/// the child environment must never reach the log.
macro_rules! log_call_outcome {
    ($tool:literal, $args:expr, $started:expr, $result:expr) => {{
        let argv = $args.join(" ");
        let ms = $started.elapsed().as_millis();
        match $result {
            Ok(out) if out.status.success() && is_monitor_call($args) => {
                tracing::debug!(target: $tool, argv = %argv, ms, concat!($tool, " call ok"))
            }
            Ok(out) if out.status.success() => {
                tracing::info!(target: $tool, argv = %argv, ms, concat!($tool, " call ok"))
            }
            Ok(out) => tracing::warn!(
                target: $tool,
                argv = %argv,
                ms,
                code = out.status.code().unwrap_or(-1),
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                concat!($tool, " call exited non-zero")
            ),
            Err(e) => {
                tracing::error!(target: $tool, argv = %argv, ms, error = %e, concat!($tool, " call failed"))
            }
        }
    }};
}

#[async_trait::async_trait]
impl ProcessRunner for SystemMutagenRunner {
    async fn run(&self, args: &[&str]) -> std::io::Result<Output> {
        // Sidecar-resolution and data-dir errors collapse into one io::Error here; the caller
        // (`sync::run_via`) re-wraps it as `SyncError::Sidecar`, losing the `DataDir` distinction.
        let mut command = command(&self.app).map_err(std::io::Error::other)?;
        let started = std::time::Instant::now();
        tracing::info!(target: "mutagen", argv = %args.join(" "), "mutagen call starting");
        // `kill_on_drop` makes the timeout drop terminate the direct child; the Mutagen daemon a
        // call may have auto-started is a separate process and keeps running.
        let call = command.args(args).kill_on_drop(true).output();
        let result = match tokio::time::timeout(MUTAGEN_CALL_TIMEOUT, call).await {
            Ok(output) => output.map_err(std::io::Error::other),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("mutagen {} timed out after {MUTAGEN_CALL_TIMEOUT:?}", args.join(" ")),
            )),
        };
        log_call_outcome!("mutagen", args, started, &result);
        result
    }
}

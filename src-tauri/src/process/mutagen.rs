//! Where the bundled Mutagen sidecar is and how it is isolated from any Mutagen the user runs.

use tauri::{AppHandle, Runtime};
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

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

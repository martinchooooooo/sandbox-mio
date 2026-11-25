//! Where the `podman` binary is and which connection it targets: the bundled client when the app
//! was started with a resource directory, else whatever `podman` the `PATH` resolves.

use tokio::process::Command;

use super::{detached_command, detached_command_std};

/// The connection `podman machine init` creates for the local machine. Renaming this const (not its
/// value) breaks the regex parse in `e2e/lib/podman-machine.ts`, which mirrors it as `DEFAULT_MACHINE`.
pub(crate) const DEFAULT_MACHINE_NAME: &str = "podman-machine-default";

/// The Podman connection name container and runtime operations target: `PODMAN_CONNECTION` when set
/// (e2e points this at a non-default machine, e.g. `mio-tests`), else `DEFAULT_MACHINE_NAME`.
///
/// Does not steer machine lifecycle (`src-tauri/src/machine/mod.rs`, which has its own
/// `MIO_PODMAN_MACHINE` override): a connection name isn't guaranteed to name a local machine — see
/// `SandboxBackend::add_host` (`src-tauri/src/backends/hosts.rs`) — so `podman machine` subcommands
/// can't safely run against it.
pub(crate) fn connection_name() -> String {
    std::env::var("PODMAN_CONNECTION").unwrap_or_else(|_| DEFAULT_MACHINE_NAME.to_string())
}

static PODMAN_RESOURCE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Record the bundled Podman resource directory for [`podman_command`] to use. Called exactly
/// once, at startup, before any subsystem shells out to podman.
pub fn set_podman_resource_dir(dir: std::path::PathBuf) {
    let _ = PODMAN_RESOURCE_DIR.set(dir);
}

pub(crate) fn podman_command() -> Command {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let mut cmd = detached_command(dir.join("podman.exe"));
            cmd.env("CONTAINERS_HELPER_BINARY_DIR", dir);
            cmd
        }
        None => detached_command("podman"),
    }
}

/// [`podman_command`]'s `std::process` twin, for callers that need a blocking `Child` rather than
/// `tokio::process`'s async one (`mockctl::run_via_blocking_process`) — same binary resolution, same
/// `CONTAINERS_HELPER_BINARY_DIR`, same detachment.
pub(crate) fn podman_command_std() -> std::process::Command {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let mut cmd = detached_command_std(dir.join("podman.exe"));
            cmd.env("CONTAINERS_HELPER_BINARY_DIR", dir);
            cmd
        }
        None => detached_command_std("podman"),
    }
}

/// The literal argv token for the bundled `podman.exe`, for embedding in an OpenSSH `ProxyCommand`
/// string — [`podman_command`] returns a `tokio::process::Command`, which cannot be. Forward-slashed
/// deliberately (`docs/features/podman-exec-transport/02_dev.md`, Resolved Decision 5).
pub(crate) fn podman_literal_path() -> String {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let path = dir.join("podman.exe").to_string_lossy().replace('\\', "/");
            format!("\"{path}\"")
        }
        None => "podman".to_string(),
    }
}

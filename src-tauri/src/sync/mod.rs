//! Mutagen file-sync engine, bundled as a Tauri sidecar.
//!
//! Mutagen ships as one self-contained binary that the app bundles via `externalBin` (see
//! `tauri.conf.json`) and invokes through the shell plugin. Tauri resolves the platform binary
//! (`mutagen-<target-triple>`) at runtime, so installs need no separately-installed Mutagen.
//!
//! Every invocation routes through [`command`], which pins `MUTAGEN_DATA_DIRECTORY` to an
//! app-private directory. Mutagen is daemon-based — the CLI is a thin client over a background
//! daemon that owns the sync sessions — and isolating the data directory keeps the app's daemon
//! and sessions from colliding with a Mutagen the user installed themselves.

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_shell::process::Command;
use tauri_plugin_shell::ShellExt;

/// Sidecar name as declared in `bundle.externalBin`.
const SIDECAR: &str = "mutagen";

/// App-private Mutagen state directory, created if absent. Isolating it from the default
/// data directory is what prevents the app's daemon and sessions clashing with a
/// user-installed Mutagen.
fn data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("mutagen");
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create mutagen data dir: {e}"))?;
    Ok(dir)
}

/// A Mutagen sidecar command with the app-private data directory applied. Callers append the
/// subcommand args and await `.output()`. The daemon auto-starts on the first session command,
/// so no explicit `daemon start` is needed.
pub fn command<R: Runtime>(app: &AppHandle<R>) -> Result<Command, String> {
    let dir = data_dir(app)?;
    Ok(app
        .shell()
        .sidecar(SIDECAR)
        .map_err(|e| e.to_string())?
        .env("MUTAGEN_DATA_DIRECTORY", dir.to_string_lossy().to_string()))
}

/// Run a Mutagen subcommand and return trimmed stdout, or the captured stderr as `Err`.
async fn run<R: Runtime>(app: &AppHandle<R>, args: &[&str]) -> Result<String, String> {
    let output = command(app)?
        .args(args)
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Report the bundled Mutagen version. A smoke test that the sidecar is bundled, resolvable,
/// and executable on this platform.
pub async fn version<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    run(app, &["version"]).await
}

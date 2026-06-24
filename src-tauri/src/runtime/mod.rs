//! Flox-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

use std::process::Output;

use tokio::process::Command;

use types::{PodmanFloxRuntime, RuntimeError, RuntimeManager};

/// Workspace bind-mount target inside every container. Matches `WORKSPACE_MOUNT` in `backends/mod.rs`.
const WORKSPACE_MOUNT: &str = "/root/work";

/// Path of the flox manifest relative to the workspace directory.
const MANIFEST_REL: &str = ".flox/env/manifest.toml";

/// Construct the managed [`RuntimeManager`] for Tauri state. The Podman connection is read from
/// `PODMAN_CONNECTION`; defaults to [`crate::backends::DEFAULT_CONNECTION`] when unset. The e2e
/// harness overrides `PODMAN_CONNECTION` to pin the local test connection.
pub fn default_runtime() -> std::sync::Arc<dyn RuntimeManager> {
    let connection = std::env::var("PODMAN_CONNECTION")
        .unwrap_or_else(|_| crate::backends::DEFAULT_CONNECTION.to_string());
    std::sync::Arc::new(PodmanFloxRuntime { connection })
}

fn check(out: Output) -> Result<String, RuntimeError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(RuntimeError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

fn validate_package(package: &str) -> Result<(), RuntimeError> {
    let ok = !package.is_empty()
        && package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '#' | ':' | '@' | '+'));
    if ok {
        Ok(())
    } else {
        Err(RuntimeError::InvalidPackage { name: package.to_string() })
    }
}

fn parse_installed(manifest: &str) -> Result<Vec<String>, RuntimeError> {
    let value: toml::Value =
        toml::from_str(manifest).map_err(|e| RuntimeError::ManifestParse(e.to_string()))?;
    let Some(install) = value.get("install").and_then(|v| v.as_table()) else {
        return Ok(Vec::new());
    };
    Ok(install.keys().cloned().collect())
}

impl PodmanFloxRuntime {
    async fn podman(&self, args: &[&str]) -> Result<Output, RuntimeError> {
        Self::podman_on(&self.connection, args).await
    }

    async fn podman_on(connection: &str, args: &[&str]) -> Result<Output, RuntimeError> {
        let mut full = vec!["--connection", connection];
        full.extend_from_slice(args);
        Command::new("podman")
            .args(&full)
            .output()
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))
    }

    async fn flox_exec(&self, name: &str, flox_args: &[&str]) -> Result<String, RuntimeError> {
        check(self.podman(&["start", name]).await?)?;
        let mut args = vec!["exec", "--workdir", WORKSPACE_MOUNT, name, "flox"];
        args.extend_from_slice(flox_args);
        check(self.podman(&args).await?)
    }
}

#[async_trait::async_trait]
impl RuntimeManager for PodmanFloxRuntime {
    async fn init(&self, image: &str, workspace_host_path: &str) -> Result<(), RuntimeError> {
        let flox_dir = std::path::Path::new(workspace_host_path).join(".flox");
        if flox_dir.exists() {
            return Ok(());
        }
        // Throwaway container so the sandbox container's state stays untouched; the bind-mount
        // lands `.flox/` directly in the workspace directory on the host.
        let volume = format!("{workspace_host_path}:{WORKSPACE_MOUNT}");
        check(
            self.podman(&[
                "run", "--rm", "--volume", &volume, "--workdir", WORKSPACE_MOUNT, image, "flox",
                "init",
            ])
            .await?,
        )
        .map(|_| ())
    }

    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        validate_package(package)?;
        self.flox_exec(name, &["install", package]).await.map(|_| ())
    }

    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        validate_package(package)?;
        self.flox_exec(name, &["uninstall", package]).await.map(|_| ())
    }

    async fn list(&self, workspace_host_path: &str) -> Result<Vec<String>, RuntimeError> {
        let manifest_path = std::path::Path::new(workspace_host_path).join(MANIFEST_REL);
        // Absent manifest (env never initialized) is an empty list, not an error.
        if !manifest_path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&manifest_path).map_err(|e| RuntimeError::ManifestRead {
            path: manifest_path.to_string_lossy().to_string(),
            error: e.to_string(),
        })?;
        parse_installed(&content)
    }

    async fn reproduce_remote(
        &self,
        name: &str,
        target_connection: &str,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError> {
        let flox_env_json_src = format!("{workspace_host_path}/.flox/env.json");
        let flox_env_src = format!("{workspace_host_path}/.flox/env");
        let flox_dir = format!("{WORKSPACE_MOUNT}/.flox");
        // `podman cp` requires the destination parent to exist.
        check(Self::podman_on(target_connection, &["exec", name, "mkdir", "-p", &flox_dir]).await?)?;
        // Copy `env.json` (flox's environment pointer) and `env/` (manifest.toml + manifest.lock).
        // `.flox/run/` is excluded: its activation symlinks cannot be tarred from a Windows host
        // ("archive/tar: unknown file mode") and are machine-local anyway — flox recreates them on activate.
        check(
            Self::podman_on(
                target_connection,
                &["cp", &flox_env_json_src, &format!("{name}:{flox_dir}/env.json")],
            )
            .await?,
        )?;
        check(
            Self::podman_on(
                target_connection,
                &["cp", &flox_env_src, &format!("{name}:{flox_dir}/")],
            )
            .await?,
        )?;
        check(
            Self::podman_on(
                target_connection,
                &["exec", "--workdir", WORKSPACE_MOUNT, name, "flox", "activate", "--", "true"],
            )
            .await?,
        )
        .map(|_| ())
    }
}

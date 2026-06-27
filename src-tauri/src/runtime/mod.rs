//! Flox-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

use std::process::Output;

use tokio::process::Command;

use types::{PodmanFloxRuntime, RuntimeError, RuntimeManager, TerminalSession};

/// Workspace bind-mount target inside every container. Matches `WORKSPACE_MOUNT` in `backends/mod.rs`.
const WORKSPACE_MOUNT: &str = "/root/work";

/// Path of the flox manifest relative to the workspace directory.
const MANIFEST_REL: &str = ".flox/env/manifest.toml";

/// Container port the zellij web server binds. Matches `ZELLIJ_WEB_PORT` in `backends/mod.rs`,
/// which publishes it to the host at `create`.
const ZELLIJ_WEB_PORT: u16 = 8082;

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

/// Parse the plaintext token from `zellij web --create-token` stdout. The token is emitted on a
/// `token_<n>: <value>` line; the value is taken as the text after the colon. Returns the first
/// such value found, or `None` when no token line is present (e.g. the command changed format).
fn parse_token(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .filter(|l| l.trim_start().starts_with("token"))
        .find_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
        .filter(|t| !t.is_empty())
}

/// Parse the published host port from `podman port <name> <port>/tcp` output. Podman prints one
/// `<ip>:<port>` mapping per line (an IPv4 and possibly an IPv6 line); the `127.0.0.1` mapping is
/// selected because `create` publishes to host loopback. Returns the host port string, or `None`
/// when no loopback mapping is present (the container was created without the `--publish` flag).
fn parse_host_port(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("127.0.0.1:"))
        .map(|p| p.to_string())
        .find(|p| !p.is_empty())
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

    /// Run `cmd` in container `name` without a login shell and without `flox` — zellij is on the
    /// global PATH from the baked base image (a login shell would set `NIX_REMOTE=daemon` with no
    /// nix daemon present and break the exec). Returns the raw [`Output`] so the caller can inspect
    /// stdout/stderr and the exit code itself (e.g. `zellij web --status` exits non-zero when
    /// offline, which is not an error to surface).
    async fn container_exec(&self, name: &str, cmd: &[&str]) -> Result<Output, RuntimeError> {
        let mut args = vec!["exec", name];
        args.extend_from_slice(cmd);
        self.podman(&args).await
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

    async fn open_terminal(
        &self,
        name: &str,
        workspace_host_path: &str,
    ) -> Result<TerminalSession, RuntimeError> {
        // The container must be running before any `podman exec`.
        check(self.podman(&["start", name]).await?)?;

        // Ensure the web server is up, idempotently: start it only when `--status` reports offline.
        // `--status` exits non-zero when offline, so inspect the combined output instead of `check`.
        // "online" is not a substring of "offline", so the contains-check does not false-positive.
        let status = self.container_exec(name, &["zellij", "web", "--status"]).await?;
        let status_text = format!(
            "{}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        if !status_text.contains("online") {
            check(self.container_exec(name, &["zellij", "web", "--daemonize"]).await?)?;
        }

        // Mint a one-time token; its plaintext is emitted only here and can never be re-read.
        let token_out = check(self.container_exec(name, &["zellij", "web", "--create-token"]).await?)?;
        let token = parse_token(&token_out).ok_or_else(|| {
            RuntimeError::TerminalUnavailable(format!(
                "could not parse a token from `zellij web --create-token` output:\n{token_out}"
            ))
        })?;

        // Read the ephemeral host port Podman published for the container's web port.
        let port_spec = format!("{ZELLIJ_WEB_PORT}/tcp");
        let port_out = check(self.podman(&["port", name, &port_spec]).await?)?;
        let host_port = parse_host_port(&port_out).ok_or_else(|| {
            RuntimeError::TerminalUnavailable(format!(
                "no 127.0.0.1 host mapping for container port {ZELLIJ_WEB_PORT}; \
                 the sandbox was likely created before the web port was published. Output:\n{port_out}"
            ))
        })?;
        let url = format!("http://127.0.0.1:{host_port}/");

        // Persist the token host-side (the docs/techdebt.md workaround). Writing host-side — the
        // path is bind-mounted into the container at /root/work/.mio/ — avoids shell-escaping JSON
        // through `podman exec`.
        let session = TerminalSession { url, token };
        let mio_dir = std::path::Path::new(workspace_host_path).join(".mio");
        std::fs::create_dir_all(&mio_dir)
            .and_then(|_| {
                let json = serde_json::json!({ "url": session.url, "token": session.token });
                std::fs::write(mio_dir.join("zellij.json"), json.to_string())
            })
            .map_err(|e| {
                RuntimeError::TerminalUnavailable(format!(
                    "failed to write token to {}: {e}",
                    mio_dir.join("zellij.json").to_string_lossy()
                ))
            })?;

        Ok(session)
    }
}

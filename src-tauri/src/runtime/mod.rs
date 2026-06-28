//! mise-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

use std::process::Output;

use tokio::process::Command;

use types::{PodmanMiseRuntime, RuntimeError, RuntimeManager};

/// Workspace bind-mount target inside every container. Matches `WORKSPACE_MOUNT` in `backends/mod.rs`.
const WORKSPACE_MOUNT: &str = "/root/work";

/// Path of the mise manifest (human-readable intent) relative to the workspace directory.
const MANIFEST_REL: &str = "mise.toml";

/// Path of the mise lockfile (resolved, checksum-pinned closure) relative to the workspace directory.
const LOCKFILE_REL: &str = "mise.lock";

/// Container port the zellij web server binds. Matches `ZELLIJ_WEB_PORT` in `backends/mod.rs`,
/// which publishes it to the host at `create`.
const ZELLIJ_WEB_PORT: u16 = 8082;

/// Construct the managed [`RuntimeManager`] for Tauri state. The Podman connection is read from
/// `PODMAN_CONNECTION`; defaults to [`crate::backends::DEFAULT_CONNECTION`] when unset. The e2e
/// harness overrides `PODMAN_CONNECTION` to pin the local test connection.
pub fn default_runtime() -> std::sync::Arc<dyn RuntimeManager> {
    let connection = std::env::var("PODMAN_CONNECTION")
        .unwrap_or_else(|_| crate::backends::DEFAULT_CONNECTION.to_string());
    std::sync::Arc::new(PodmanMiseRuntime { connection, registry_cache: tokio::sync::OnceCell::new() })
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

/// Parse the installed tool names from the `[tools]` table of a `mise.toml`. An absent `[tools]`
/// table (a freshly initialized manifest) yields an empty list, not an error.
fn parse_installed(manifest: &str) -> Result<Vec<String>, RuntimeError> {
    let value: toml::Value =
        toml::from_str(manifest).map_err(|e| RuntimeError::ManifestParse(e.to_string()))?;
    let Some(tools) = value.get("tools").and_then(|v| v.as_table()) else {
        return Ok(Vec::new());
    };
    Ok(tools.keys().cloned().collect())
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

impl PodmanMiseRuntime {
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

    /// Run `mise <args>` in container `name`, in the workspace so mise reads the workspace `mise.toml`.
    /// mise's data dir and lockfile settings come from the image ENV the base image bakes.
    async fn mise_exec(&self, name: &str, mise_args: &[&str]) -> Result<String, RuntimeError> {
        check(self.podman(&["start", name]).await?)?;
        let mut args = vec!["exec", "--workdir", WORKSPACE_MOUNT, name, "mise"];
        args.extend_from_slice(mise_args);
        check(self.podman(&args).await?)
    }
}

#[async_trait::async_trait]
impl RuntimeManager for PodmanMiseRuntime {
    async fn init(&self, workspace_host_path: &str) -> Result<(), RuntimeError> {
        let workspace = std::path::Path::new(workspace_host_path);
        let manifest = workspace.join(MANIFEST_REL);
        if manifest.exists() {
            return Ok(());
        }
        // Written host-side: the workspace is bind-mounted into the container, so no container is
        // needed (mise has no `init` subcommand; the config is otherwise created on first `mise use`).
        // The empty `mise.lock` must exist for `MISE_LOCKFILE=1` installs to maintain the lockfile.
        std::fs::create_dir_all(workspace)
            .and_then(|_| std::fs::write(&manifest, "[tools]\n"))
            .and_then(|_| std::fs::write(workspace.join(LOCKFILE_REL), ""))
            .map_err(|e| RuntimeError::ManifestRead {
                path: manifest.to_string_lossy().to_string(),
                error: e.to_string(),
            })
    }

    async fn install(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        validate_package(package)?;
        self.mise_exec(name, &["use", package]).await.map(|_| ())
    }

    async fn uninstall(&self, name: &str, package: &str) -> Result<(), RuntimeError> {
        validate_package(package)?;
        self.mise_exec(name, &["rm", package]).await.map(|_| ())
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
        let manifest_src = format!("{workspace_host_path}/{MANIFEST_REL}");
        let lockfile_src = format!("{workspace_host_path}/{LOCKFILE_REL}");
        // The container must be running for `podman exec`.
        check(Self::podman_on(target_connection, &["start", name]).await?)?;
        // Copy `mise.toml` and `mise.lock` directly rather than racing the workspace sync (see the
        // "Manifest timing on deploy" note in docs/features/sandbox-runtime-management/02_dev.md).
        check(
            Self::podman_on(
                target_connection,
                &["cp", &manifest_src, &format!("{name}:{WORKSPACE_MOUNT}/{MANIFEST_REL}")],
            )
            .await?,
        )?;
        check(
            Self::podman_on(
                target_connection,
                &["cp", &lockfile_src, &format!("{name}:{WORKSPACE_MOUNT}/{LOCKFILE_REL}")],
            )
            .await?,
        )?;
        // Install the lock-pinned tools from the upstream release hosts. Resolving from `mise.lock`
        // (not tool names) is what makes the remote reproduce the identical builds the laptop locked.
        check(
            Self::podman_on(
                target_connection,
                &["exec", "--workdir", WORKSPACE_MOUNT, name, "mise", "install"],
            )
            .await?,
        )
        .map(|_| ())
    }

    async fn prepare_terminal(
        &self,
        name: &str,
        connection: &str,
        _workspace_host_path: &str,
    ) -> Result<(String, u16), RuntimeError> {
        // The container must be running before any `podman exec`.
        check(Self::podman_on(connection, &["start", name]).await?)?;

        // Ensure the web server is up, idempotently: start it only when `--status` reports offline.
        // `--status` exits non-zero when offline, so inspect the combined output instead of `check`.
        // "online" is not a substring of "offline", so the contains-check does not false-positive.
        let status = Self::podman_on(connection, &["exec", name, "zellij", "web", "--status"]).await?;
        let status_text = format!(
            "{}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        if !status_text.contains("online") {
            check(Self::podman_on(connection, &["exec", name, "zellij", "web", "--daemonize"]).await?)?;
        }

        // Mint a one-time token; its plaintext is emitted only here and can never be re-read.
        let token_out = check(Self::podman_on(connection, &["exec", name, "zellij", "web", "--create-token"]).await?)?;
        let token = parse_token(&token_out).ok_or_else(|| {
            RuntimeError::TerminalUnavailable(format!(
                "could not parse a token from `zellij web --create-token` output:\n{token_out}"
            ))
        })?;

        // Read the ephemeral port Podman published for the container's web port on the connection's
        // host loopback. The returned port is the destination for the Mutagen forwarding session.
        let port_spec = format!("{ZELLIJ_WEB_PORT}/tcp");
        let port_out = check(Self::podman_on(connection, &["port", name, &port_spec]).await?)?;
        let host_port_str = parse_host_port(&port_out).ok_or_else(|| {
            RuntimeError::TerminalUnavailable(format!(
                "no 127.0.0.1 host mapping for container port {ZELLIJ_WEB_PORT}; \
                 the sandbox was likely created before the web port was published. Output:\n{port_out}"
            ))
        })?;
        let host_port: u16 = host_port_str.parse().map_err(|e| {
            RuntimeError::TerminalUnavailable(format!("could not parse host port {host_port_str:?}: {e}"))
        })?;

        Ok((token, host_port))
    }

    fn local_connection(&self) -> &str {
        &self.connection
    }

    async fn registry(&self, name: &str) -> Result<Vec<types::RegistryEntry>, types::RuntimeError> {
        self.registry_cache.get_or_try_init(|| async {
            #[derive(serde::Deserialize)]
            struct Raw { short: String, description: Option<String> }
            let json = self.mise_exec(name, &["registry", "--json"]).await?;
            let raw: Vec<Raw> = serde_json::from_str(&json)
                .map_err(|e| types::RuntimeError::ManifestParse(format!("mise registry --json: {e}")))?;
            Ok(raw.into_iter().map(|r| types::RegistryEntry { short: r.short, description: r.description }).collect())
        }).await.cloned()
    }

    async fn tool_dir(&self, name: &str, tool: &str) -> Result<String, types::RuntimeError> {
        let output = self.mise_exec(name, &["where", tool]).await?;
        Ok(output.trim().to_string())
    }

    async fn exec_in_container(&self, name: &str, args: &[&str]) -> Result<String, types::RuntimeError> {
        check(self.podman(&["start", name]).await?)?;
        let mut full_args = vec!["exec", "--workdir", WORKSPACE_MOUNT, name];
        full_args.extend_from_slice(args);
        check(self.podman(&full_args).await?)
    }
}

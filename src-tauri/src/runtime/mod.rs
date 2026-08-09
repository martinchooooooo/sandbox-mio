//! mise-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

mod mock;

use std::process::Output;

use mock::MockRuntime;
use types::{PodmanMiseRuntime, RuntimeError, RuntimeManager};

const WORKSPACE_MOUNT: &str = "/root/work";

const MANIFEST_REL: &str = "mise.toml";

const LOCKFILE_REL: &str = "mise.lock";

const ZELLIJ_WEB_PORT: u16 = 8082;

const PASEO_WEB_PORT: u16 = 6767;

const PASEO_PROJECTS_DIR: &str = "/root/.paseo/projects";
const PASEO_PROJECTS_FILE: &str = "/root/.paseo/projects/projects.json";

pub fn default_runtime(
    published_port_transport: std::sync::Arc<dyn crate::transport::types::PortTransport>,
) -> std::sync::Arc<dyn RuntimeManager> {
    if crate::backends::mock_backend_enabled() {
        return std::sync::Arc::new(MockRuntime::new());
    }
    let connection = crate::backends::connection_name();
    std::sync::Arc::new(PodmanMiseRuntime {
        connection,
        registry_cache: tokio::sync::OnceCell::new(),
        published_port_transport,
        exec_port_transport: crate::transport::default_exec_port_transport(),
    })
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
    let Some(tools) = value.get("tools").and_then(|v| v.as_table()) else {
        return Ok(Vec::new());
    };
    Ok(tools.keys().cloned().collect())
}

fn parse_token(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .filter(|l| l.trim_start().starts_with("token"))
        .find_map(|l| l.split_once(':').map(|(_, v)| v.trim().to_string()))
        .filter(|t| !t.is_empty())
}

impl PodmanMiseRuntime {
    async fn podman(&self, args: &[&str]) -> Result<Output, RuntimeError> {
        Self::podman_on(&self.connection, args).await
    }

    async fn podman_on(connection: &str, args: &[&str]) -> Result<Output, RuntimeError> {
        let mut full = vec!["--connection", connection];
        full.extend_from_slice(args);
        crate::backends::podman_command()
            .args(&full)
            .output()
            .await
            .map_err(|e| RuntimeError::CommandFailed(e.to_string()))
    }

    fn port_transport(&self, exec_capable: bool) -> &std::sync::Arc<dyn crate::transport::types::PortTransport> {
        if exec_capable {
            &self.exec_port_transport
        } else {
            &self.published_port_transport
        }
    }

    // Runs mise in container workspace; data dir and lockfile from image ENV.
    async fn mise_exec(&self, name: &str, mise_args: &[&str]) -> Result<String, RuntimeError> {
        check(self.podman(&["start", name]).await?)?;
        let mut args = vec!["exec", "--workdir", WORKSPACE_MOUNT, name, "mise"];
        args.extend_from_slice(mise_args);
        check(self.podman(&args).await?)
    }

    async fn seed_paseo_projects(connection: &str, name: &str) -> Result<(), RuntimeError> {
        let already_seeded = Self::podman_on(connection, &["exec", name, "test", "-f", PASEO_PROJECTS_FILE])
            .await
            .map(|out| out.status.success())
            .unwrap_or(false);
        if already_seeded {
            return Ok(());
        }

        check(Self::podman_on(connection, &["exec", name, "mkdir", "-p", PASEO_PROJECTS_DIR]).await?)?;

        let now = iso8601_now();
        let projects = serde_json::json!([{
            "projectId": WORKSPACE_MOUNT,
            "rootPath": WORKSPACE_MOUNT,
            "kind": "non_git",
            "displayName": name,
            "customName": null,
            "createdAt": &now,
            "updatedAt": &now,
            "archivedAt": null,
        }]);
        let contents = serde_json::to_string_pretty(&projects)
            .map_err(|e| RuntimeError::AgentWebUnavailable(e.to_string()))?;

        let tmp_path = std::env::temp_dir().join(format!("paseo-projects-{name}.json"));
        std::fs::write(&tmp_path, &contents)
            .map_err(|e| RuntimeError::AgentWebUnavailable(format!("failed to write {}: {e}", tmp_path.to_string_lossy())))?;
        let dest = format!("{name}:{PASEO_PROJECTS_FILE}");
        let result = check(Self::podman_on(connection, &["cp", &tmp_path.to_string_lossy(), &dest]).await?);
        let _ = std::fs::remove_file(&tmp_path);
        result.map(|_| ())
    }
}

fn iso8601_now() -> String {
    let since_epoch =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = since_epoch.as_secs();
    let millis = since_epoch.subsec_millis();
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[async_trait::async_trait]
impl RuntimeManager for PodmanMiseRuntime {
    async fn init(&self, workspace_host_path: &str) -> Result<(), RuntimeError> {
        let workspace = std::path::Path::new(workspace_host_path);
        let manifest = workspace.join(MANIFEST_REL);
        if manifest.exists() {
            return Ok(());
        }
        // Write host-side; workspace is bind-mounted so container init not needed.
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
        exec_capable: bool,
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
        // host loopback (or, for an exec-capable sandbox, the unchanged container port — see
        // `PassthroughPortTransport`). The returned port is the destination for the Mutagen
        // forwarding session.
        let transport = self.port_transport(exec_capable);
        let host_port = transport
            .resolve_remote_port(name, connection, ZELLIJ_WEB_PORT)
            .await
            .map_err(|e| RuntimeError::TerminalUnavailable(e.to_string()))?;

        Ok((token, host_port))
    }

    async fn prepare_agent_web(
        &self,
        name: &str,
        connection: &str,
        password: &str,
        exec_capable: bool,
    ) -> Result<u16, RuntimeError> {
        // The container must be running before any `podman exec`.
        check(Self::podman_on(connection, &["start", name]).await?)?;

        Self::seed_paseo_projects(connection, name).await?;

        check(
            Self::podman_on(
                connection,
                &[
                    "exec", "--workdir", WORKSPACE_MOUNT, name,
                    "mise", "use", "node@lts", "npm:@getpaseo/cli",
                ],
            )
            .await?,
        )?;

        // Stop any running daemon unconditionally (best-effort, fails when none is running):
        let _ = Self::podman_on(
            connection,
            &[
                "exec", "--workdir", WORKSPACE_MOUNT, name,
                "mise", "exec", "--", "paseo", "daemon", "stop",
            ],
        )
        .await;

        // `mise exec` from the workspace dir puts the workspace's tools (opencode, pi) on PATH
        // and applies `mise.toml [env]`, so paseo discovers installed providers and its spawned
        // agents see the provisioned environment.
        let listen = format!("0.0.0.0:{PASEO_WEB_PORT}");
        let password_env = format!("PASEO_PASSWORD={password}");
        check(
            Self::podman_on(
                connection,
                &[
                    "exec", "-d", "--workdir", WORKSPACE_MOUNT, "--env", &password_env, name,
                    "mise", "exec", "--", "paseo", "daemon", "start", "--foreground",
                    "--listen", &listen, "--web-ui", "--hostnames", "true",
                ],
            )
            .await?,
        )?;

        // `exec -d --foreground` returns once the process is spawned, not once it accepts
        // connections, and a fresh daemon's startup can outlast the web client's connect
        // timeout.
        let listen_url = format!("http://127.0.0.1:{PASEO_WEB_PORT}/");
        check(
            Self::podman_on(
                connection,
                &[
                    "exec", name, "curl", "--silent", "--fail", "--output", "/dev/null",
                    "--retry", "30", "--retry-delay", "1", "--retry-connrefused",
                    "--max-time", "45", &listen_url,
                ],
            )
            .await?,
        )
        .map_err(|e| {
            RuntimeError::AgentWebUnavailable(format!(
                "paseo daemon did not come up on {PASEO_WEB_PORT} within 45s: {e}"
            ))
        })?;

        self.port_transport(exec_capable)
            .resolve_remote_port(name, connection, PASEO_WEB_PORT)
            .await
            .map_err(|e| RuntimeError::AgentWebUnavailable(e.to_string()))
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

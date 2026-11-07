//! mise-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/lib.rs`.

pub mod types;

mod mock;

use std::process::Output;

use std::sync::Arc;

use mock::MockRuntime;
use types::{MiseRuntime, RuntimeError, RuntimeManager};

use crate::provider::types::{ExecOpts, MachineProvider, MachineRef};

const WORKSPACE_MOUNT: &str = "/root/work";

const MANIFEST_REL: &str = "mise.toml";

const LOCKFILE_REL: &str = "mise.lock";

const ZELLIJ_WEB_PORT: u16 = 8082;

const PASEO_WEB_PORT: u16 = 6767;

const PASEO_PROJECTS_DIR: &str = "/root/.paseo/projects";
const PASEO_PROJECTS_FILE: &str = "/root/.paseo/projects/projects.json";

/// Pinned rather than `node@lts` / `npm:@getpaseo/cli` (latest): `prepare_agent_web` runs this
/// `mise use` on every "Agents" tab open, and an unpinned tag forces mise to re-resolve it over
/// the network every time, even when the tool is already installed — slow on its own and worse
/// through a sandbox's traffic proxy (TLS interception per request). A pinned version already
/// present locally needs no network round trip. Bump these deliberately, not automatically.
const PASEO_NODE_TOOL: &str = "node@24.19.0";
const PASEO_CLI_TOOL: &str = "npm:@getpaseo/cli@0.4.0";

pub fn default_runtime(
    providers: Arc<crate::provider::ProviderRegistry>,
    published_port_transport: Arc<dyn crate::transport::types::PortTransport>,
    secrets: Arc<dyn types::ToolchainSecrets>,
) -> Arc<dyn RuntimeManager> {
    if crate::backends::mock_backend_enabled() {
        return Arc::new(MockRuntime::new());
    }
    Arc::new(MiseRuntime {
        providers,
        registry_cache: tokio::sync::OnceCell::new(),
        published_port_transport,
        exec_port_transport: crate::transport::default_exec_port_transport(),
        secrets,
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

/// Options that exec `mise` from the workspace directory: `WORKSPACE_MOUNT` as `--workdir`, no env,
/// not detached.
fn workdir_opts() -> ExecOpts {
    ExecOpts { workdir: Some(WORKSPACE_MOUNT.to_string()), ..Default::default() }
}

impl MiseRuntime {
    /// The provider driving `machine`, which may be a deploy target rather than the local one.
    fn provider_on(&self, machine: &MachineRef) -> Arc<dyn MachineProvider> {
        self.providers.provider_for(machine)
    }

    /// The provider running the sandbox named `name`, for the name-only methods that carry no
    /// [`MachineRef`] of their own.
    async fn provider_of(&self, name: &str) -> Arc<dyn MachineProvider> {
        self.providers.provider_of(name).await.1
    }

    fn port_transport(&self, exec_capable: bool) -> &Arc<dyn crate::transport::types::PortTransport> {
        if exec_capable {
            &self.exec_port_transport
        } else {
            &self.published_port_transport
        }
    }

    /// [`workdir_opts`] plus the toolchain secret environment, for calls that resolve or download
    /// tools.
    fn network_opts(&self) -> ExecOpts {
        ExecOpts { secret_env: self.secrets.env(), ..workdir_opts() }
    }

    // Runs mise in container workspace; data dir and lockfile from image ENV.
    async fn mise_exec(&self, name: &str, mise_args: &[&str]) -> Result<String, RuntimeError> {
        self.mise_exec_with(name, mise_args, workdir_opts()).await
    }

    async fn mise_exec_with(
        &self,
        name: &str,
        mise_args: &[&str],
        opts: ExecOpts,
    ) -> Result<String, RuntimeError> {
        let provider = self.provider_of(name).await;
        provider.start(name).await?;
        let mut argv = vec!["mise"];
        argv.extend_from_slice(mise_args);
        check(provider.exec(name, &argv, opts).await?)
    }

    async fn seed_paseo_projects(
        provider: &Arc<dyn MachineProvider>,
        name: &str,
    ) -> Result<(), RuntimeError> {
        let already_seeded = provider
            .exec(name, &["test", "-f", PASEO_PROJECTS_FILE], ExecOpts::default())
            .await
            .map(|out| out.status.success())
            .unwrap_or(false);
        if already_seeded {
            return Ok(());
        }

        check(provider.exec(name, &["mkdir", "-p", PASEO_PROJECTS_DIR], ExecOpts::default()).await?)?;

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
        let result = provider.copy_in(name, &tmp_path.to_string_lossy(), PASEO_PROJECTS_FILE).await;
        let _ = std::fs::remove_file(&tmp_path);
        result.map_err(RuntimeError::from)
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
impl RuntimeManager for MiseRuntime {
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
        self.mise_exec_with(name, &["use", package], self.network_opts()).await.map(|_| ())
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
        target: &MachineRef,
        workspace_host_path: &str,
    ) -> Result<(), RuntimeError> {
        let manifest_src = format!("{workspace_host_path}/{MANIFEST_REL}");
        let lockfile_src = format!("{workspace_host_path}/{LOCKFILE_REL}");
        // `reproduce_remote` targets an arbitrary machine, so it drives that machine's provider
        // rather than the local one, exactly as `deploy` does in `backends`.
        let provider = self.provider_on(target);
        // The container must be running for `podman exec`.
        provider.start(name).await?;
        // Copy `mise.toml` and `mise.lock` directly rather than racing the workspace sync (see the
        // "Manifest timing on deploy" note in docs/features/sandbox-runtime-management/02_dev.md).
        provider.copy_in(name, &manifest_src, &format!("{WORKSPACE_MOUNT}/{MANIFEST_REL}")).await?;
        provider.copy_in(name, &lockfile_src, &format!("{WORKSPACE_MOUNT}/{LOCKFILE_REL}")).await?;
        // Install the lock-pinned tools from the upstream release hosts. Resolving from `mise.lock`
        // (not tool names) is what makes the remote reproduce the identical builds the laptop locked.
        check(provider.exec(name, &["mise", "install"], self.network_opts()).await?).map(|_| ())
    }

    async fn prepare_terminal(
        &self,
        name: &str,
        machine: &MachineRef,
        _workspace_host_path: &str,
        exec_capable: bool,
    ) -> Result<(String, u16), RuntimeError> {
        let provider = self.provider_on(machine);
        // The container must be running before any `podman exec`.
        provider.start(name).await?;

        // Ensure the web server is up, idempotently: start it only when `--status` reports offline.
        // `--status` exits non-zero when offline, so inspect the combined output instead of `check`.
        // "online" is not a substring of "offline", so the contains-check does not false-positive.
        let status = provider.exec(name, &["zellij", "web", "--status"], ExecOpts::default()).await?;
        let status_text = format!(
            "{}{}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        if !status_text.contains("online") {
            check(provider.exec(name, &["zellij", "web", "--daemonize"], ExecOpts::default()).await?)?;
        }

        // Mint a one-time token; its plaintext is emitted only here and can never be re-read.
        let token_out = check(provider.exec(name, &["zellij", "web", "--create-token"], ExecOpts::default()).await?)?;
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
            .resolve_remote_port(name, &machine.id, ZELLIJ_WEB_PORT)
            .await
            .map_err(|e| RuntimeError::TerminalUnavailable(e.to_string()))?;

        Ok((token, host_port))
    }

    async fn prepare_agent_web(
        &self,
        name: &str,
        machine: &MachineRef,
        password: &str,
        exec_capable: bool,
    ) -> Result<u16, RuntimeError> {
        let provider = self.provider_on(machine);
        // The container must be running before any `podman exec`.
        provider.start(name).await?;

        Self::seed_paseo_projects(&provider, name).await?;

        check(
            provider
                .exec(name, &["mise", "use", PASEO_NODE_TOOL, PASEO_CLI_TOOL], self.network_opts())
                .await?,
        )?;

        // Stop any running daemon unconditionally (best-effort, fails when none is running):
        let _ = provider
            .exec(name, &["mise", "exec", "--", "paseo", "daemon", "stop"], workdir_opts())
            .await;

        // `mise exec` from the workspace dir puts the workspace's tools (opencode, pi) on PATH
        // and applies `mise.toml [env]`, so paseo discovers installed providers and its spawned
        // agents see the provisioned environment.
        let listen = format!("0.0.0.0:{PASEO_WEB_PORT}");
        let start_opts = ExecOpts {
            workdir: Some(WORKSPACE_MOUNT.to_string()),
            secret_env: vec![("PASEO_PASSWORD".to_string(), password.to_string())],
            detached: true,
            ..Default::default()
        };
        check(
            provider
                .exec(
                    name,
                    &[
                        "mise", "exec", "--", "paseo", "daemon", "start", "--foreground",
                        "--listen", &listen, "--web-ui", "--hostnames", "true",
                    ],
                    start_opts,
                )
                .await?,
        )?;

        // `exec -d --foreground` returns once the process is spawned, not once it accepts
        // connections, and a fresh daemon's startup can outlast the web client's connect
        // timeout.
        let listen_url = format!("http://127.0.0.1:{PASEO_WEB_PORT}/");
        check(
            provider
                .exec(
                    name,
                    &[
                        "curl", "--silent", "--fail", "--output", "/dev/null",
                        "--retry", "30", "--retry-delay", "1", "--retry-connrefused",
                        "--max-time", "45", &listen_url,
                    ],
                    ExecOpts::default(),
                )
                .await?,
        )
        .map_err(|e| {
            RuntimeError::AgentWebUnavailable(format!(
                "paseo daemon did not come up on {PASEO_WEB_PORT} within 45s: {e}"
            ))
        })?;

        self.port_transport(exec_capable)
            .resolve_remote_port(name, &machine.id, PASEO_WEB_PORT)
            .await
            .map_err(|e| RuntimeError::AgentWebUnavailable(e.to_string()))
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
        let provider = self.provider_of(name).await;
        provider.start(name).await?;
        check(provider.exec(name, args, self.network_opts()).await?)
    }
}

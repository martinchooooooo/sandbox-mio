//! mise-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/bootstrap.rs`.

pub mod types;

mod mock;
mod paseo_plugins;
/// Release builds embed the archive `src-tauri/build.rs` packs with this same module.
#[cfg(any(debug_assertions, test))]
mod plugin_archive;

use std::collections::HashMap;
use std::process::Output;

use std::sync::{Arc, LazyLock, Mutex};

use mock::MockRuntime;
use types::{MiseRuntime, RuntimeError, RuntimeManager};

use mio_provider::types::{ExecOpts, MachineProvider, MachineRef};

const WORKSPACE_MOUNT: &str = "/root/work";

const MANIFEST_REL: &str = "mise.toml";

const LOCKFILE_REL: &str = "mise.lock";

/// The container port the Paseo daemon listens on and its forward dials.
pub const PASEO_WEB_PORT: u16 = 6767;

/// Pinned because the launch script and the login seeded by `prepare_agent_web_pane`
/// (`src-tauri/src/services/panes.rs`) rely on Paseo internals; re-check both when bumping.
/// The CLI version must equal `@getpaseo/plugin` in `paseo-plugins/package.json` (tested).
const PASEO_NODE_TOOL: &str = "node@24.19.0";
const PASEO_CLI_TOOL: &str = "npm:@getpaseo/cli@0.16.1";

/// Starts the Paseo daemon, or confirms the running one is current, in one exec
/// (`docs/features/agent-web-ui/02_dev.md`, Revision: Paseo 0.16).
const PASEO_UP_SCRIPT: &str = include_str!("../../resources/agent-web/paseo-up.sh");
/// [`PASEO_UP_SCRIPT`] exit code: the pinned tools are missing; install them and run it again.
const PASEO_UP_INSTALL_NEEDED: i32 = 10;
/// [`PASEO_UP_SCRIPT`] exit code: the daemon is still starting; run it again.
const PASEO_UP_STARTING: i32 = 11;
/// [`PASEO_UP_SCRIPT`] exit code: the delivered plugins are stale; deliver them and run it again.
const PASEO_UP_PLUGINS_STALE: i32 = 12;
/// `podman exec` exit code and stderr marker for a container that is not running.
const PODMAN_EXEC_NOT_RUNNING_CODE: i32 = 125;
const PODMAN_EXEC_NOT_RUNNING_MARKER: &str = "container state improper";
/// Each starting run waits about 15s, so with the install and delivery runs this covers a cold
/// start of about a minute.
const PASEO_UP_ATTEMPTS: usize = 6;

/// One lock per sandbox around `prepare_agent_web`. paseo-up's flock covers only the script, so a
/// pre-warm racing a pane open would otherwise run the `mise use` and plugin delivery twice at once.
static AGENT_WEB_LOCKS: LazyLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Replaces the in-sandbox plugin directory with the archive on stdin.
const PASEO_PLUGINS_DELIVER_SCRIPT: &str = include_str!("../../resources/agent-web/paseo-plugins-deliver.sh");
/// Container-filesystem home of the app-owned Paseo plugins, outside the workspace
/// (`docs/features/paseo-plugins/01_ideas.md`, Where plugin files live in the sandbox).
const PASEO_PLUGIN_DIR: &str = "/usr/local/lib/mio/paseo-plugins";

pub fn default_runtime(
    providers: Arc<mio_provider::ProviderRegistry>,
    secrets: Arc<dyn types::ToolchainSecrets>,
) -> Arc<dyn RuntimeManager> {
    if mio_domain::mock_backend_enabled() {
        return Arc::new(MockRuntime::new());
    }
    Arc::new(MiseRuntime {
        providers,
        registry_cache: tokio::sync::OnceCell::new(),
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

    /// [`workdir_opts`] plus the toolchain secret environment, for calls that resolve or download
    /// tools.
    fn network_opts(&self, name: &str) -> ExecOpts {
        ExecOpts { secret_env: self.secrets.env(name), ..workdir_opts() }
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
}

/// Whether `out` is podman refusing an exec because the container is stopped.
fn exec_hit_stopped_container(out: &Output) -> bool {
    out.status.code() == Some(PODMAN_EXEC_NOT_RUNNING_CODE)
        && String::from_utf8_lossy(&out.stderr).contains(PODMAN_EXEC_NOT_RUNNING_MARKER)
}

/// Splits a pinned mise tool spec such as `node@24.19.0` into its tool name and version.
fn split_tool_spec(spec: &str) -> (&str, &str) {
    spec.rsplit_once('@').unwrap_or((spec, ""))
}

/// The plain inputs of [`PASEO_UP_SCRIPT`]; the password travels separately as a secret.
fn paseo_up_env(
    project_name: &str,
    plugins: &paseo_plugins::PluginBundle,
    prewarm: bool,
) -> Vec<(String, String)> {
    let (node, node_version) = split_tool_spec(PASEO_NODE_TOOL);
    let (cli, cli_version) = split_tool_spec(PASEO_CLI_TOOL);
    [
        ("MIO_PASEO_PORT", PASEO_WEB_PORT.to_string()),
        ("MIO_PASEO_NODE", node.to_string()),
        ("MIO_PASEO_NODE_VERSION", node_version.to_string()),
        ("MIO_PASEO_CLI", cli.to_string()),
        ("MIO_PASEO_CLI_VERSION", cli_version.to_string()),
        ("MIO_PASEO_PROJECT_NAME", project_name.to_string()),
        ("MIO_PASEO_PLUGIN_DIR", PASEO_PLUGIN_DIR.to_string()),
        ("MIO_PASEO_PLUGIN_HASH", plugins.hash.clone()),
        ("MIO_PASEO_PLUGIN_IDS", plugins.ids.join(" ")),
        ("MIO_PASEO_PREWARM", if prewarm { "1" } else { "0" }.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
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
        self.mise_exec_with(name, &["use", package], self.network_opts(name)).await.map(|_| ())
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
        check(provider.exec(name, &["mise", "install"], self.network_opts(name)).await?).map(|_| ())
    }

    async fn ensure_running(&self, name: &str, machine: &MachineRef) -> Result<(), RuntimeError> {
        Ok(self.provider_on(machine).start(name).await?)
    }

    async fn prepare_agent_web(
        &self,
        name: &str,
        machine: &MachineRef,
        password: &str,
        opening: bool,
    ) -> Result<(), RuntimeError> {
        let lock = AGENT_WEB_LOCKS.lock().unwrap().entry(name.to_string()).or_default().clone();
        let _serialized = lock.lock().await;
        let provider = self.provider_on(machine);

        // The script reads the workspace's `mise.toml`, and its `mise exec` launch puts the
        // workspace's tools on the daemon's PATH and applies `mise.toml [env]` to its agents.
        let plugins = paseo_plugins::plugin_bundle()?;
        let script_opts = || ExecOpts {
            workdir: Some(WORKSPACE_MOUNT.to_string()),
            env: paseo_up_env(name, &plugins, !opening),
            secret_env: vec![("PASEO_PASSWORD".to_string(), password.to_string())],
            detached: false,
        };
        let argv = ["sh", "-c", PASEO_UP_SCRIPT, "paseo-up"];
        let mut installed = false;
        let mut delivered = false;
        for attempt in 1..=PASEO_UP_ATTEMPTS {
            let began = std::time::Instant::now();
            let mut out = provider.exec(name, &argv, script_opts()).await?;
            // Only the first exec: a container stopped on a later attempt was stopped during the
            // open, and starting it again would undo the user's stop.
            if opening && attempt == 1 && exec_hit_stopped_container(&out) {
                provider.start(name).await?;
                out = provider.exec(name, &argv, script_opts()).await?;
            }
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            tracing::info!(
                target: "runtime",
                sandbox = name,
                attempt,
                code = out.status.code().unwrap_or(-1),
                ms = began.elapsed().as_millis() as u64,
                "paseo-up run"
            );
            for line in String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.starts_with("timing ")) {
                tracing::debug!(target: "runtime", sandbox = name, attempt, "paseo-up {line}");
            }
            match out.status.code() {
                Some(0) => {
                    if !stderr.is_empty() {
                        tracing::warn!(target: "runtime", sandbox = name, %stderr, "paseo-up reported a problem");
                    }
                    return Ok(());
                }
                // Only when missing: a `mise use` on every open re-resolves the tools.
                Some(PASEO_UP_INSTALL_NEEDED) if !installed => {
                    let began = std::time::Instant::now();
                    check(
                        provider
                            .exec(name, &["mise", "use", PASEO_NODE_TOOL, PASEO_CLI_TOOL], self.network_opts(name))
                            .await?,
                    )?;
                    installed = true;
                    tracing::info!(target: "runtime", sandbox = name, ms = began.elapsed().as_millis() as u64, "paseo tools installed");
                }
                Some(PASEO_UP_PLUGINS_STALE) if !delivered => {
                    let began = std::time::Instant::now();
                    let deliver = [
                        "sh",
                        "-c",
                        PASEO_PLUGINS_DELIVER_SCRIPT,
                        "paseo-plugins-deliver",
                        PASEO_PLUGIN_DIR,
                        &plugins.hash,
                    ];
                    let out = provider.exec_piped(name, &deliver, &plugins.archive).await?;
                    if !out.status.success() {
                        return Err(RuntimeError::AgentWebUnavailable(format!(
                            "delivering Paseo plugins failed: {}",
                            String::from_utf8_lossy(&out.stderr).trim()
                        )));
                    }
                    delivered = true;
                    tracing::info!(target: "runtime", sandbox = name, ms = began.elapsed().as_millis() as u64, "paseo plugins delivered");
                }
                Some(PASEO_UP_PLUGINS_STALE) => {
                    return Err(RuntimeError::AgentWebUnavailable(format!(
                        "Paseo plugins were delivered but {PASEO_PLUGIN_DIR}/.bundle-hash still differs from {}",
                        plugins.hash
                    )))
                }
                Some(PASEO_UP_STARTING) => {}
                code => {
                    return Err(RuntimeError::AgentWebUnavailable(format!(
                        "paseo-up exited with {code:?}: {stderr}"
                    )))
                }
            }
        }
        Err(RuntimeError::AgentWebUnavailable(format!(
            "paseo daemon did not come up on {PASEO_WEB_PORT} after {PASEO_UP_ATTEMPTS} attempts"
        )))
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
        check(provider.exec(name, args, self.network_opts(name)).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_spec_splits_at_the_version_not_the_npm_scope() {
        assert_eq!(split_tool_spec("npm:@getpaseo/cli@0.16.1"), ("npm:@getpaseo/cli", "0.16.1"));
        assert_eq!(split_tool_spec("node@24.19.0"), ("node", "24.19.0"));
    }
}

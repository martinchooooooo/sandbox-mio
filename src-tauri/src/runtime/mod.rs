//! mise-based software management for sandboxes. Public surface is [`types::RuntimeManager`],
//! held in Tauri state as `Arc<dyn RuntimeManager>` and registered in `src-tauri/src/bootstrap.rs`.

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

const PASEO_WEB_PORT: u16 = 6767;

/// Pinned because the launch script and the login seeded by `prepare_agent_web_pane`
/// (`src-tauri/src/services/panes.rs`) rely on Paseo internals; re-check both when bumping.
const PASEO_NODE_TOOL: &str = "node@24.19.0";
const PASEO_CLI_TOOL: &str = "npm:@getpaseo/cli@0.16.1";

/// Starts the Paseo daemon, or confirms the running one is current, in one exec
/// (`docs/features/agent-web-ui/02_dev.md`, Revision: Paseo 0.16).
const PASEO_UP_SCRIPT: &str = include_str!("../../resources/agent-web/paseo-up.sh");
/// [`PASEO_UP_SCRIPT`] exit code: the pinned tools are missing; install them and run it again.
const PASEO_UP_INSTALL_NEEDED: i32 = 10;
/// [`PASEO_UP_SCRIPT`] exit code: the daemon is still starting; run it again.
const PASEO_UP_STARTING: i32 = 11;
/// Each run waits about 15s, so this covers a cold start of about a minute.
const PASEO_UP_ATTEMPTS: usize = 5;

pub fn default_runtime(
    providers: Arc<crate::provider::ProviderRegistry>,
    secrets: Arc<dyn types::ToolchainSecrets>,
) -> Arc<dyn RuntimeManager> {
    if crate::backends::mock_backend_enabled() {
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
}

/// Splits a pinned mise tool spec such as `node@24.19.0` into its tool name and version.
fn split_tool_spec(spec: &str) -> (&str, &str) {
    spec.rsplit_once('@').unwrap_or((spec, ""))
}

/// The plain inputs of [`PASEO_UP_SCRIPT`]; the password travels separately as a secret.
fn paseo_up_env(project_name: &str) -> Vec<(String, String)> {
    let (node, node_version) = split_tool_spec(PASEO_NODE_TOOL);
    let (cli, cli_version) = split_tool_spec(PASEO_CLI_TOOL);
    [
        ("MIO_PASEO_PORT", PASEO_WEB_PORT.to_string()),
        ("MIO_PASEO_NODE", node.to_string()),
        ("MIO_PASEO_NODE_VERSION", node_version.to_string()),
        ("MIO_PASEO_CLI", cli.to_string()),
        ("MIO_PASEO_CLI_VERSION", cli_version.to_string()),
        ("MIO_PASEO_PROJECT_NAME", project_name.to_string()),
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

    async fn prepare_agent_web(
        &self,
        name: &str,
        machine: &MachineRef,
        password: &str,
    ) -> Result<u16, RuntimeError> {
        let provider = self.provider_on(machine);
        // The container must be running before any `podman exec`.
        provider.start(name).await?;

        // The script reads the workspace's `mise.toml`, and its `mise exec` launch puts the
        // workspace's tools on the daemon's PATH and applies `mise.toml [env]` to its agents.
        let script_opts = || ExecOpts {
            workdir: Some(WORKSPACE_MOUNT.to_string()),
            env: paseo_up_env(name),
            secret_env: vec![("PASEO_PASSWORD".to_string(), password.to_string())],
            detached: false,
        };
        let argv = ["sh", "-c", PASEO_UP_SCRIPT, "paseo-up"];
        let mut installed = false;
        for _ in 0..PASEO_UP_ATTEMPTS {
            let out = provider.exec(name, &argv, script_opts()).await?;
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            match out.status.code() {
                Some(0) => {
                    if !stderr.is_empty() {
                        tracing::warn!(target: "runtime", sandbox = name, %stderr, "paseo-up reported a problem");
                    }
                    return Ok(PASEO_WEB_PORT);
                }
                // Only when missing: a `mise use` on every open re-resolves the tools.
                Some(PASEO_UP_INSTALL_NEEDED) if !installed => {
                    check(
                        provider
                            .exec(name, &["mise", "use", PASEO_NODE_TOOL, PASEO_CLI_TOOL], self.network_opts())
                            .await?,
                    )?;
                    installed = true;
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
        check(provider.exec(name, args, self.network_opts()).await?)
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

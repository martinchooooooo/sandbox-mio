pub mod types;

use std::process::{Output, Stdio};
use std::sync::Arc;

use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::env_ext::env_dir;
use crate::process::types::ProcessRunner;
use crate::process::{detached_command, detached_command_std};
use crate::provider::types::{InstanceSpec, ProviderId};
use crate::provider::ProviderRegistry;
use types::{
    BackendError, CliBackend, ConnectionInfo, RemoteDeployment, Sandbox, SandboxBackend,
    SyncStrategy, WorkspacePlan,
};

// `e2e/lib/podman-machine.ts` regex-parses the two declarations below and `DEFAULT_MACHINE_NAME`,
// so their literal form (name, type, `const`) is load-bearing beyond Rust.
const BASE_IMAGE: &str = "sandbox-mio/base";
/// Bump whenever the Containerfile changes: `ensure_base_image` short-circuits on an existing
/// tag, so an unchanged version silently keeps stale images.
pub(crate) const BASE_IMAGE_VERSION: u32 = 7;

/// The connection `podman machine init` creates for the local machine. Renaming this const (not its
/// value) breaks the regex parse in `e2e/lib/podman-machine.ts`, which mirrors it as `DEFAULT_MACHINE`.
pub(crate) const DEFAULT_MACHINE_NAME: &str = "podman-machine-default";

pub(crate) fn base_image() -> String {
    format!("{BASE_IMAGE}:{BASE_IMAGE_VERSION}")
}

const DISMISSED_MARKER: &str = "dismissed_image_version";

/// The [`BASE_IMAGE_VERSION`] a workspace's user dismissed the update prompt for. Absent or
/// unreadable means nothing was dismissed, so the prompt shows.
pub fn dismissed_image_version(workspace_path: &str) -> Option<u32> {
    let path = crate::mio_files::mio_path(workspace_path, DISMISSED_MARKER);
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Record that the workspace's user dismissed the update prompt for `version`.
pub fn dismiss_image_update(workspace_path: &str, version: u32) -> Result<(), BackendError> {
    let path = crate::mio_files::mio_path(workspace_path, DISMISSED_MARKER);
    Ok(crate::mio_files::write_text(&path, &version.to_string())?)
}

/// The Podman connection name container and runtime operations target: `PODMAN_CONNECTION` when set
/// (e2e points this at a non-default machine, e.g. `mio-tests`), else `DEFAULT_MACHINE_NAME`.
///
/// Does not steer machine lifecycle (`src-tauri/src/machine/mod.rs`, which has its own
/// `MIO_PODMAN_MACHINE` override): a connection name isn't guaranteed to name a local machine — see
/// `add_host` below — so `podman machine` subcommands can't safely run against it.
pub(crate) fn connection_name() -> String {
    std::env::var("PODMAN_CONNECTION").unwrap_or_else(|_| DEFAULT_MACHINE_NAME.to_string())
}

/// The account the exec-transport SSH session authenticates as. Must match the base image, which
/// runs everything as root (`/root/work` lives under `/root`).
const CONTAINER_USER: &str = "root";

fn workspace_root() -> Result<std::path::PathBuf, BackendError> {
    if let Some(root) = env_dir("SANDBOX_WORKSPACE_ROOT") {
        return Ok(root);
    }
    let documents = dirs::document_dir().ok_or(BackendError::DocumentsDirNotFound)?;
    Ok(documents.join("sandbox-mio"))
}

/// Rejects names Podman would refuse. Also the path-safety gate for `workspace_path`: the name
/// becomes a path segment, so barring `/` and `..` keeps auto-created workspaces inside the root.
pub(crate) fn validate_name(name: &str) -> Result<(), BackendError> {
    let valid = name
        .chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphanumeric() || (i > 0 && matches!(c, '_' | '.' | '-')));
    if name.is_empty() || !valid {
        return Err(BackendError::InvalidName { name: name.to_string() });
    }
    Ok(())
}

/// [`validate_name`] plus the checks that apply only to a name being newly claimed: the proxy
/// sidecar suffix is reserved. Existing sandboxes that predate the reservation stay addressable.
pub(crate) fn validate_new_name(name: &str) -> Result<(), BackendError> {
    validate_name(name)?;
    if name.ends_with(crate::proxy::types::PROXY_CONTAINER_SUFFIX) {
        return Err(BackendError::ReservedName { name: name.to_string() });
    }
    Ok(())
}

/// Name-derived workspace path under `workspace_root()`. `pub(crate)` because
/// `PodmanProvider::list_instances` falls back to it for containers lacking the workspace label.
pub(crate) fn workspace_path(name: &str) -> Result<String, BackendError> {
    let path = workspace_root()?.join(name);
    Ok(path.to_string_lossy().replace('\\', "/"))
}

pub fn default_backend(
    registry: Arc<ProviderRegistry>,
    runner: Arc<dyn ProcessRunner>,
    app: tauri::AppHandle,
) -> std::sync::Arc<dyn SandboxBackend> {
    let connection = connection_name();
    std::sync::Arc::new(CliBackend {
        connection,
        registry,
        runner,
        app,
        events_stop: std::sync::Mutex::new(None),
    })
}

pub(crate) fn mock_backend_enabled() -> bool {
    std::env::var("MIO_MOCK_BACKEND").ok().as_deref() == Some("1")
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
fn podman_literal_path() -> String {
    match PODMAN_RESOURCE_DIR.get() {
        Some(dir) => {
            let path = dir.join("podman.exe").to_string_lossy().replace('\\', "/");
            format!("\"{path}\"")
        }
        None => "podman".to_string(),
    }
}

async fn run_podman(
    runner: &Arc<dyn ProcessRunner>,
    args: &[&str],
) -> Result<Output, BackendError> {
    runner.run(args).await.map_err(BackendError::from)
}

/// Returns stdout on success, or the captured stderr as `Err` on a non-zero exit.
/// Podman writes lifecycle errors (no such image, no such container, ...) to stderr.
fn check(out: Output) -> Result<String, BackendError> {
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    } else {
        Err(BackendError::CommandFailed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ))
    }
}

fn ssh_target(ssh_url: &str) -> Result<(String, Option<String>), BackendError> {
    let rest = ssh_url
        .strip_prefix("ssh://")
        .ok_or_else(|| BackendError::ParseError(format!("expected an ssh:// URL, got: {ssh_url}")))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err(BackendError::ParseError(format!("ssh URL is missing a host: {ssh_url}")));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() {
            return Ok((host.to_string(), Some(port.to_string())));
        }
    }
    Ok((authority.to_string(), None))
}

fn parse_connection_entry(entry: &serde_json::Value) -> Result<Option<ConnectionInfo>, BackendError> {
    let uri = entry["URI"]
        .as_str()
        .ok_or_else(|| BackendError::ParseError("connection entry has no URI".to_string()))?;
    let identity = entry["Identity"].as_str().unwrap_or_default().to_string();
    let Some(authority) = uri.strip_prefix("ssh://").and_then(|r| r.split('/').next()) else {
        // Not an ssh:// connection (e.g. a local unix socket): nothing for Mutagen to sync over.
        return Ok(None);
    };
    let (user, hostport) = authority
        .split_once('@')
        .ok_or_else(|| BackendError::ParseError(format!("connection URI has no user: {uri}")))?;
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), p.to_string())
        }
        _ => (hostport.to_string(), "22".to_string()),
    };
    Ok(Some(ConnectionInfo {
        host,
        port,
        user: user.to_string(),
        identity,
    }))
}

impl CliBackend {
    fn image() -> String {
        base_image()
    }

    /// Reject a name any provider already runs an instance under. Names must be unique across the
    /// union of both providers, not per provider, because every other command addresses a sandbox
    /// by name alone (`docs/features/microsandbox-provider/02_dev.md`, Phase J).
    async fn ensure_name_available(&self, name: &str) -> Result<(), BackendError> {
        match self.list().await?.into_iter().find(|s| s.name == name) {
            Some(existing) => Err(BackendError::NameTaken {
                name: name.to_string(),
                provider: existing.provider,
            }),
            None => Ok(()),
        }
    }

    /// Runs `podman` without `--connection`, for system-connection registry operations
    /// that are not bound to the backend's active connection.
    async fn podman_unbound(&self, args: &[&str]) -> Result<Output, BackendError> {
        run_podman(&self.runner, args).await
    }

    /// Runs `podman --connection <connection> <args>` against an arbitrary connection. Used by
    /// deploy, whose target connection differs from the backend's pinned `self.connection`.
    async fn podman_on(&self, connection: &str, args: &[&str]) -> Result<Output, BackendError> {
        let mut full = vec!["--connection", connection];
        full.extend_from_slice(args);
        self.podman_unbound(&full).await
    }

    fn ensure_key(identity_path: &str) -> Result<(), BackendError> {
        if std::path::Path::new(identity_path).exists() {
            return Ok(());
        }
        let out = detached_command_std("ssh-keygen")
            .args(["-t", "ed25519", "-f", identity_path, "-N", ""])
            .output()
            .map_err(|e| BackendError::CommandFailed(format!("failed to run ssh-keygen: {e}")))?;
        check(out).map(|_| ())
    }

    /// Copies `<identity_path>.pub` into the container at the fixed path
    /// `sshd_config.container`'s `AuthorizedKeysFile` references, via the given provider so remote
    /// deploys use a target-pinned provider. For Podman the container need only exist, not be
    /// running (`podman cp` works against a created-but-stopped container); microsandbox's
    /// `copy_in` goes through `msb exec` and does need a running instance, which it always has,
    /// since `msb create` auto-starts what it creates.
    async fn install_sandbox_key(
        &self,
        provider: &Arc<dyn crate::provider::types::MachineProvider>,
        name: &str,
        identity_path: &str,
    ) -> Result<(), BackendError> {
        let pub_key = std::fs::read_to_string(format!("{identity_path}.pub"))
            .map_err(|e| BackendError::CommandFailed(format!("failed to read {identity_path}.pub: {e}")))?;
        let tmp_path = std::env::temp_dir().join(format!("sandbox-mio-authorized-keys-{name}"));
        std::fs::write(&tmp_path, &pub_key)
            .map_err(|e| BackendError::CommandFailed(format!("failed to write {tmp_path:?}: {e}")))?;
        let result = provider
            .copy_in(name, &tmp_path.to_string_lossy(), "/etc/ssh/authorized_keys.container")
            .await
            .map_err(BackendError::from);
        let _ = std::fs::remove_file(&tmp_path);
        result
    }

    async fn verify_host(&self, name: &str) -> Result<(), BackendError> {
        let out = self.podman_on(name, &["info", "--format", "json"]).await?;
        check(out).map(|_| ())
    }

    /// Live SSH endpoint for connection `name`, or `None` for a connection not reachable over SSH.
    /// Read fresh every call: a machine's SSH port changes on each boot.
    async fn remote_connection_info(
        runner: &Arc<dyn ProcessRunner>,
        name: &str,
    ) -> Result<Option<ConnectionInfo>, BackendError> {
        let out = run_podman(runner, &["system", "connection", "list", "--format", "json"]).await?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&check(out)?).map_err(|e| BackendError::ParseError(e.to_string()))?;
        let entry = entries
            .iter()
            .find(|e| e["Name"].as_str() == Some(name))
            .ok_or_else(|| BackendError::ParseError(format!("no podman connection named {name:?}")))?;
        parse_connection_entry(entry)
    }

    /// Create `dir` on the remote host over the connection's SSH identity. Used instead of the
    /// laptop-side `std::fs::create_dir_all` because a remote bind-mount source lives in the
    /// remote filesystem.
    async fn remote_mkdir(info: &ConnectionInfo, dir: &str) -> Result<(), BackendError> {
        let out = detached_command("ssh")
            .arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-i")
            .arg(&info.identity)
            .arg("-p")
            .arg(&info.port)
            .arg(format!("{}@{}", info.user, info.host))
            .arg(format!("mkdir -p '{dir}'"))
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        check(out).map(|_| ())
    }

    /// Resolve where a freshly created sandbox's workspace lives. `None` derives and creates a
    /// fresh directory under `workspace_root()` (today's behavior). `Some(p)` canonicalizes `p`
    /// via `dunce::canonicalize` (plain `std::fs::canonicalize` would return a Windows `\\?\`
    /// UNC-prefixed path that `podman -v` does not understand), rejects it if not found or not a
    /// directory, and rejects it as `PathAlreadyTracked` if it falls inside `workspace_root()` or
    /// exactly matches an already-tracked sandbox's workspace — without touching the filesystem
    /// further.
    /// Installs the sandbox's SSH key and writes its alias, after which the exec transport works. A
    /// recreated instance has a fresh host key, which `forget_sandbox_host_key` clears.
    async fn provision_exec_transport(
        &self,
        provider: &Arc<dyn crate::provider::types::MachineProvider>,
        name: &str,
        provider_id: ProviderId,
    ) -> Result<(), BackendError> {
        let identity_path = ensure_sandbox_identity(&self.app, name)?;
        forget_sandbox_host_key(&identity_path);
        tracing::info!(
            target: "backends",
            %name,
            provider = %provider_id,
            %identity_path,
            "installing sandbox ssh key"
        );
        // The key install is provider-neutral: both aliases tunnel to the base image's own
        // `mio-sshd`, so both need the pubkey at the path its `sshd_config.container` names.
        // Notably *not* `msb ssh authorize` for microsandbox — that authorizes against
        // microsandbox's own SSH server, which `ensure_msb_sandbox_alias` no longer uses.
        self.install_sandbox_key(provider, name, &identity_path).await?;
        match provider_id {
            ProviderId::Podman => ensure_sandbox_alias(name, &self.connection, &identity_path).await?,
            ProviderId::Microsandbox => ensure_msb_sandbox_alias(name, &identity_path).await?,
        }
        tracing::info!(target: "backends", %name, "ssh key installed and alias written");
        Ok(())
    }

    /// Settles a microsandbox sandbox's policy before `msb create`: loads the workspace file, seeds
    /// the starter allowlist when the dialog chose allowlist and the file has no rules, applies the
    /// dialog's mode (an imported workspace's file wins otherwise), validates, and snapshots it as
    /// `applied.json` (`docs/features/microsandbox-network-policy/02_dev.md`, Phase D).
    fn prepare_egress(
        &self,
        name: &str,
        workspace: &str,
        request: Option<&types::EgressRequest>,
        caps: crate::provider::types::ProviderCapabilities,
        chain: Option<crate::provider::types::ChainedProxy>,
    ) -> Result<crate::provider::types::EgressPolicy, BackendError> {
        use crate::net_policy;
        let mut policy = net_policy::load(workspace)?;
        let mode = request.and_then(|r| r.mode);
        if let Some(mode) = mode {
            if mode == net_policy::EgressMode::Allowlist && policy.rules.is_empty() {
                let starter = net_policy::starter(caps, &request.map(|r| r.starter_hosts.clone()).unwrap_or_default());
                policy.rules = starter.rules;
            }
            policy.egress = mode;
        }
        net_policy::validate(&policy, caps)?;
        if mode.is_some() {
            net_policy::save(workspace, &policy)?;
        }
        let applied = net_policy::AppliedPolicy { policy, chained_proxy: chain.is_some() };
        net_policy::write_applied(&self.app, name, &applied)?;
        Ok(net_policy::to_egress(&applied.policy, chain)?)
    }

    async fn resolve_create_workspace(
        &self,
        name: &str,
        existing_path: Option<&str>,
    ) -> Result<String, BackendError> {
        let Some(p) = existing_path else {
            // Pre-created so an unwritable path fails here, not silently via podman's
            // auto-creation of the mount source.
            let workspace = workspace_path(name)?;
            std::fs::create_dir_all(&workspace).map_err(|e| BackendError::WorkspaceCreate {
                path: workspace.clone(),
                error: e.to_string(),
            })?;
            return Ok(workspace);
        };
        let canonical = dunce::canonicalize(p)
            .map_err(|_| BackendError::PathNotFound { path: p.to_string() })?;
        if !canonical.is_dir() {
            return Err(BackendError::PathNotADirectory { path: p.to_string() });
        }
        let root = workspace_root()?;
        let canonical_root = dunce::canonicalize(&root).unwrap_or(root);
        if canonical.starts_with(&canonical_root) {
            return Err(BackendError::PathAlreadyTracked { path: p.to_string() });
        }
        let normalized = canonical.to_string_lossy().replace('\\', "/");
        if self.list().await?.iter().any(|s| s.workspace_path == normalized) {
            return Err(BackendError::PathAlreadyTracked { path: p.to_string() });
        }
        Ok(normalized)
    }

    /// Resolve where `name`'s workspace lives on `connection` and how it reaches the laptop. A
    /// local connection returns equal paths and a no-op strategy; a remote connection returns a
    /// remote bind-mount source and the live SSH endpoint. Called by `deploy` against the chosen
    /// target connection to compute the remote endpoint; `create` is always local.
    async fn resolve_workspace(
        &self,
        name: &str,
        connection: &str,
    ) -> Result<WorkspacePlan, BackendError> {
        let laptop_dir = workspace_path(name)?;
        let Some(info) = Self::remote_connection_info(&self.runner, connection).await? else {
            // Local machine: bind-mount source and laptop directory are the same, no sync.
            return Ok(WorkspacePlan {
                bind_mount_source: laptop_dir.clone(),
                laptop_dir,
                strategy: SyncStrategy::Local,
            });
        };
        // Root logs into `/root`; every other user into `/home/<user>`.
        let home = if info.user == "root" {
            "/root".to_string()
        } else {
            format!("/home/{}", info.user)
        };
        Ok(WorkspacePlan {
            bind_mount_source: format!("{home}/work/{name}"),
            laptop_dir,
            strategy: SyncStrategy::Remote(info),
        })
    }
}

/// Best-effort: a failed send (frontend dropped the channel) must not abort provisioning.
fn progress(channel: &Channel<serde_json::Value>, step: &str, status: &str, message: &str) {
    let _ = channel.send(serde_json::json!({
        "step": step,
        "status": status,
        "message": message,
    }));
}

/// Splits a proxy attachment into the Podman bridge (`network`, `env`) and the microsandbox chain.
/// A microsandbox attachment never maps onto `network`/`env`, which `MsbProvider` refuses.
fn split_attachment(
    name: &str,
    provider_id: ProviderId,
    proxy: Option<&crate::proxy::types::ProxyAttachment>,
) -> Result<
    (Option<(String, Vec<(String, String)>)>, Option<crate::provider::types::ChainedProxy>),
    BackendError,
> {
    use crate::proxy::types::ProxyAttachment;
    match proxy {
        None => Ok((None, None)),
        Some(ProxyAttachment::PodmanBridge { network, env }) if provider_id == ProviderId::Podman => {
            Ok((Some((network.clone(), env.clone())), None))
        }
        Some(ProxyAttachment::MsbChain { host_port, password_env, upstream_ca_path })
            if provider_id == ProviderId::Microsandbox =>
        {
            Ok((
                None,
                Some(crate::provider::types::ChainedProxy {
                    host_port: *host_port,
                    username: crate::provider::msb_policy::SOCKS_USERNAME.to_string(),
                    password_env: password_env.clone(),
                    upstream_ca_path: upstream_ca_path.clone(),
                }),
            ))
        }
        Some(_) => Err(BackendError::CommandFailed(format!(
            "{name:?}: the proxy attachment does not match the {provider_id} provider"
        ))),
    }
}

#[async_trait::async_trait]
impl SandboxBackend for CliBackend {
    async fn ensure_base_image(&self, provider: ProviderId) -> Result<(), BackendError> {
        Ok(self.registry.local(provider).ensure_base_image().await?)
    }

    async fn base_image_ready(&self, provider: ProviderId) -> Result<bool, BackendError> {
        Ok(self.registry.local(provider).base_image_ready().await?)
    }

    async fn create(
        &self,
        name: &str,
        existing_path: Option<&str>,
        provider_id: ProviderId,
        proxy: Option<&crate::proxy::types::ProxyAttachment>,
        egress: Option<&types::EgressRequest>,
    ) -> Result<Sandbox, BackendError> {
        validate_new_name(name)?;
        if egress.is_some() && !self.registry.local(provider_id).capabilities().egress_policy {
            return Err(BackendError::CommandFailed(format!(
                "{provider_id} sandboxes cannot enforce a network policy yet"
            )));
        }
        self.ensure_name_available(name).await?;
        let provider = self.registry.local(provider_id);
        let image = Self::image();
        // Every sandbox is created locally; remoteness is a later deploy action, so the workspace
        // is always a laptop directory — bind-mounted for Podman, Mutagen-synced into the disk
        // volume `MsbProvider::create_instance` attaches for microsandbox.
        let workspace = self.resolve_create_workspace(name, existing_path).await?;
        let (bridge, chain) = split_attachment(name, provider_id, proxy)?;
        let egress_policy = if provider_id == ProviderId::Microsandbox {
            Some(self.prepare_egress(name, &workspace, egress, provider.capabilities(), chain)?)
        } else {
            None
        };
        let spec = InstanceSpec {
            name: name.to_string(),
            image,
            workspace_source: workspace.clone(),
            labels: vec![(crate::provider::WORKSPACE_LABEL.to_string(), workspace.clone())],
            env: bridge.as_ref().map(|(_, env)| env.clone()).unwrap_or_default(),
            network: bridge.map(|(network, _)| crate::provider::types::InstanceNetwork {
                name: network,
                ip: None,
                dns: None,
            }),
            egress: egress_policy,
        };
        let id = match provider.create_instance(&spec).await {
            Ok(id) => id,
            Err(e) => {
                crate::net_policy::remove_applied(&self.app, name);
                return Err(e.into());
            }
        };
        self.provision_exec_transport(&provider, name, provider_id).await?;
        Ok(Sandbox {
            id,
            name: name.to_string(),
            status: "created".to_string(),
            workspace_path: workspace,
            image_version: Some(BASE_IMAGE_VERSION),
            provider: provider_id,
        })
    }

    async fn recreate(
        &self,
        name: &str,
        proxy: Option<&crate::proxy::types::ProxyAttachment>,
        egress: Option<&types::EgressRequest>,
    ) -> Result<Sandbox, BackendError> {
        let (provider_id, provider) = self.registry.provider_of(name).await;
        let current = self
            .list()
            .await?
            .into_iter()
            .find(|s| s.name == name)
            .ok_or_else(|| BackendError::CommandFailed(format!("no sandbox named {name:?}")))?;
        if provider_id != ProviderId::Microsandbox {
            // Podman recreate belongs to `docs/features/proxy-toggle-management/02_dev.md`.
            return Err(BackendError::CommandFailed(format!(
                "{name:?}: re-applying network settings is only supported for microsandbox sandboxes"
            )));
        }
        let workspace = current.workspace_path.clone();
        let (_, chain) = split_attachment(name, provider_id, proxy)?;
        // The snapshot is rewritten from the current workspace file, which is the point of a
        // re-apply (`docs/features/microsandbox-network-policy/02_dev.md`, Phase G).
        let egress_policy =
            self.prepare_egress(name, &workspace, egress, provider.capabilities(), chain)?;
        let spec = InstanceSpec {
            name: name.to_string(),
            image: Self::image(),
            workspace_source: workspace.clone(),
            labels: vec![(crate::provider::WORKSPACE_LABEL.to_string(), workspace.clone())],
            env: Vec::new(),
            network: None,
            egress: Some(egress_policy),
        };
        let id = provider.recreate_instance(&spec).await?;
        self.provision_exec_transport(&provider, name, provider_id).await?;
        Ok(Sandbox {
            id,
            name: name.to_string(),
            status: "running".to_string(),
            workspace_path: workspace,
            image_version: Some(BASE_IMAGE_VERSION),
            provider: provider_id,
        })
    }

    async fn list(&self) -> Result<Vec<Sandbox>, BackendError> {
        let mut all = Vec::new();
        for (id, provider) in self.registry.all() {
            match provider.list_instances().await {
                Ok(instances) => all.extend(instances),
                // Only Podman's failure is fatal: every install has Podman, but `msb` may not be
                // installed at all, and that must not blank out the Podman sandboxes as well.
                Err(e) if id == ProviderId::Podman => return Err(e.into()),
                Err(e) => {
                    tracing::debug!(target: "backends", provider = %id, error = %e, "skipping provider in list")
                }
            }
        }
        Ok(all)
    }

    async fn start(&self, name: &str) -> Result<(), BackendError> {
        Ok(self.registry.provider_of(name).await.1.start(name).await?)
    }

    async fn stop(&self, name: &str) -> Result<(), BackendError> {
        Ok(self.registry.provider_of(name).await.1.stop(name).await?)
    }

    async fn destroy(&self, name: &str) -> Result<(), BackendError> {
        Ok(self.registry.provider_of(name).await.1.remove(name).await?)
    }

    async fn deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<RemoteDeployment, BackendError> {
        validate_name(name)?;
        // Refused rather than silently deployed unproxied: deploy has no target-side proxy story
        // yet (`docs/features/network-proxy/02_dev.md`, Resolved Decision 6).
        if crate::proxy::exists(&self.app, &self.registry, name).await {
            return Err(BackendError::ProxyDeployUnsupported { name: name.to_string() });
        }
        // The deploy picker is already hidden for a provider without remote targets
        // (`ProviderCapabilities.remote_targets`); this is the backend half of that gate, so an
        // invoke that gets through anyway fails with a reason rather than nonsense podman argv.
        let owner = self.registry.provider_of(name).await.1;
        if !owner.capabilities().remote_targets {
            return Err(BackendError::RemoteTargetsUnsupported { name: name.to_string() });
        }
        // Deploying onto the source connection would name-collide with the kept-stopped local
        // container and is pointless; any other connection (including a different local machine)
        // is a valid target.
        if target_connection == self.connection {
            return Err(BackendError::SelfDeploy {
                name: name.to_string(),
                connection: target_connection.to_string(),
            });
        }
        // Resolve the remote endpoint first, so an unreachable target fails before any mutation of
        // the local container.
        let plan = self.resolve_workspace(name, target_connection).await?;
        let SyncStrategy::Remote(info) = plan.strategy else {
            return Err(BackendError::NoSshEndpoint {
                connection: target_connection.to_string(),
            });
        };

        // A second provider pinned to the target: `self.provider` targets the local connection,
        // but every remote-container operation below runs against `target_connection`.
        let target_provider =
            crate::provider::default_provider(self.runner.clone(), target_connection.to_string());

        // Stop the workload onto the remote; kept stopped (not removed) so undeploy can restart it.
        owner.stop(name).await?;
        // No committed-image transfer: software is reproduced from the mise manifest by
        // `RuntimeManager::reproduce_remote` after this returns. Must not be reintroduced.
        let image = Self::image();

        // Create the remote workspace and register the OpenSSH alias so Mutagen can reach it.
        Self::remote_mkdir(&info, &plan.bind_mount_source).await?;
        crate::ssh::write_alias(&crate::ssh::types::SshAlias::Direct {
            alias: target_connection.to_string(),
            host: info.host.clone(),
            port: info.port.clone(),
            user: info.user.clone(),
            identity: info.identity.clone(),
        })?;

        // Ensure the base image exists on the remote. It is never published to any registry, so
        // absence means build — the same path `ensure_base_image` takes for the local connection.
        target_provider.ensure_base_image().await?;

        // Create and start the remote container on the target connection, not self.connection.
        // No workspace label on remote containers, matching prior behaviour.
        let spec = InstanceSpec {
            name: name.to_string(),
            image: image.clone(),
            workspace_source: plan.bind_mount_source.clone(),
            labels: Vec::new(),
            env: Vec::new(),
            network: None,
            egress: None,
        };
        target_provider.create_instance(&spec).await?;
        target_provider.start(name).await?;
        let identity_path = ensure_sandbox_identity(&self.app, name)?;
        forget_sandbox_host_key(&identity_path);
        self.install_sandbox_key(&target_provider, name, &identity_path).await?;
        ensure_sandbox_alias(name, target_connection, &identity_path).await?;

        Ok(RemoteDeployment {
            alpha: plan.laptop_dir,
            beta: format!("{target_connection}:{}", plan.bind_mount_source),
        })
    }

    async fn remove_remote(&self, name: &str, connection: &str) -> Result<(), BackendError> {
        let target_provider =
            crate::provider::default_provider(self.runner.clone(), connection.to_string());
        Ok(target_provider.remove(name).await?)
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), BackendError> {
        // `ProcessRunner` (`src/mockctl/mod.rs`) models one-shot calls only, so the scripted-runner
        // seam cannot stand in for a streamed child: this gate is what keeps the light tier off podman.
        if mock_backend_enabled() {
            return Ok(());
        }

        let mut child = podman_command()
            .arg("--connection")
            .arg(&self.connection)
            .args(["events", "--format", "json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // Ends `podman events` on every return path, including a superseded stream's.
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;

        // Dropping the previous sender wakes the earlier stream's `stop` branch. Only the main
        // window subscribes, so a second call means that window reloaded.
        let (stop_tx, mut stop) = tokio::sync::oneshot::channel::<()>();
        drop(
            self.events_stop
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .replace(stop_tx),
        );

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| BackendError::CommandFailed("no stdout".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| BackendError::CommandFailed("no stderr".to_string()))?;
        // Drained on its own task rather than left inherited: this call runs unconditionally at app
        // startup (`ensureActivity` in `src/routes/+layout.svelte`), before any Podman-readiness
        // check, so a connection-not-found failure must not leak raw podman stderr to the app's own
        // console — it has to come back as a `BackendError` the caller can swallow.
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            let _ = tokio::io::AsyncReadExt::read_to_string(&mut BufReader::new(stderr), &mut buf).await;
            buf
        });

        let mut lines = BufReader::new(stdout).lines();
        loop {
            let line = tokio::select! {
                line = lines.next_line() => line.map_err(|e| BackendError::CommandFailed(e.to_string()))?,
                _ = &mut stop => return Ok(()),
            };
            let Some(line) = line else { break };
            if line.trim().is_empty() {
                continue;
            }
            let value: serde_json::Value =
                serde_json::from_str(&line).map_err(|e| BackendError::ParseError(e.to_string()))?;
            on_event
                .send(value)
                .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        }

        let status = child.wait().await.map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        if !status.success() {
            let stderr = stderr_task.await.unwrap_or_default();
            return Err(BackendError::CommandFailed(stderr.trim().to_string()));
        }
        Ok(())
    }

    async fn list_hosts(&self) -> Result<Vec<String>, BackendError> {
        let out = self.podman_unbound(&["system", "connection", "list", "--format", "json"]).await?;
        let stdout = check(out)?;
        let entries: Vec<serde_json::Value> =
            serde_json::from_str(&stdout).map_err(|e| BackendError::ParseError(e.to_string()))?;
        Ok(entries
            .into_iter()
            .filter_map(|e| e["Name"].as_str().map(|s| s.to_string()))
            .collect())
    }

    async fn add_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity: Option<&str>,
    ) -> Result<(), BackendError> {
        let mut args = vec!["system", "connection", "add"];
        if let Some(identity) = identity {
            args.push("--identity");
            args.push(identity);
        }
        args.push(name);
        args.push(ssh_url);
        check(self.podman_unbound(&args).await?).map(|_| ())
    }

    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), BackendError> {
        let (target, port) = ssh_target(ssh_url)?;
        let mut cmd = detached_command("ssh");
        // BatchMode=yes turns a missing key authorization into a fast failure instead of a
        // password prompt that would hang headless — authorizing the key is a manual
        // prerequisite (`ssh` reads passwords from the TTY, not stdin).
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg("-i").arg(identity_path);
        if let Some(port) = port {
            cmd.arg("-p").arg(port);
        }
        cmd.arg(&target);
        // Linger must be enabled (as root) *before* `systemctl --user`, since it is what
        // starts the per-user systemd manager and creates its D-Bus socket; the tutorial's
        // reverse order fails on a host with no active login session. On hosts without
        // passwordless sudo this fails under BatchMode and the user runs it by hand.
        cmd.arg(
            "sudo loginctl enable-linger \"$USER\" \
             && XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user enable --now podman.socket",
        );
        let out = cmd
            .output()
            .await
            .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
        check(out).map(|_| ())
    }

    async fn provision_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity_path: &str,
        on_progress: Channel<serde_json::Value>,
    ) -> Result<(), BackendError> {
        let step = |label: &str, result: Result<(), BackendError>| -> Result<(), BackendError> {
            match &result {
                Ok(()) => progress(&on_progress, label, "ok", ""),
                Err(e) => progress(&on_progress, label, "error", &e.to_string()),
            }
            result
        };

        step("ensure_key", Self::ensure_key(identity_path))?;
        step(
            "prepare_host",
            self.prepare_host(ssh_url, identity_path).await,
        )?;
        step(
            "add_host",
            self.add_host(name, ssh_url, Some(identity_path)).await,
        )?;
        step("verify_host", self.verify_host(name).await)?;
        progress(&on_progress, "done", "ok", "");
        Ok(())
    }
}

/// Per-sandbox ed25519 keypair for the exec-transport `Proxied` alias, generated once and reused
/// for the sandbox's lifetime. Lives under app data (not `~/.ssh`), per-sandbox and
/// per-worktree-isolatable via `MIO_APP_DATA_DIR`. Returns the private key path. Free function
/// (not a `CliBackend` method) so `SandboxRouter` (`src-tauri/src/services/routing.rs`) can
/// re-derive an already-created sandbox's identity path deterministically from its name,
/// without persisting the path anywhere new.
pub fn ensure_sandbox_identity(
    app: &tauri::AppHandle,
    name: &str,
) -> Result<String, BackendError> {
    let dir = crate::paths::sandbox_key_dir(app, name)
        .map_err(|e| BackendError::CommandFailed(e.to_string()))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| BackendError::CommandFailed(format!("failed to create {dir:?}: {e}")))?;
    let identity_path = dir.join("id_ed25519").to_string_lossy().replace('\\', "/");
    CliBackend::ensure_key(&identity_path)?;
    Ok(identity_path)
}

/// Ensure the SSH alias for a Podman sandbox is written to `~/.ssh/sandbox-mio/config`.
/// The identity and installed key last the sandbox's lifetime, but the alias is rewritten on every
/// terminal, agent-web and expose-port open because the alias file may not survive an app restart.
pub async fn ensure_sandbox_alias(
    name: &str,
    connection: &str,
    identity_path: &str,
) -> Result<(), BackendError> {
    // `mio-sshd` is referenced by bare name (it lives on the container's default `PATH` via
    // `/usr/local/bin`, baked in the Containerfile) rather than by its `/usr/local/bin/...` absolute
    // path, so the argument carries no MSYS-manglable token. This is load-bearing, not cosmetic:
    // Mutagen forwarding runs in the long-lived daemon, which spawns the `ssh → sh -c "exec …"`
    // transport — the daemon "outlives the app" (`crate::sync::sweep_stale_forwards`) and is only
    // env-configured by whichever CLI call happens to auto-start it, so the `MSYS_NO_PATHCONV`
    // suppression set on `crate::sync::command` does not reach a pre-existing daemon. A bare name
    // is correct regardless of daemon provenance. See `docs/features/podman-exec-transport/02_dev.md`,
    // Resolved Decision 5.
    let command = format!(
        "{} --connection {connection} exec -i {name} mio-sshd",
        podman_literal_path()
    );
    crate::ssh::write_alias(&crate::ssh::types::SshAlias::Proxied {
        alias: name.to_string(),
        command,
        user: CONTAINER_USER.to_string(),
        identity: identity_path.to_string(),
        known_hosts: sandbox_known_hosts_path(identity_path),
    })?;
    Ok(())
}

/// Guest-side mount point for a microsandbox sandbox's workspace volume. Kept as its own literal
/// (matching the crate's existing convention — `runtime::WORKSPACE_MOUNT` and `provider::
/// WORKSPACE_MOUNT` each keep a private copy too) rather than importing `provider`'s, since it's a
/// guest-path fact any module may need, not shared mutable state.
const MSB_WORKSPACE_MOUNT: &str = "/root/work";

/// The literal argv token for `msb`, for embedding in an OpenSSH `ProxyCommand` string — the
/// counterpart of [`podman_literal_path`], resolved by the same [`crate::process::msb_executable`]
/// as every other `msb` call so the transport spawns the bundled binary too.
fn msb_literal_path() -> String {
    let exe = crate::process::msb_executable();
    format!("\"{}\"", exe.to_string_lossy().replace('\\', "/"))
}

/// The `ProxyCommand` [`ensure_msb_sandbox_alias`] pins, split out from the alias write so the
/// transport choice is assertable without touching `~/.ssh`.
fn msb_proxy_command(name: &str) -> String {
    format!("{} exec --stream {name} -- mio-sshd", msb_literal_path())
}

/// Ensure the SSH alias for a microsandbox sandbox is written to `~/.ssh/sandbox-mio/config`.
/// Mirrors [`ensure_sandbox_alias`]'s Podman/exec-transport counterpart exactly, down to execing the
/// image's own `mio-sshd` — only the exec substrate differs (`msb exec --stream` in place of `podman
/// exec -i`; both are byte-faithful piped stdio with no PTY).
///
/// Tunnelling to `mio-sshd` rather than to microsandbox's own `msb ssh serve <name> --stdio` server
/// is load-bearing, not stylistic. That server answers exec and pty channels but silently drops a
/// `subsystem sftp` request, and Mutagen installs its remote agent with `scp`, which speaks the SFTP
/// protocol on OpenSSH 9+ — so every sync against a microsandbox sandbox died with `unable to
/// install agent: unable to copy agent binary: unable to run SCP process: exit status 1` while a
/// plain `ssh <alias> <cmd>` worked fine. The base image's `sshd` already configures `Subsystem sftp`
/// (`src-tauri/resources/Containerfile`), and microsandbox boots that same image, so routing through
/// it fixes `scp` and costs nothing (confirmed hands-on against real `msb` 0.6.6: `ssh` and `scp`
/// both exit 0 over this ProxyCommand, `scp` exits 1 over `msb ssh serve`).
///
/// `mio-sshd` is referenced by bare name and the bundled `msb` path is forward-slashed, so the
/// command carries no MSYS-manglable token — see [`ensure_sandbox_alias`] on why that matters for a
/// pre-existing Mutagen daemon.
pub async fn ensure_msb_sandbox_alias(name: &str, identity_path: &str) -> Result<(), BackendError> {
    let command = msb_proxy_command(name);
    // The exact `ProxyCommand` OpenSSH will run, logged verbatim: the transport is the one part of
    // the microsandbox path no Rust-side error can describe, since every failure in it surfaces as
    // an opaque `scp`/`ssh` exit code from inside Mutagen.
    tracing::info!(target: "backends", %name, %command, %identity_path, "writing microsandbox ssh alias");
    crate::ssh::write_alias(&crate::ssh::types::SshAlias::Proxied {
        alias: name.to_string(),
        command,
        user: CONTAINER_USER.to_string(),
        identity: identity_path.to_string(),
        known_hosts: sandbox_known_hosts_path(identity_path),
    })?;
    Ok(())
}

/// The Mutagen `beta` scp-style URL for syncing a microsandbox sandbox's workspace volume. No
/// host/port/user: the alias named `name` (written by [`ensure_msb_sandbox_alias`]) resolves the
/// full connection through its `ProxyCommand`, exactly like the exec-transport `Proxied` alias's
/// beta URL in `deploy` needs none beyond the alias name (`docs/features/microsandbox-provider/
/// 02_dev.md`, Phase H).
pub fn msb_sync_beta(name: &str) -> String {
    format!("{name}:{MSB_WORKSPACE_MOUNT}")
}

/// Mutagen session name for a microsandbox sandbox's workspace sync. Deliberately *not* the bare
/// sandbox name: a session under that name means "deployed to a remote host" everywhere else
/// (`SessionInfo`, `sandbox_deployments`), and a microsandbox sandbox is never deployed — its
/// session only keeps the disk volume in step with the laptop directory.
pub fn msb_sync_session(name: &str) -> String {
    format!("workspace-{name}")
}

/// The `UserKnownHostsFile` [`ensure_sandbox_alias`] pins, kept next to the sandbox's own identity.
fn sandbox_known_hosts_path(identity_path: &str) -> String {
    match identity_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/known_hosts"),
        None => "known_hosts".to_string(),
    }
}

/// Drop the host key pinned for a sandbox alias. Must run whenever a *fresh* container is created
/// under a name that may have been used before: the sshd host key is baked into the base image, so
/// it changes across an image rebuild while the alias name stays the same.
fn forget_sandbox_host_key(identity_path: &str) {
    let _ = std::fs::remove_file(sandbox_known_hosts_path(identity_path));
}

#[cfg(test)]
mod msb_sync_tests {
    use super::*;

    #[test]
    fn msb_sync_beta_has_no_host_or_port() {
        assert_eq!(msb_sync_beta("my-sandbox"), "my-sandbox:/root/work");
    }

    /// The transport must tunnel to the base image's own `mio-sshd`, never to microsandbox's
    /// `msb ssh serve` server: that server drops the `sftp` subsystem request, which breaks the
    /// `scp` Mutagen installs its agent with, while leaving a plain `ssh <alias> <cmd>` working —
    /// so nothing but a sync exercises the difference. See [`ensure_msb_sandbox_alias`].
    #[test]
    fn msb_proxy_command_tunnels_to_the_image_sshd() {
        let command = msb_proxy_command("my-sandbox");
        assert!(command.ends_with(" exec --stream my-sandbox -- mio-sshd"), "{command}");
        assert!(!command.contains("ssh serve"), "{command}");
    }
}

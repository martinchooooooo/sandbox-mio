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
    BackendError, CliBackend, ConnectionInfo, DeployPlan, RemoteDeployment, Sandbox, SandboxBackend,
    SyncStrategy, WorkspacePlan,
};

// `e2e/lib/podman-machine.ts` regex-parses the two declarations below and `DEFAULT_MACHINE_NAME`,
// so their literal form (name, type, `const`) is load-bearing beyond Rust.
const BASE_IMAGE: &str = "sandbox-mio/base";
/// Bump whenever the Containerfile changes: `ensure_base_image` short-circuits on an existing
/// tag, so an unchanged version silently keeps stale images.
pub(crate) const BASE_IMAGE_VERSION: u32 = 10;

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

/// Podman 4.0 is the first release taking several `--network` flags with `name:ip=`, which the
/// dual-homed sidecar needs; netavark is the backend proven by the Phase A spike
/// (`docs/features/proxy-remote-deploy/02_dev.md`, Resolved Decision 6).
const MIN_REMOTE_PODMAN_MAJOR: u64 = 4;

/// Whether the target's `podman info --format json` describes a host that can run the bounded
/// sandbox, or the reason it cannot. Pure so the floor is unit-tested.
fn check_remote_podman(connection: &str, info_json: &str) -> Result<(), BackendError> {
    let unsupported = |reason: String| BackendError::RemoteHostUnsupported {
        connection: connection.to_string(),
        reason,
    };
    let info: serde_json::Value = serde_json::from_str(info_json)
        .map_err(|e| BackendError::ParseError(format!("podman info on {connection:?}: {e}")))?;
    let version = info["version"]["Version"].as_str().unwrap_or_default();
    let major: u64 = version.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
    if major < MIN_REMOTE_PODMAN_MAJOR {
        return Err(unsupported(format!(
            "Podman {version:?} is older than {MIN_REMOTE_PODMAN_MAJOR}.0"
        )));
    }
    let backend = info["host"]["networkBackend"].as_str().unwrap_or_default();
    if backend != "netavark" {
        return Err(unsupported(format!(
            "its network backend is {backend:?}, and the egress boundary needs netavark"
        )));
    }
    Ok(())
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
    /// union of providers, not per provider, because every other command addresses a sandbox by
    /// name alone.
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
    /// deploys use a target-pinned provider. The container need only exist, not be running
    /// (`podman cp` works against a created-but-stopped container).
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
        // The alias tunnels to the base image's own `mio-sshd`, so the pubkey must sit at the path
        // its `sshd_config.container` names.
        self.install_sandbox_key(provider, name, &identity_path).await?;
        // `podman create` leaves the container stopped, which `exec` cannot reach, so the host key
        // is pinned on the first start instead (`services::ssh_access::spawn_pin_host_keys`).
        match provider_id {
            ProviderId::Podman => ensure_sandbox_alias(name, &self.connection, &identity_path).await?,
        }
        tracing::info!(target: "backends", %name, "ssh key installed and alias written");
        Ok(())
    }

    /// Settles the accepted policy from the create dialog alone; an existing workspace file stays a
    /// proposal and only its binding variables get placeholders
    /// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 5).
    fn prepare_egress(
        &self,
        name: &str,
        workspace: &str,
        request: Option<&types::EgressRequest>,
        caps: crate::provider::types::ProviderCapabilities,
    ) -> Result<(crate::provider::types::EgressPolicy, Vec<String>), BackendError> {
        use crate::net_policy;
        let mut policy = net_policy::NetPolicy::default();
        if let Some(mode) = request.and_then(|r| r.mode) {
            if mode == net_policy::EgressMode::Allowlist {
                let hosts = request.map(|r| r.starter_hosts.clone()).unwrap_or_default();
                policy.rules = net_policy::starter(caps, &hosts).rules;
            }
            policy.egress = mode;
        }
        net_policy::validate(&policy, caps)?;
        // An unparsable file is left alone; the network section reports it after create.
        let (guest_vars, write_file) = match net_policy::load_if_present(workspace) {
            Ok(Some(file)) => (net_policy::stampable_vars(&file), false),
            Ok(None) => (Vec::new(), true),
            Err(_) => (Vec::new(), false),
        };
        if write_file {
            net_policy::save(workspace, &policy)?;
        }
        let guest_env = caps.sidecar_egress.then(|| guest_vars.clone());
        let applied = net_policy::AppliedPolicy { policy, revision: 0, guest_env };
        net_policy::write_applied(&self.app, name, &applied)?;
        Ok((net_policy::to_egress(&applied.policy)?, guest_vars))
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

/// A Podman bridge attachment: network, sandbox environment, and DNS server.
type Bridge = (String, Vec<(String, String)>, String);

/// Reduces a proxy attachment to the Podman bridge. A provider with its own attachment shape
/// adds a variant to `ProxyAttachment` and an arm here.
fn split_attachment(proxy: Option<&crate::proxy::types::ProxyAttachment>) -> Option<Bridge> {
    use crate::proxy::types::ProxyAttachment;
    match proxy {
        None => None,
        Some(ProxyAttachment::PodmanBridge { network, env, dns }) => {
            Some((network.clone(), env.clone(), dns.clone()))
        }
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
        let caps = self.registry.local(provider_id).capabilities();
        let bridge = split_attachment(proxy);
        // A Podman sidecar enforces the policy only for a sandbox behind it
        // (`docs/features/podman-egress-boundary/02_dev.md`, Resolved Decision 14).
        let sidecar_enforced = caps.sidecar_egress && bridge.is_some();
        if egress.is_some() && !caps.egress_policy && !sidecar_enforced {
            return Err(BackendError::CommandFailed(format!(
                "{provider_id} sandboxes cannot enforce a network policy without the proxy"
            )));
        }
        self.ensure_name_available(name).await?;
        let provider = self.registry.local(provider_id);
        let image = Self::image();
        // Every sandbox is created locally; remoteness is a later deploy action, so the workspace
        // is always a laptop directory.
        let workspace = self.resolve_create_workspace(name, existing_path).await?;
        let mut env = bridge.as_ref().map(|(_, env, _)| env.clone()).unwrap_or_default();
        let egress_policy = if caps.egress_policy {
            Some(self.prepare_egress(name, &workspace, egress, caps)?.0)
        } else if sidecar_enforced {
            // Runs even with no request, so `applied.json` always exists; the sidecar, not the
            // engine, enforces the result. Placeholders exist only from create (Resolved Decision 7).
            let (_, guest_vars) = self.prepare_egress(name, &workspace, egress, caps)?;
            env.extend(guest_vars.iter().map(|v| (v.clone(), crate::net_policy::placeholder(v))));
            None
        } else {
            None
        };
        let spec = InstanceSpec {
            name: name.to_string(),
            image,
            workspace_source: workspace.clone(),
            labels: vec![(crate::provider::WORKSPACE_LABEL.to_string(), workspace.clone())],
            env,
            network: bridge.map(|(network, _, dns)| crate::provider::types::InstanceNetwork {
                name: network,
                ip: None,
                dns: Some(dns),
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
        _proxy: Option<&crate::proxy::types::ProxyAttachment>,
        _egress: Option<&types::EgressRequest>,
    ) -> Result<Sandbox, BackendError> {
        // Podman recreate belongs to `docs/features/proxy-toggle-management/02_dev.md`.
        Err(BackendError::CommandFailed(format!(
            "{name:?}: recreating a sandbox is not supported yet"
        )))
    }

    async fn list(&self) -> Result<Vec<Sandbox>, BackendError> {
        let mut all = Vec::new();
        for (id, provider) in self.registry.all() {
            match provider.list_instances().await {
                Ok(instances) => all.extend(instances),
                // Podman's failure is fatal: every install has it. Another provider's may not be
                // installed at all, and must not blank out the sandboxes of the rest.
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

    async fn plan_deploy(
        &self,
        name: &str,
        target_connection: &str,
    ) -> Result<DeployPlan, BackendError> {
        validate_name(name)?;
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
        if !matches!(plan.strategy, SyncStrategy::Remote(_)) {
            return Err(BackendError::NoSshEndpoint { connection: target_connection.to_string() });
        }
        let info = check(self.podman_on(target_connection, &["info", "--format", "json"]).await?)?;
        check_remote_podman(target_connection, &info)?;
        Ok(DeployPlan { target: target_connection.to_string(), workspace: plan })
    }

    async fn deploy(
        &self,
        name: &str,
        deploy_plan: &DeployPlan,
        proxy: &crate::proxy::types::ProxyAttachment,
        extra_env: &[(String, String)],
    ) -> Result<RemoteDeployment, BackendError> {
        let target_connection = deploy_plan.target.as_str();
        let plan = &deploy_plan.workspace;
        let SyncStrategy::Remote(info) = &plan.strategy else {
            return Err(BackendError::NoSshEndpoint { connection: target_connection.to_string() });
        };
        let owner = self.registry.provider_of(name).await.1;

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
        Self::remote_mkdir(info, &plan.bind_mount_source).await?;
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
        let crate::proxy::types::ProxyAttachment::PodmanBridge { network, env: bridge_env, dns } = proxy;
        let mut env = bridge_env.clone();
        env.extend(extra_env.iter().cloned());
        let spec = InstanceSpec {
            name: name.to_string(),
            image: image.clone(),
            workspace_source: plan.bind_mount_source.clone(),
            labels: Vec::new(),
            env,
            network: Some(crate::provider::types::InstanceNetwork {
                name: network.clone(),
                ip: None,
                dns: Some(dns.clone()),
            }),
            egress: None,
        };
        target_provider.create_instance(&spec).await?;
        target_provider.start(name).await?;
        let identity_path = ensure_sandbox_identity(&self.app, name)?;
        forget_sandbox_host_key(&identity_path);
        self.install_sandbox_key(&target_provider, name, &identity_path).await?;
        seed_sandbox_host_keys(&target_provider, name, &identity_path).await;
        ensure_sandbox_alias(name, target_connection, &identity_path).await?;

        Ok(RemoteDeployment {
            alpha: plan.laptop_dir.clone(),
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
    let command = podman_proxy_command(name, connection);
    crate::ssh::write_alias(&crate::ssh::types::SshAlias::Proxied {
        alias: name.to_string(),
        command,
        user: CONTAINER_USER.to_string(),
        identity: identity_path.to_string(),
        known_hosts: sandbox_known_hosts_path(identity_path),
    })?;
    Ok(())
}

/// The `ProxyCommand` a sandbox's own alias pins on `machine`. The single source for both the
/// alias writes and the string `sandbox_ssh_access` shows the user, so the two cannot diverge.
pub fn sandbox_proxy_command(name: &str, machine: &crate::provider::types::MachineRef) -> String {
    match machine.provider {
        ProviderId::Podman => podman_proxy_command(name, &machine.id),
    }
}

/// The `ProxyCommand` [`ensure_sandbox_alias`] pins for a Podman sandbox on `connection`.
fn podman_proxy_command(name: &str, connection: &str) -> String {
    // Bare `mio-sshd`, never its absolute path: a pre-existing Mutagen daemon lacks `MSYS_NO_PATHCONV`
    // and would mangle it (`docs/features/podman-exec-transport/02_dev.md`, Resolved Decision 5).
    format!(
        "{} --connection {connection} exec -i {name} mio-sshd",
        podman_literal_path()
    )
}

/// The `UserKnownHostsFile` [`ensure_sandbox_alias`] pins, kept next to the sandbox's own identity.
pub(crate) fn sandbox_known_hosts_path(identity_path: &str) -> String {
    match identity_path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/known_hosts"),
        None => "known_hosts".to_string(),
    }
}

/// Drop the host key pinned for a sandbox alias. Must run whenever a *fresh* container is created
/// under a name that may have been used before: the sshd host key is baked into the base image, so
/// it changes across an image rebuild while the alias name stays the same. The file stays empty
/// until [`seed_sandbox_host_keys`] next reaches the container running.
fn forget_sandbox_host_key(identity_path: &str) {
    let _ = std::fs::remove_file(sandbox_known_hosts_path(identity_path));
}

/// Renders the pinned `known_hosts` from the host `.pub` files, dropping any line that is not a key.
/// The `[<alias>]:*` pattern matches the port some clients carry, meaningless over a `ProxyCommand`.
pub(crate) fn render_known_hosts(alias: &str, pub_keys: &str) -> String {
    let mut out = String::new();
    for line in pub_keys.lines() {
        let mut fields = line.split_whitespace();
        let (Some(kind), Some(blob)) = (fields.next(), fields.next()) else {
            continue;
        };
        let is_key_type = kind.starts_with("ssh-") || kind.starts_with("ecdsa-");
        let is_base64 = blob.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
        if is_key_type && is_base64 {
            out.push_str(&format!("{alias},[{alias}]:* {kind} {blob}\n"));
        }
    }
    out
}

/// Pins the running container's host keys so a strict-checking client connects on first contact
/// (`docs/features/host-tool-ssh-access/02_dev.md`, Resolved Decisions). Best effort: a failure is
/// warn-logged and must never fail a create, deploy or panel fetch.
pub(crate) async fn seed_sandbox_host_keys(
    provider: &Arc<dyn crate::provider::types::MachineProvider>,
    name: &str,
    identity_path: &str,
) {
    let out = provider
        .exec(
            name,
            &["sh", "-c", "cat /etc/ssh/ssh_host_*_key.pub"],
            crate::provider::types::ExecOpts::default(),
        )
        .await;
    let out = match out {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            tracing::warn!(target: "backends", %name, code = out.status.code().unwrap_or(-1), "reading host keys failed; not pinning");
            return;
        }
        Err(e) => {
            tracing::warn!(target: "backends", %name, error = %e, "reading host keys failed; not pinning");
            return;
        }
    };
    let rendered = render_known_hosts(name, &String::from_utf8_lossy(&out.stdout));
    if rendered.is_empty() {
        tracing::warn!(target: "backends", %name, "container returned no host keys; not pinning");
        return;
    }
    // A start's background pin and the panel's re-fetch seed concurrently; sharing the temp file
    // unlocked, one could rename the other's half-written copy into place.
    static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = std::path::PathBuf::from(sandbox_known_hosts_path(identity_path));
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == rendered) {
        return;
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &rendered).and_then(|()| std::fs::rename(&tmp, &path)) {
        tracing::warn!(target: "backends", %name, error = %e, "writing pinned host keys failed");
        let _ = std::fs::remove_file(&tmp);
    }
}

#[cfg(test)]
mod known_hosts_tests {
    use super::*;

    const ED: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO root@abc";
    const RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQAB+/== root@abc";
    const EC: &str = "ecdsa-sha2-nistp256 AAAAE2VjZHNh root@abc";

    #[test]
    fn every_key_gets_the_port_agnostic_host_pattern() {
        let out = render_known_hosts("box", &format!("{EC}\n{ED}\n{RSA}\n"));
        assert_eq!(
            out,
            "box,[box]:* ecdsa-sha2-nistp256 AAAAE2VjZHNh\n\
             box,[box]:* ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO\n\
             box,[box]:* ssh-rsa AAAAB3NzaC1yc2EAAAADAQAB+/==\n"
        );
    }

    #[test]
    fn empty_or_garbage_output_renders_nothing() {
        assert_eq!(render_known_hosts("box", ""), "");
        assert_eq!(render_known_hosts("box", "cat: /etc/ssh/ssh_host_*_key.pub: No such file\n"), "");
        assert_eq!(render_known_hosts("box", "\n\n not a key\n"), "");
    }

    #[test]
    fn garbage_lines_are_dropped_among_real_keys() {
        let out = render_known_hosts("box", &format!("noise\r\n{ED}\r\n"));
        assert_eq!(out, "box,[box]:* ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIELO\n");
    }
}

#[cfg(test)]
mod proxy_command_tests {
    use super::*;

    fn machine(provider: ProviderId, id: &str) -> crate::provider::types::MachineRef {
        crate::provider::types::MachineRef { provider, id: id.to_string() }
    }

    /// The `--connection` value is what a deploy or undeploy changes, so it must come from the
    /// machine the route resolved rather than from the local default.
    #[test]
    fn podman_proxy_command_execs_on_the_route_machine() {
        let command = sandbox_proxy_command("my-sandbox", &machine(ProviderId::Podman, "remote-box"));
        assert!(
            command.ends_with(" --connection remote-box exec -i my-sandbox mio-sshd"),
            "{command}"
        );
        assert_eq!(command, podman_proxy_command("my-sandbox", "remote-box"));
    }

    /// A bare `mio-sshd` keeps the argument free of MSYS-manglable absolute paths.
    #[test]
    fn podman_proxy_command_names_mio_sshd_bare() {
        let command = podman_proxy_command("my-sandbox", "podman-machine-default");
        assert!(!command.contains("/usr/local/bin"), "{command}");
    }

}

#[cfg(test)]
mod remote_podman_tests {
    use super::*;

    fn info(version: &str, backend: &str) -> String {
        format!(r#"{{"version":{{"Version":"{version}"}},"host":{{"networkBackend":"{backend}"}}}}"#)
    }

    #[test]
    fn netavark_podman_4_or_newer_is_accepted() {
        assert!(check_remote_podman("t", &info("4.0.0", "netavark")).is_ok());
        assert!(check_remote_podman("t", &info("5.8.6", "netavark")).is_ok());
    }

    #[test]
    fn an_old_podman_or_cni_is_refused_with_a_reason() {
        for bad in [info("3.4.4", "netavark"), info("5.0.0", "cni"), "{}".to_string()] {
            assert!(matches!(
                check_remote_podman("t", &bad),
                Err(BackendError::RemoteHostUnsupported { .. })
            ));
        }
    }
}

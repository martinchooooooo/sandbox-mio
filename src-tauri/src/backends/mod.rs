mod hosts;
pub mod image;
pub mod types;

use std::process::{Output, Stdio};
use std::sync::Arc;

use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, BufReader};

use mio_domain::mock_backend_enabled;
use mio_domain::types::{BackendError, ProviderId, Sandbox};
use mio_process::detached_command;
use mio_process::podman::{connection_name, podman_command};
use mio_process::types::ProcessRunner;
use mio_provider::image::{base_image, BASE_IMAGE_VERSION};
use mio_provider::types::InstanceSpec;
use mio_provider::ProviderRegistry;
use crate::ssh;
use types::{
    CliBackend, ConnectionInfo, DeployPlan, RemoteDeployment, SandboxBackend, SyncStrategy,
    WorkspacePlan,
};

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

/// Runs `podman --connection <connection> <args>` against an arbitrary connection. Used by
/// deploy and host verification, whose target connection differs from the backend's pinned
/// `self.connection`.
async fn podman_on(
    runner: &Arc<dyn ProcessRunner>,
    connection: &str,
    args: &[&str],
) -> Result<Output, BackendError> {
    let mut full = vec!["--connection", connection];
    full.extend_from_slice(args);
    run_podman(runner, &full).await
}

/// Reads the running container's host keys for `ssh::identity::pin_host_keys`, which takes no
/// provider. Best effort: a failure is warn-logged and never fails a create, deploy or panel fetch.
pub(crate) async fn seed_sandbox_host_keys(
    provider: &Arc<dyn mio_provider::types::MachineProvider>,
    name: &str,
    identity_path: &str,
) {
    let out = provider
        .exec(
            name,
            &["sh", "-c", "cat /etc/ssh/ssh_host_*_key.pub"],
            mio_provider::types::ExecOpts::default(),
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
    ssh::identity::pin_host_keys(name, identity_path, &String::from_utf8_lossy(&out.stdout));
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

    /// Copies `<identity_path>.pub` into the container at the fixed path
    /// `sshd_config.container`'s `AuthorizedKeysFile` references, via the given provider so remote
    /// deploys use a target-pinned provider. The container need only exist, not be running
    /// (`podman cp` works against a created-but-stopped container).
    async fn install_sandbox_key(
        &self,
        provider: &Arc<dyn mio_provider::types::MachineProvider>,
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

    /// Installs the sandbox's SSH key and writes its alias, after which the exec transport works. A
    /// recreated instance has a fresh host key, which `forget_sandbox_host_key` clears.
    async fn provision_exec_transport(
        &self,
        provider: &Arc<dyn mio_provider::types::MachineProvider>,
        name: &str,
        provider_id: ProviderId,
    ) -> Result<(), BackendError> {
        let identity_path = ssh::identity::ensure_sandbox_identity(&self.app, name)?;
        ssh::identity::forget_sandbox_host_key(&identity_path);
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
            ProviderId::Podman => {
                ssh::identity::ensure_sandbox_alias(name, &self.connection, &identity_path).await?
            }
        }
        tracing::info!(target: "backends", %name, "ssh key installed and alias written");
        Ok(())
    }

    /// Undoes a create that failed once its instance existed, so a same-name retry starts clean.
    /// Every step is best-effort and warn-logged; the caller's error is what surfaces.
    async fn rollback_create(
        &self,
        provider: &Arc<dyn mio_provider::types::MachineProvider>,
        name: &str,
    ) {
        // The instance goes first: it is what blocks a same-name retry. The alias goes before the
        // key file it names.
        if let Err(e) = provider.remove(name).await {
            tracing::warn!(target: "backends", %name, error = %e, "rollback: could not remove the instance");
        }
        if let Err(e) = ssh::remove_alias(name) {
            tracing::warn!(target: "backends", %name, error = %e, "rollback: could not remove the ssh alias");
        }
        match crate::paths::sandbox_key_dir(&self.app, name) {
            Ok(dir) => match std::fs::remove_dir_all(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!(target: "backends", %name, dir = %dir.display(), error = %e, "rollback: could not remove the ssh identity directory"),
            },
            Err(e) => tracing::warn!(target: "backends", %name, error = %e, "rollback: could not resolve the ssh identity directory"),
        }
        crate::applied_policy::remove_applied(&self.app, name);
    }

    /// Settles the accepted policy from the create dialog alone; an existing workspace file stays a
    /// proposal and only its binding variables get placeholders
    /// (`docs/features/net-policy-review/02_dev.md`, Resolved Decision 5).
    fn prepare_egress(
        &self,
        name: &str,
        workspace: &str,
        request: Option<&types::EgressRequest>,
        caps: mio_provider::types::ProviderCapabilities,
    ) -> Result<(mio_provider::types::EgressPolicy, Vec<String>), BackendError> {
        use crate::applied_policy::{write_applied, AppliedPolicy};
        use mio_net_policy::schema::{load_if_present, save, starter, to_egress, NetPolicy};
        use mio_net_policy::validate::{stampable_vars, validate};
        let mut policy = NetPolicy::default();
        if let Some(mode) = request.and_then(|r| r.mode) {
            if mode == mio_provider::types::EgressMode::Allowlist {
                let hosts = request.map(|r| r.starter_hosts.clone()).unwrap_or_default();
                policy.rules = starter(caps, &hosts).rules;
            }
            policy.egress = mode;
        }
        validate(&policy, caps)?;
        // An unparsable file is left alone; the network section reports it after create.
        let (guest_vars, write_file) = match load_if_present(workspace) {
            Ok(Some(file)) => (stampable_vars(&file), false),
            Ok(None) => (Vec::new(), true),
            Err(_) => (Vec::new(), false),
        };
        if write_file {
            save(workspace, &policy)?;
        }
        let guest_env = caps.sidecar_egress.then(|| guest_vars.clone());
        let applied = AppliedPolicy { policy, revision: 0, guest_env };
        write_applied(&self.app, name, &applied)?;
        Ok((to_egress(&applied.policy)?, guest_vars))
    }

    /// Resolve where a freshly created sandbox's workspace lives. `None` derives and creates a
    /// fresh directory under `workspace_root()` (today's behavior). `Some(p)` canonicalizes `p`
    /// via `dunce::canonicalize` (plain `std::fs::canonicalize` would return a Windows `\\?\`
    /// UNC-prefixed path that `podman -v` does not understand), rejects it if not found or not a
    /// directory, and rejects it as `PathAlreadyTracked` if it falls inside `workspace_root()` or
    /// exactly matches an already-tracked sandbox's workspace — without touching the filesystem
    /// further.
    async fn resolve_create_workspace(
        &self,
        name: &str,
        existing_path: Option<&str>,
    ) -> Result<String, BackendError> {
        let Some(p) = existing_path else {
            // Pre-created so an unwritable path fails here, not silently via podman's
            // auto-creation of the mount source.
            let workspace = mio_domain::workspace_path(name)?;
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
        let root = mio_domain::workspace_root()?;
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
        let laptop_dir = mio_domain::workspace_path(name)?;
        let Some(info) = hosts::connection_info(&self.runner, connection).await? else {
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

/// A Podman bridge attachment: network, sandbox environment, and DNS server.
type Bridge = (String, Vec<(String, String)>, String);

/// Reduces a proxy attachment to the Podman bridge. A provider with its own attachment shape
/// adds a variant to `ProxyAttachment` and an arm here.
fn split_attachment(proxy: Option<&mio_provider::types::ProxyAttachment>) -> Option<Bridge> {
    use mio_provider::types::ProxyAttachment;
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
        proxy: Option<&mio_provider::types::ProxyAttachment>,
        egress: Option<&types::EgressRequest>,
    ) -> Result<Sandbox, BackendError> {
        mio_domain::validate_new_name(name)?;
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
            env.extend(guest_vars.iter().map(|v| (v.clone(), mio_net_policy::schema::placeholder(v))));
            None
        } else {
            None
        };
        let spec = InstanceSpec {
            name: name.to_string(),
            image,
            workspace_source: workspace.clone(),
            labels: vec![(mio_provider::WORKSPACE_LABEL.to_string(), workspace.clone())],
            env,
            network: bridge.map(|(network, _, dns)| mio_provider::types::InstanceNetwork {
                name: network,
                ip: None,
                dns: Some(dns),
            }),
            egress: egress_policy,
        };
        let id = match provider.create_instance(&spec).await {
            Ok(id) => id,
            Err(e) => {
                crate::applied_policy::remove_applied(&self.app, name);
                return Err(e.into());
            }
        };
        if let Err(e) = self.provision_exec_transport(&provider, name, provider_id).await {
            tracing::error!(target: "backends", %name, error = %e, "exec transport provisioning failed, rolling back create");
            self.rollback_create(&provider, name).await;
            return Err(e);
        }
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
        _proxy: Option<&mio_provider::types::ProxyAttachment>,
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
        mio_domain::validate_name(name)?;
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
        let info = check(podman_on(&self.runner, target_connection, &["info", "--format", "json"]).await?)?;
        hosts::check_remote_podman(target_connection, &info)?;
        Ok(DeployPlan { target: target_connection.to_string(), workspace: plan })
    }

    async fn deploy(
        &self,
        name: &str,
        deploy_plan: &DeployPlan,
        proxy: &mio_provider::types::ProxyAttachment,
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
            mio_provider::default_provider(self.runner.clone(), target_connection.to_string());

        // Stop the workload onto the remote; kept stopped (not removed) so undeploy can restart it.
        owner.stop(name).await?;
        // No committed-image transfer: software is reproduced from the mise manifest by
        // `RuntimeManager::reproduce_remote` after this returns. Must not be reintroduced.
        let image = Self::image();

        // Create the remote workspace and register the OpenSSH alias so Mutagen can reach it.
        Self::remote_mkdir(info, &plan.bind_mount_source).await?;
        ssh::write_alias(&ssh::types::SshAlias::Direct {
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
        let mio_provider::types::ProxyAttachment::PodmanBridge { network, env: bridge_env, dns } = proxy;
        let mut env = bridge_env.clone();
        env.extend(extra_env.iter().cloned());
        let spec = InstanceSpec {
            name: name.to_string(),
            image: image.clone(),
            workspace_source: plan.bind_mount_source.clone(),
            labels: Vec::new(),
            env,
            network: Some(mio_provider::types::InstanceNetwork {
                name: network.clone(),
                ip: None,
                dns: Some(dns.clone()),
            }),
            egress: None,
        };
        target_provider.create_instance(&spec).await?;
        target_provider.start(name).await?;
        let identity_path = ssh::identity::ensure_sandbox_identity(&self.app, name)?;
        ssh::identity::forget_sandbox_host_key(&identity_path);
        self.install_sandbox_key(&target_provider, name, &identity_path).await?;
        seed_sandbox_host_keys(&target_provider, name, &identity_path).await;
        ssh::identity::ensure_sandbox_alias(name, target_connection, &identity_path).await?;

        Ok(RemoteDeployment {
            alpha: plan.laptop_dir.clone(),
            beta: format!("{target_connection}:{}", plan.bind_mount_source),
        })
    }

    async fn remove_remote(&self, name: &str, connection: &str) -> Result<(), BackendError> {
        let target_provider =
            mio_provider::default_provider(self.runner.clone(), connection.to_string());
        Ok(target_provider.remove(name).await?)
    }

    async fn listen_events(&self, on_event: Channel<serde_json::Value>) -> Result<(), BackendError> {
        // `ProcessRunner` (`src-tauri/crates/mockctl/src/lib.rs`) models one-shot calls only, so the scripted-runner
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
        hosts::list(&self.runner).await
    }

    async fn add_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity: Option<&str>,
    ) -> Result<(), BackendError> {
        hosts::add(&self.runner, name, ssh_url, identity).await
    }

    async fn prepare_host(&self, ssh_url: &str, identity_path: &str) -> Result<(), BackendError> {
        hosts::prepare(ssh_url, identity_path).await
    }

    async fn provision_host(
        &self,
        name: &str,
        ssh_url: &str,
        identity_path: &str,
        on_progress: Channel<serde_json::Value>,
    ) -> Result<(), BackendError> {
        hosts::provision(&self.runner, name, ssh_url, identity_path, on_progress).await
    }
}
